// Navigation Manager - Manages preview navigation and device mode
//
// Iframe architecture:
// - This code runs in the main "preview" webview
// - Preview content is in an <iframe> element
// - Navigation tracking via iframe load events and IframeRPC
// - Back/forward commands use IframeRPC (postMessage to iframe)
//
// Also manages desktop/mobile preview mode toggle (device mode is a preview control).

import { info, warn, error, debug } from "../log";
import { extractDisplayPath } from "./path-utils";
import {
  createNavigationHistory,
  NavigationHistory,
} from "./navigation-history";
import { IframeRPC } from "./iframe-rpc";
import { pageSource } from "./page-source";
import type { PageInfo } from "@symbiosis-lab/moss-ui";
import { UrlAutocomplete } from "./url-autocomplete";
import { withShellMarker, stripShellMarker } from "./shell-marker";
import { applyAmbientBand, applySiteAccent } from "./ambient-band";
import { t, onLocaleChange } from "../i18n/index";
import { setThemeOverride } from "../shared/theme";
import { SwipeIndicator, type SwipePhasePayload } from "./swipe/swipe-indicator";
import { host } from "../host";
import { createGenerationToken } from "../shared/utils/generation-token";
import { effectiveChromeTheme } from "../utils/chrome-theme";

// Swipe messages arriving within this window after a navigation echo are
// trackpad momentum from the gesture that caused the navigation — one strong
// physical swipe must not chain multiple back/forwards across documents.
const SWIPE_NAV_COOLDOWN_MS = 600;

// Glass-chrome tokens that depend on the previewed backdrop (see tokens.css
// [data-chrome-theme] blocks). Mirrored inline at runtime for WebKit.
const CHROME_TOKEN_NAMES = [
  "--moss-chrome-icon-color",
  "--moss-chrome-pill-bg",
  "--moss-chrome-surface-bg",
  // The advisory panel's own theme. Every one of these must be listed or the
  // panel keeps its boot-time pole for the whole session: the mirror below is
  // what makes a [data-chrome-theme] flip re-resolve at all in WebKit, and a
  // token missing from here is simply never written.
  "--moss-chrome-panel-bg",
  "--moss-chrome-panel-fg",
  "--moss-chrome-panel-fg-secondary",
  "--moss-chrome-panel-fg-muted",
  "--moss-chrome-panel-accent",
  "--moss-chrome-panel-accent-quiet",
  "--moss-chrome-panel-danger",
  "--moss-chrome-panel-broken",
  "--moss-chrome-panel-degraded",
  "--moss-chrome-panel-border",
  "--moss-chrome-panel-hover",
  // Depth on the same axis: the level strings are computed on the probe (their
  // inputs substituted there), so only the two levels need mirroring.
  "--moss-chrome-elevation-1",
  "--moss-chrome-elevation-2",
] as const;
// Chrome glyph elements whose (inherited) `color` feeds the icon/SVG currentColor.
const CHROME_GLYPH_SELECTOR =
  ".moss-edit-button, .moss-device-toggle, .moss-nav-button, " +
  ".moss-control-panel .moss-icon-button, .moss-control-panel button, .moss-url-text";

/**
 * Canonical record of what page the preview is currently showing.
 *
 * - `url`: the full URL as echoed by the bridge via moss-navigation
 *   (includes query/hash). Identity checks compare pathnames only.
 * - `sourceFile`: the absolute source-file path backing this URL, read
 *   from the page source's answer; `null` until that answer lands.
 */
export type ActivePage = { url: string; sourceFile: string | null };

/**
 * Compare two full URLs by pathname only, ignoring query string and hash.
 * Returns `false` if either URL fails to parse.
 *
 * Used for identity validation of scroll-sync messages: cache-bust query
 * strings (?_t=1234) and hash fragments (#section) must not defeat the
 * match.
 */
export function sameUrlPathname(a: string, b: string): boolean {
  try {
    return new URL(a).pathname === new URL(b).pathname;
  } catch {
    return false;
  }
}

/** Parse a URL string safely, returning null if invalid. */
function tryParseUrl(url: string): URL | null {
  try { return new URL(url); } catch { return null; }
}

/**
 * Navigation event from preview webview
 */
interface PreviewNavigatedEvent {
  url: string;
  title?: string;
  /**
   * True when the loaded page is the preview server's synthetic 404 page
   * (marked via `<meta name="moss-not-found">`). Drives the rename 404-retry.
   * Absent on the non-HTML fallback path and on older bridge bundles.
   */
  notFound?: boolean;
  /**
   * The document's REAL navigation type from the bridge
   * (PerformanceNavigationTiming: "navigate" | "back_forward" | "reload").
   * Present only on document-load echoes — popstate/hashchange
   * re-notifications omit it (the performance entry describes the original
   * load, not the same-document traversal). When present it outranks
   * pendingNavType guessing: a stack that classifies echoes by what the
   * iframe actually did cannot drift from the iframe's session history.
   */
  navType?: string;
}

/** Reported to {@link NavigationManager.setNavigationResultListener} after every
 *  navigation the bridge echoes — used by the shell to retry a rename that
 *  raced ahead of the moved page being served. */
export interface NavigationResultInfo {
  /** URL the preview navigated to, shell marker stripped (query preserved). */
  url: string;
  /** True when that page is the server's synthetic 404. */
  notFound: boolean;
}

/**
 * Manages back/forward navigation for the preview iframe
 *
 * This class:
 * - Tracks navigation history (persists across iframe navigations)
 * - Sends navigation commands to iframe via IframeRPC
 * - Updates URL display when iframe navigates
 * - Manages back/forward button disabled states based on navigation history
 * - Manages desktop/mobile preview mode toggle
 */
export class NavigationManager {
  private backButton: HTMLButtonElement | null;
  private forwardButton: HTMLButtonElement | null;
  private urlText: HTMLElement | null;

  // Iframe and RPC.
  //
  // IMPORTANT: The preview iframe is CROSS-ORIGIN.
  // - Parent (app shell) runs at tauri://localhost
  // - Iframe runs at http://localhost:8080 (preview server)
  //
  // This means iframe.contentDocument and iframe.contentWindow.document
  // are inaccessible — any attempt silently throws a SecurityError.
  // Likewise, MutationObserver cannot observe the iframe's DOM.
  //
  // All communication MUST go through postMessage:
  // - parent → iframe: use IframeRPC (see preview/iframe-rpc.ts)
  //   or raw postMessage for custom messages (e.g. moss-asset-ready)
  // - iframe → parent: the iframe's preview.js posts messages like
  //   moss-navigation, moss-page-color, moss-source-target,
  //   moss-preview-pointerdown, etc.
  //   Listen via window.addEventListener("message", ...) below.
  //
  // DO NOT attempt iframe.contentDocument — it will not work and the
  // try/catch will swallow the error silently, causing subtle bugs.
  private iframe: HTMLIFrameElement | null;
  private rpc: IframeRPC | null = null;
  // Observes the shell <html data-theme> and mirrors it into the preview iframe,
  // so the preview always matches the shell's theme (moon SSOT). Disconnected in destroy().
  private themeObserver: MutationObserver | null = null;
  // Last backdrop color the preview reported (via moss-page-color), and the
  // committed chrome appearance derived from it. The chrome theme tracks the
  // previewed page's backdrop, independent of the OS/app data-theme.
  private lastPageColor: string | null = null;
  private chromeTheme: "light" | "dark" | null = null;
  // Last site accent the preview reported (via moss-site-accent). A per-SITE
  // fact: survives same-site navigation, cleared in reset() and by a page
  // that posts an explicit null. Stored raw so app theme flips can re-derive
  // the tint (lightness comes from the app surface, not the accent).
  private lastSiteAccent: string | null = null;

  // Navigation history tracked in parent window
  private history: NavigationHistory;
  private pendingNavType: "back" | "forward" | null = null;
  // The one deadline every navigation lives under (armNavDeadline). Back and forward
  // arm it short, with a hook that drops the phantom history entries.
  private navDeadline: ReturnType<typeof setTimeout> | undefined;
  private static readonly TRAVERSAL_WATCHDOG_MS = 2500;
  // Long on purpose: the build is told the intended page so it can hurry a slow or
  // cloud-held page; giving up early would point it back at the old page mid-load.
  private static readonly NAV_DEADLINE_MS = 30000;

  // When the last navigation echo arrived. Swipe messages inside the
  // cooldown window after it are the momentum tail of the swipe that caused
  // that navigation (the trackpad keeps emitting for ~1s after fingers
  // lift), not a new gesture.
  private lastNavigationAt = 0;

  // Interactive swipe UI (arrow chips + native phase stream on macOS).
  private swipeIndicator!: SwipeIndicator;

  // Title/path toggle state.
  //
  // `currentPath` is DERIVED from activePage so there is exactly one source of
  // truth for the URL we're displaying. Any future rewriter that updates
  // activePage automatically keeps the displayed path in sync — no second
  // field to forget.
  private get currentPath(): string {
    return this.activePage ? extractDisplayPath(this.activePage.url) : "/";
  }
  private currentTitle: string = "";
  private displayMode: "path" | "title" = "title";

  /**
   * Optional observer notified after every bridge-echoed navigation, carrying
   * whether the loaded page was the server's synthetic 404. PreviewManager uses
   * it to drive the rename 404-retry. There is exactly one consumer, so a single
   * slot (not an array) keeps the contract obvious.
   */
  private navigationResultListener: ((info: NavigationResultInfo) => void) | null = null;
  private previewRequirementListener: ((urlPath: string) => void) | null = null;
  // URL autocomplete
  private autocompleteContainer: HTMLElement | null = null;
  private autocomplete: UrlAutocomplete | null = null;
  private isAutocompleteOpen: boolean = false;

  // Device mode (desktop/mobile preview)
  private previewContainer: HTMLElement | null;
  private toggleButton: HTMLElement | null;
  private isMobileMode: boolean = false;
  /**
   * Optional callback invoked after every device-mode transition
   * (desktop → mobile or mobile → desktop). PreviewManager wires this to
   * re-apply the current zoom so the correct transform branch is used after
   * a toggle. Single-slot (one consumer) — same pattern as navigationResultListener.
   */
  private deviceModeChangeListener: ((isMobile: boolean) => void) | null = null;

  // Folder path for article detection (set by PreviewManager)
  private folderPath: string = "";

  // Cached editor scroll position for post-rebuild restoration.
  // The parent owns this state; the bridge is stateless.
  private lastEditorPosition: { line: number; anchorId: string | null; atTop: boolean; atBottom: boolean } | null = null;

  // Tracks who last took ownership of the preview scroll position.
  // 'editor' = editor line sync drives the preview (default).
  // 'user'   = a deliberate user hash-nav holds the position; editor sync is
  //            suppressed until the user next scrolls the editor.
  //
  // Invariant: any call site that calls scrollToSourceLine on behalf of the
  // editor must also set this to 'editor' (via claimScrollForEditor or inline).
  private scrollIntent: 'editor' | 'user' = 'editor';

  /** The page the preview is currently showing; its source is the page source's answer. */
  private activePage: { url: string } | null = null;
  private readonly unsubscribePageSource: () => void;
  private readonly pageRequest = createGenerationToken();
  private requestedUrl: string | null = null;
  private morphSequence = 0;

  /** Record intent before assigning iframe.src, so late refresh work loses authority. */
  beginNavigation(url: string): void {
    this.pageRequest.bump();
    this.requestedUrl = stripShellMarker(url);
    pageSource.invalidate(extractDisplayPath(url));
    this.reportPreviewRequirement();
    this.armNavDeadline(NavigationManager.NAV_DEADLINE_MS);
  }

  /** The one deadline; a newer call replaces it. Expiry runs the hook, then abandons the navigation. */
  private armNavDeadline(ms: number, onExpire?: () => void): void {
    clearTimeout(this.navDeadline);
    const request = this.pageRequest.current();
    this.navDeadline = setTimeout(() => {
      if (this.pageRequest.isStale(request) || this.requestedUrl === null) return;
      onExpire?.();
      this.requestedUrl = null;
      this.pageRequest.bump();
      this.followShownPage();
      info(`[NavigationManager] navigation abandoned: no page reported in within ${ms}ms`);
      this.reportPreviewRequirement();
    }, ms);
  }

  /** The page source answers for the page still on screen once a navigation is abandoned. */
  private followShownPage(): void {
    const shown = this.activePage ? extractDisplayPath(this.activePage.url) : null;
    pageSource.invalidate(shown);
    if (shown && this.folderPath) void pageSource.ensure(shown);
  }

  /** Tell the build which page to watch: the intended page while a navigation is pending, else the shown page. */
  private reportPreviewRequirement(): void {
    const url = this.requestedUrl ?? this.activePage?.url;
    if (url) this.previewRequirementListener?.(extractDisplayPath(url));
  }

  getRequestRevision(): number { return this.pageRequest.current(); }
  isNavigationPending(): boolean { return this.requestedUrl !== null; }

  /** Public read access for identity validation and testing. */
  getActivePage(): ActivePage | null {
    if (!this.activePage) return null;
    const { url } = this.activePage;
    return { url, sourceFile: pageSource.sourceFile(extractDisplayPath(url)) };
  }

  // Echo-suppression token: when the editor (parent) drives a navigation by
  // setting iframe.src, the resulting moss-navigation bounce-back should be
  // treated as an acknowledgment, not as a new user navigation. The
  // coordinator calls markEditorNavigation(url) BEFORE issuing the nav.
  private pendingEcho: { url: string; expiresAt: number } | null = null;
  /**
   * TTL for pendingEcho tokens. Sized to cover the typical iframe.src → bridge
   * load → moss-navigation round-trip on modern hardware (well under 300ms in
   * practice). Past this point, the navigation likely failed (non-HTML content,
   * network error, bridge crash) and the token is cleared so the next user
   * navigation isn't misclassified as an echo.
   */
  private static readonly ECHO_TTL_MS = 1000;

  /** The single `<footer>` on a previewed page. `data-source-none` is stamped
   *  only in preview builds (`crates/moss-build/src/build/page/shell.rs`), so this selector can
   *  never match published output. See {@link revealFooter}. */
  private static readonly FOOTER_SELECTOR = 'body > footer[data-source-none]';
  /** Duration of the reveal wash. Written INTO the `animation` shorthand (not
   *  read from the stylesheet), so this is the only place the duration is
   *  stated; the keyframes it drives are in `crates/moss-build/src/ops/serve/iframe_bridge.rs`. */
  private static readonly REVEAL_MS = 1400;
  /** Identifies the current reveal so a superseded one's cleanup timer cannot
   *  clear a newer wash. */
  private revealToken = 0;
  /** Class carrying the hover-driven wash (chip hover → preview flash). The
   *  rules live server-side next to the `moss-slot-reveal` keyframes in
   *  `crates/moss-build/src/ops/serve/iframe_bridge.rs`; keep the name in lockstep with them. */
  private static readonly FM_FLASH_CLASS = 'moss-fm-flash';
  /** One-shot: reveal the footer once the page at `url` is up. Set when the
   *  reveal has to cross a page change (the preview was showing a different
   *  language tree), because the RPC target does not exist until the new page's
   *  bridge is running.
   *
   *  Keyed by URL and TTL-bounded for the same reason `pendingEcho` is: a
   *  navigation can be superseded before its echo arrives. Opening `footer.md`
   *  and then clicking an article inside that window produces exactly one echo,
   *  for the article — and an unkeyed arm would consume it and flash the
   *  article's footer, which the user asked for nothing about.
   *
   *  Consumed exactly once — a reveal answers a click, it is not a state to
   *  re-assert on every rebuild. */
  private pendingFooterReveal: { url: string; expiresAt: number } | null = null;

  /** Called by coordinator BEFORE initiating an editor-driven navigation. */
  markEditorNavigation(url: string): void {
    this.pendingEcho = { url, expiresAt: Date.now() + NavigationManager.ECHO_TTL_MS };
  }

  /** Register the (single) observer notified after each bridge-echoed
   *  navigation. Pass `null` to clear. See {@link NavigationResultInfo}. */
  setNavigationResultListener(cb: ((info: NavigationResultInfo) => void) | null): void {
    this.navigationResultListener = cb;
  }

  /** Report the page the preview requests; Rust resolves its source. */
  setPreviewRequirementListener(cb: ((urlPath: string) => void) | null): void {
    this.previewRequirementListener = cb;
  }

  /** Build a full URL for `urlPath` using the current iframe origin.
   *  Returns null when no iframe origin is available (iframe unmounted or bad src).
   *  Used by both resolveFullUrl and navigateToPage to ensure the URL marked for
   *  echo-suppression matches the URL that will actually be navigated to.
   */
  private buildNavigationUrl(urlPath: string): string | null {
    if (!this.iframe?.src) return null;
    try {
      const base = new URL(this.iframe.src);
      const normalized = urlPath.startsWith("/") ? urlPath.slice(1) : urlPath;
      return `${base.protocol}//${base.host}/${normalized}`;
    } catch {
      return null;
    }
  }

  /**
   * Resolve a path (e.g. "/posts/hello/") to a full URL using current iframe origin.
   * Used to pre-compute the URL the bridge will echo back via moss-navigation,
   * so that markEditorNavigation records the exact URL consumePendingEchoIfMatch
   * will see.
   */
  resolveFullUrl(urlPath: string): string | null {
    return this.buildNavigationUrl(urlPath);
  }

  /** Public for testing; internal consumers use consumePendingEchoIfMatch. */
  hasPendingEchoFor(url: string): boolean {
    if (!this.pendingEcho) return false;
    if (Date.now() > this.pendingEcho.expiresAt) {
      this.pendingEcho = null;
      return false;
    }
    return this.pendingEcho.url === url;
  }

  private consumePendingEchoIfMatch(url: string): boolean {
    if (this.hasPendingEchoFor(url)) {
      this.pendingEcho = null;
      return true;
    }
    return false;
  }

  constructor(iframe: HTMLIFrameElement | null) {
    this.iframe = iframe;
    // The build is asked to resolve again whenever the lookup learns a different source for the page.
    this.unsubscribePageSource = pageSource.subscribe((page, previous) => {
      if (this.activePage && page.sourcePath !== (previous?.sourcePath ?? null)) this.previewRequirementListener?.(page.urlPath);
    });

    // The mode-dependent tooltip is written imperatively — re-render on locale
    // switch or it stays in the previous language (mixed-language chrome bug).
    onLocaleChange(() => this.updateToggleButton());

    if (this.iframe) {
      this.rpc = new IframeRPC(this.iframe);
      // Mirror live shell-theme changes (moon toggle, OS change) into the iframe.
      this.themeObserver = new MutationObserver(() => {
        this.pushThemeToIframe(this.currentShellTheme());
        // A backdrop measured under the theme we just left is no evidence about
        // this one, and applyChromeTheme resolves `next ?? shell` — so a stale
        // color outranks the chosen theme until the bridge re-reports. Drop it.
        this.lastPageColor = null;
        this.chromeTheme = null;
        this.applyChromeTheme();
        // The identity tint's lightness comes from the app surface, so an
        // app theme flip re-derives it from the stored accent (the bridge
        // will also re-post after its own data-theme settles; both writes
        // converge on the same value).
        applySiteAccent(document.documentElement, this.lastSiteAccent, this.currentShellTheme());
      });
      this.themeObserver.observe(document.documentElement, {
        attributes: true, attributeFilter: ["data-theme"],
      });
    }

    this.history = createNavigationHistory();

    this.backButton = document.getElementById(
      "btn-nav-back"
    ) as HTMLButtonElement;
    this.forwardButton = document.getElementById(
      "btn-nav-forward"
    ) as HTMLButtonElement;
    this.urlText = document.getElementById("moss-url-text");

    // Device toggle button (in title bar)
    this.toggleButton = document.querySelector(".moss-device-toggle");
    this.previewContainer = document.querySelector(".moss-preview-container");

    this.backButton?.addEventListener("click", () => this.goBack());
    this.forwardButton?.addEventListener("click", () => this.goForward());
    // Single-click to toggle title/path, double-click to enter edit mode
    this.urlText?.addEventListener("click", () => this.toggleDisplayMode());
    this.urlText?.addEventListener("dblclick", (e) => {
      e.preventDefault();
      this.enterAutocompleteMode();
    });
    this.toggleButton?.addEventListener("click", () => this.togglePreviewMode());

    this.setupAutocompleteContainer();

    this.loadModePreference();

    // Listen for navigation events from iframe
    this.setupIframeNavigationListener();

    // Seed data-chrome-theme from data-theme (no backdrop color yet, so it
    // reflects the intended theme).
    this.applyChromeTheme();

    // Interactive trackpad swipe navigation (arrow chips + native NSEvent
    // phases on macOS). When its stream is live it owns the gesture and the
    // heuristic moss-swipe-nav fallback below is suppressed.
    this.swipeIndicator = new SwipeIndicator({
      container: this.previewContainer ?? document.body,
      iframe: this.iframe,
      navigate: {
        canGoBack: () => this.history.canGoBack(),
        canGoForward: () => this.history.canGoForward(),
        goBack: () => this.goBack(),
        goForward: () => this.goForward(),
      },
      subscribe: (onEvent: (e: SwipePhasePayload) => void) =>
        host.swipePhases?.<SwipePhasePayload>(onEvent) ?? Promise.resolve(false),
    });
    void this.swipeIndicator.start();
  }

  // Track pending navigation for timeout fallback
  private navigationTimeout: number | undefined;

  /**
   * Setup listener for iframe navigation events
   * Uses postMessage (moss-navigation) as the authoritative source of navigation events.
   *
   * Note: We intentionally do NOT use iframe.load events because:
   * 1. iframe.src stays at the programmatically-set URL even when user navigates via links
   * 2. This causes race conditions when switching folders (old URL fires before new content)
   * 3. The bridge script in the iframe is the authoritative source via moss-navigation
   */
  /** Push the current shell theme into the preview iframe (cross-origin postMessage). */
  private pushThemeToIframe(theme: "light" | "dark"): void {
    this.iframe?.contentWindow?.postMessage({ type: "moss-set-theme", theme }, "*");
  }

  /**
   * Push the device-preview mode into the iframe (cross-origin postMessage).
   *
   * Nothing consumes this today: the shell insets the iframe, and site.css defines no rule
   * for either class. The message still flows, so a real consumer can return without new plumbing.
   * If device mode gains a consumer it must stay a message, not a width check: a desktop
   * preview dragged to 390px matches the phone frame exactly.
   */
  private pushDeviceModeToIframe(mobile: boolean): void {
    this.iframe?.contentWindow?.postMessage(
      { type: "moss-set-device-mode", mobile },
      "*",
    );
  }

  /** The shell's current theme from <html data-theme>, defaulting to light. */
  private currentShellTheme(): "light" | "dark" {
    return document.documentElement.getAttribute("data-theme") === "dark" ? "dark" : "light";
  }

  /**
   * Refresh <html data-chrome-theme> from the last reported backdrop color so
   * the floating glass chrome stays legible over the previewed page. Falls back
   * to mirroring the intended (OS/moon) theme when no/invalid color is known.
   */
  private applyChromeTheme(): void {
    const next = effectiveChromeTheme(this.lastPageColor, this.chromeTheme);
    const resolved = next ?? this.currentShellTheme();
    // Preserve the hysteresis baseline across null/cross-origin gaps — only
    // commit a real derivation, never null it out.
    if (next !== null) this.chromeTheme = next;
    const root = document.documentElement;
    root.setAttribute("data-chrome-theme", resolved);

    // WebKit does NOT reliably re-resolve `var()`-based properties when a
    // data-* attribute selects a different token block at RUNTIME (the same
    // limitation that forces pre-paint theming in index.html). An inline
    // custom-property change DOES re-resolve (this is how --moss-page-color
    // drives the glass surface live), so mirror the resolved chrome tokens
    // inline. Values come from a probe so they stay sourced from tokens.css.
    const vals = this.readChromeTokens(resolved);
    for (const [name, value] of Object.entries(vals)) if (value) root.style.setProperty(name, value);
    // The inherited `color` is the stickiest case in WebKit; set it directly on
    // the glyph elements (their SVGs use currentColor / a var stroke).
    const iconColor = vals["--moss-chrome-icon-color"];
    if (iconColor) {
      for (const el of document.querySelectorAll<HTMLElement>(CHROME_GLYPH_SELECTOR)) {
        el.style.color = iconColor;
      }
    }
  }

  /**
   * Read the glass-chrome token values for a theme from an isolated, detached
   * probe (driven by the [data-chrome-theme] blocks in tokens.css) so the
   * values stay DRY and are not masked by the inline :root overrides we write.
   */
  private readChromeTokens(theme: "light" | "dark"): Record<string, string> {
    const probe = document.createElement("div");
    probe.style.display = "none";
    probe.setAttribute("data-chrome-theme", theme);
    document.body.appendChild(probe);
    const cs = getComputedStyle(probe);
    const out = Object.fromEntries(CHROME_TOKEN_NAMES.map((name) => [name, cs.getPropertyValue(name).trim()]));
    probe.remove();
    return out;
  }

  private setupIframeNavigationListener(): void {
    if (!this.iframe) {
      warn("⚠️ [NavigationManager] No iframe to listen to");
      return;
    }

    // Listen for navigation messages from iframe (via bridge script)
    // This is the ONLY source of navigation events - see notes above
    window.addEventListener("message", (event) => {
      if (event.data?.type === "moss-navigation") {
        this.handleNavigationUpdate(event.data);
      }

      // Trackpad swipe over the preview (detected in the iframe bridge —
      // wheel events never reach the shell). Same path as the titlebar
      // buttons, so canGoBack/canGoForward gating applies unchanged.
      // All content-message handlers below share isFromPreview().
      // Guards: only the preview iframe itself may navigate (its content is
      // untrusted site JS; nested embeds can post via window.top); a swipe
      // arriving within the post-navigation cooldown is the momentum tail of
      // the swipe that caused the navigation, not a new gesture; and
      // localStorage `moss.preview.swipe`="0" is the kill-switch (same
      // convention as `moss.preview.morph`).
      if (
        event.data?.type === "moss-swipe-nav" &&
        !this.swipeIndicator?.nativeActive &&
        this.isFromPreview(event) &&
        Date.now() - this.lastNavigationAt >= SWIPE_NAV_COOLDOWN_MS &&
        localStorage.getItem("moss.preview.swipe") !== "0"
      ) {
        if (event.data.direction === "back") {
          this.goBack();
        } else if (event.data.direction === "forward") {
          this.goForward();
        }
      }

      // Page color from iframe (cross-origin, must use postMessage).
      // Source-checked like the swipe and ambient handlers: a nested embed
      // can post to window.top regardless of sender-side gating, and this
      // value lands in --moss-page-color, which the glass chrome and the
      // ambient band's fallback both consume.
      if (
        event.data?.type === "moss-page-color" &&
        this.isFromPreview(event)
      ) {
        const color = event.data.color;
        if (color && /^(#[\da-fA-F]{3,8}|rgba?\([^)]+\)|hsla?\([^)]+\)|[a-zA-Z]+)$/.test(color)) {
          document.documentElement.style.setProperty("--moss-page-color", color);
          this.lastPageColor = color;
        } else {
          document.documentElement.style.removeProperty("--moss-page-color");
          this.lastPageColor = null;
        }
        // Re-derive the floating-chrome appearance from the new backdrop.
        this.applyChromeTheme();
      }

      // Ambient band sampled off the page's top strip; validated in
      // ambient-band.ts, which owns the trust boundary. Source-checked for the
      // same reason the swipe handler above is — gating the sampler on
      // shell-mount gates the *sender*, and a nested embed can post to
      // window.top regardless.
      if (
        event.data?.type === "moss-chrome-ambient" &&
        this.isFromPreview(event)
      ) {
        applyAmbientBand(document.documentElement, event.data.colors);
      }

      // Site identity (accent) from the shell-mounted bridge. Validated and
      // derived in ambient-band.ts and site-tint.ts, which own the trust
      // boundary. Nulls pass through deliberately: a token-less page clears
      // the previous page's sticky tint.
      if (
        event.data?.type === "moss-site-accent" &&
        this.isFromPreview(event)
      ) {
        this.lastSiteAccent = typeof event.data.color === "string" ? event.data.color : null;
        applySiteAccent(document.documentElement, this.lastSiteAccent, this.currentShellTheme());
      }

      // The site's moon is the shell's only theme input (shared/theme.ts);
      // the request is the bridge's cold-start ask (see iframe-bridge.ts).
      if (this.isFromPreview(event)) {
        const { type, theme } = event.data ?? {};
        if (type === "moss-theme-change" && (theme === "light" || theme === "dark")) setThemeOverride(theme);
        if (type === "moss-theme-request") this.pushThemeToIframe(this.currentShellTheme());
      }
    });

    // Fallback for non-HTML content (RSS, XML) where bridge script doesn't load
    // We listen for iframe load and start a timeout - if no moss-navigation received
    // within 500ms, use iframe.src as fallback
    this.iframe.addEventListener("load", () => {
      // Keep a freshly-served preview in lockstep with the shell's theme.
      this.pushThemeToIframe(this.currentShellTheme());
      // Likewise re-assert device mode.
      this.pushDeviceModeToIframe(this.isMobileMode);
      // Drop the previous document's ambient band; the new page's sampler
      // reports within a frame or two (its load listener re-arms the dedup
      // so the re-post is never swallowed — see chrome-ambient.ts). A page
      // whose top strip resolves to nothing never posts at all — without
      // this its chrome would keep the *last* page's colours indefinitely.
      // The site accent deliberately survives this boundary: identity is
      // per-site, content light is per-page.
      applyAmbientBand(document.documentElement, null);
      if (this.requestedUrl) {
        const armed = this.pageRequest.current();
        clearTimeout(this.navigationTimeout);
        this.navigationTimeout = window.setTimeout(() => {
          // Confirm only the navigation that armed this, and only if iframe.src
          // is the page it intended: after a reload of the outgoing page, src
          // names the intended page while the document is the old one.
          if (
            this.iframe?.src && this.requestedUrl && !this.pageRequest.isStale(armed) &&
            this.sameRoute(this.iframe.src, this.requestedUrl)
          ) {
            info("✅ [NavigationManager] Fallback: using iframe.src for non-HTML content");
            // `synthesized`: this is a guess from iframe.src, not a real bridge
            // report — it has no notFound info (title is ""). Must NOT drive the
            // rename 404-retry: a slow 404's bridge echo could otherwise be
            // pre-empted by this false `notFound:false`, killing the retry.
            this.handleNavigationUpdate({ url: this.iframe.src, title: "" }, { synthesized: true });
          }
        }, 500);
      }
    });
  }

  /**
   * True only for messages genuinely sent by the current preview document.
   *
   * The null check is load-bearing: a detached iframe has
   * `contentWindow === null`, and a null `event.source` (closed sender,
   * MessagePort) would satisfy bare equality by null === null — so a
   * torn-down shell could still be steered. `contentWindow` is read live
   * per message, never cached, so a reload can't leave a stale match.
   */
  private isFromPreview(event: MessageEvent): boolean {
    return event.source !== null && event.source === this.iframe?.contentWindow;
  }

  /**
   * Clean up event listeners
   */
  destroy(): void {
    this.pageRequest.bump();
    this.unsubscribePageSource();
    pageSource.invalidate();
    this.swipeIndicator?.dispose();
    this.rpc?.destroy();
    this.autocomplete?.destroy();
    this.autocompleteContainer?.remove();
    this.themeObserver?.disconnect(); // stop mirroring theme into a torn-down iframe
  }

  /**
   * Reset navigation state for a new folder.
   *
   * Called when switching between website folders (e.g., via context menu).
   * This clears the navigation history so the new folder starts fresh,
   * preventing stale paths and broken back/forward buttons.
   */
  reset(): void {
    this.pageRequest.bump();
    clearTimeout(this.navDeadline);
    clearTimeout(this.navigationTimeout);
    this.requestedUrl = null;
    pageSource.invalidate();
    // Create fresh history - clears all entries and _initialized flag
    this.history = createNavigationHistory();

    // Reset display state (currentPath derives from activePage, reset below)
    this.currentTitle = "";
    this.pendingNavType = null;
    this.lastEditorPosition = null;
    this.scrollIntent = 'editor';
    this.activePage = null;
    this.pendingEcho = null;
    this.pendingFooterReveal = null;

    // Drop the previous site's backdrop so its chrome theme doesn't carry into
    // the new folder through the load gap; mirror the intended theme until the
    // new site reports its page color.
    this.lastPageColor = null;
    this.chromeTheme = null;
    // Same boundary for the identity tint: the accent is a per-site fact and
    // this is the site switch. (It deliberately does NOT clear on iframe load
    // — clearing per navigation would flash the chrome to neutral on every
    // page change within a site.)
    this.lastSiteAccent = null;
    applySiteAccent(document.documentElement, null, this.currentShellTheme());
    this.applyChromeTheme();

    // Update UI to reflect reset state
    this.updateDisplay();
    this.updateButtonStates(false, false);

    info("✅ [NavigationManager] Reset for new folder");
  }

  private handleNavigationUpdate(
    data: PreviewNavigatedEvent,
    opts: { synthesized?: boolean } = {}
  ): void {
    this.lastNavigationAt = Date.now();
    // Strip the shell marker (`__moss_shell=1`) so it never enters
    // history, echo matching, display paths, or the PREVIEW_URL_CHANGED
    // event payload. The marker is a private shell↔server side-channel;
    // see `packages/moss-ui/src/ui/preview/shell-marker.ts` for the contract.
    const currentUrl = stripShellMarker(data.url);
    // A replaced document can still echo while its successor is loading.
    if (this.requestedUrl && !this.sameRoute(currentUrl, this.requestedUrl)) return;
    // Confirmed. A mismatched echo (above) changes nothing and leaves the deadline armed.
    clearTimeout(this.navDeadline);
    clearTimeout(this.navigationTimeout);
    if (!this.requestedUrl && currentUrl !== this.getCurrentUrl()) this.pageRequest.bump();
    this.requestedUrl = null;
    // If this message is the echo of a navigation we just issued, consume the
    // token. We still update display + activePage below; the isEcho flag
    // suppresses new-navigation bookkeeping further down.
    // The page shown until now. History can lag behind it: a page the editor
    // opened is shown without being recorded.
    const historyUrl = this.history.getCurrentUrl();
    const previousUrl = this.activePage?.url ?? historyUrl;
    const isEcho = this.consumePendingEchoIfMatch(currentUrl);

    // Detect hash-fragment navigation on the same pathname: user clicked a
    // #section link on the current page. This is a deliberate scroll act —
    // transfer ownership to the user so the editor sync doesn't clobber it.
    //
    // Conditions: URL must have *changed* (not a rebuild of the same URL),
    // pathname must match, and the new URL must have a hash.
    // First-ever nav (previousUrl === null) is never same-pathname, so safe.
    //
    // Both isEcho and isHashNavOnSamePage are resolved before any scrollIntent
    // mutation below: if an editor echo happens to target a same-pathname hash
    // URL, isHashNavOnSamePage fires 'user' here, but the echo branch at the
    // bottom of this method immediately corrects it back to 'editor'.
    const currentUrlObj = tryParseUrl(currentUrl);
    const previousUrlObj = previousUrl ? tryParseUrl(previousUrl) : null;
    const isHashNavOnSamePage =
      currentUrl !== previousUrl &&
      currentUrlObj !== null &&
      previousUrlObj !== null &&
      currentUrlObj.pathname === previousUrlObj.pathname &&
      currentUrlObj.hash !== '';
    if (isHashNavOnSamePage) {
      this.scrollIntent = 'user';
    }

    // Whether this update is a same-URL RELOAD (rebuild echo) rather than a
    // genuine navigation. Set true ONLY in the same-URL branch below, which is
    // unreachable on first load (that path goes through the `!_initialized`
    // branch). Threaded into the PREVIEW_URL_CHANGED event so app.ts can
    // suppress the editor:load-source forward on a reload.
    let isReload = false;

    // Track navigation in parent-side history
    if (!this.history._initialized) {
      // First navigation - set as initial state (not a "new" navigation)
      this.history._setInitialUrl(currentUrl);
      debug(`✅ [NavigationManager] Initial URL set: ${currentUrl}`);
    } else if (currentUrl !== previousUrl && currentUrl !== historyUrl) {
      const backUrl =
        this.history._index > 0 ? this.history._stack[this.history._index - 1] : null;
      const fwdUrl =
        this.history._index < this.history._stack.length - 1
          ? this.history._stack[this.history._index + 1]
          : null;
      if (data.navType === "back_forward") {
        // The iframe REALLY traversed — move the index to the matching
        // neighbor. A traversal never truncates forward history. The
        // pendingNavType hint breaks ties when both neighbors match.
        if (this.pendingNavType === "back" && backUrl === currentUrl) {
          this.history._index--;
        } else if (this.pendingNavType === "forward" && fwdUrl === currentUrl) {
          this.history._index++;
        } else if (backUrl === currentUrl) {
          this.history._index--;
        } else if (fwdUrl === currentUrl) {
          this.history._index++;
        } else {
          // Not a neighbor: multi-step traversal (history.go(n)) or URL
          // drift. Scan the whole stack — moving the index never truncates.
          const idx = this.history._stack.lastIndexOf(currentUrl);
          if (idx >= 0) {
            this.history._index = idx;
          } else if (!isEcho) {
            // Truly unknown traversal target (stack drifted earlier) —
            // last resort resync by pushing.
            this.history._onNewNavigation(currentUrl);
          }
        }
        debug(`✅ [NavigationManager] Traversal (real back_forward) to: ${currentUrl}`);
      } else if (data.navType === "navigate" || data.navType === "reload") {
        // The iframe REALLY loaded a new document — a stale pendingNavType
        // (its traversal silently never happened) must NOT be consumed as
        // one, or the index drifts from the iframe's session history.
        if (!isEcho) {
          this.history._onNewNavigation(currentUrl);
          debug(`✅ [NavigationManager] New navigation: ${previousUrl} -> ${currentUrl}`);
        } else {
          debug(`✅ [NavigationManager] Echo ack for editor-driven nav: ${currentUrl}`);
        }
      } else if (this.pendingNavType === "back") {
        // Legacy classification (no navType: same-document popstate/hash
        // echoes, non-HTML fallback, older bundles).
        this.history._index--;
        debug("✅ [NavigationManager] Back navigation completed");
      } else if (this.pendingNavType === "forward") {
        this.history._index++;
        debug("✅ [NavigationManager] Forward navigation completed");
      } else {
        if (!isEcho) {
          this.history._onNewNavigation(currentUrl);
          debug(`✅ [NavigationManager] New navigation: ${previousUrl} -> ${currentUrl}`);
        } else {
          debug(`✅ [NavigationManager] Echo ack for editor-driven nav: ${currentUrl}`);
        }
      }
      this.pendingNavType = null;
    } else if (currentUrl !== previousUrl) {
      // Back on the page history already points at: nothing to record.
      this.pendingNavType = null;
    } else {
      // Same URL — this is a reload (e.g., after rebuild). It is NOT a
      // navigation: mark it so downstream consumers can distinguish a rebuild
      // echo from a real page change (app.ts suppresses editor:load-source on
      // a reload so a home-page rebuild can't stomp a just-opened new file).
      isReload = true;
      // A same-URL echo can be the completion of a traversal whose target
      // equals the displayed URL (editor-driven navs update activePage
      // without pushing history). Whatever it was, the pending traversal is
      // over — a stale marker here would misclassify the next echo.
      this.pendingNavType = null;
      // Restore scroll
      // to the editor's last known position so the preview stays in sync —
      // but only when the editor owns the scroll. If the user clicked a
      // hash link (scrollIntent === 'user'), preserve that position.
      if (this.scrollIntent === 'editor' && this.lastEditorPosition) {
        debug(`[NavigationManager] Same-URL reload — restoring scroll to line ${this.lastEditorPosition.line} (atTop=${this.lastEditorPosition.atTop})`);
        this.scrollToSourceLine(this.lastEditorPosition.line, this.lastEditorPosition.anchorId, this.lastEditorPosition.atTop, this.lastEditorPosition.atBottom);
      } else {
        debug(`[NavigationManager] Same-URL reload, scroll preserved (intent=${this.scrollIntent})`);
      }
    }

    // currentPath derives from activePage, so this single assignment keeps the
    // displayed path in sync. Identity checks use the URL.
    this.activePage = { url: currentUrl };
    if (!isReload) this.reportPreviewRequirement();
    this.currentTitle = data.notFound ? "" : (data.title || "");
    this.updateDisplay();

    // A footer reveal that had to cross a page change: the new page's bridge is
    // up now, so the RPC has a target. Only for the page we actually armed for
    // — an echo from a navigation that superseded ours means the user moved on,
    // and revealing there would be an interruption rather than an answer. The
    // TTL retires an arm whose navigation never echoed at all.
    if (this.pendingFooterReveal) {
      const armed = this.pendingFooterReveal;
      const matches = sameUrlPathname(currentUrl, armed.url);
      if (matches || Date.now() > armed.expiresAt) {
        this.pendingFooterReveal = null;
      }
      if (matches) void this.revealFooter();
    }

    // Update button states from parent-side history
    const canGoBack = this.history.canGoBack();
    const canGoForward = this.history.canGoForward();
    debug(`[NavigationManager] Button states: canGoBack=${canGoBack}, canGoForward=${canGoForward}, historyLength=${this.history.length()}, historyIndex=${this.history.getActiveIndex()}`);
    this.updateButtonStates(canGoBack, canGoForward);

    // The page source announces the page's answer (PREVIEW_URL_CHANGED).
    if (this.folderPath) void pageSource.navigate(this.currentPath, isReload);
    else pageSource.unavailable(this.currentPath, isReload);

    // Notify the rename 404-retry observer (PreviewManager). Reported for every
    // REAL bridge-echoed navigation, including echoes — the consumer filters by
    // pathname against its tracked rename target, so an unrelated nav is a
    // no-op there. Skipped on the `synthesized` fallback path: that branch only
    // knows iframe.src (no notFound info), so reporting a false `notFound:false`
    // there could pre-empt and kill an in-flight retry. The try/catch keeps a
    // misbehaving future listener from corrupting navigation bookkeeping below.
    if (!opts.synthesized) {
      try {
        this.navigationResultListener?.({ url: currentUrl, notFound: !!data.notFound });
      } catch (e) {
        warn(`[NavigationManager] navigation-result listener threw: ${e}`);
      }
    }

    // Post-navigation scroll re-assertion for editor-driven navigations.
    // When the editor triggered this nav (echo), the new page has no scroll
    // context yet. Re-push the cached editor position so the preview starts
    // at the correct line for the new document.
    //
    // Only fire when this is a NEW-URL echo. Same-URL reload is already
    // handled above in the `currentUrl === previousUrl` branch. Firing here
    // too would double-scroll if the editor navigates to the currently-
    // displayed URL.
    //
    // The intent flip to 'editor' is unconditional: if a prior hash-nav had
    // set intent to 'user', an editor-driven page nav always re-establishes
    // editor ownership — even if lastEditorPosition is null (fresh editor).
    if (isEcho && currentUrl !== previousUrl) {
      this.scrollIntent = 'editor';
      if (this.lastEditorPosition) {
        debug(`[NavigationManager] Post-nav re-assert: line ${this.lastEditorPosition.line} (atTop=${this.lastEditorPosition.atTop})`);
        this.scrollToSourceLine(this.lastEditorPosition.line, this.lastEditorPosition.anchorId, this.lastEditorPosition.atTop, this.lastEditorPosition.atBottom);
      }
    } else if (!isEcho && currentUrl !== previousUrl && !isHashNavOnSamePage) {
      // User navigated to a genuinely new page (non-hash link) — editor owns it.
      // Clear the cached position: it describes a line/anchor/scroll-edge on
      // the PREVIOUS page's content. Leaving it set would let a later same-URL
      // reload of the new page (e.g. a rebuild echo) resurrect a stale
      // atBottom/line from the page the user left, scrolling the new page to
      // the wrong spot before the editor ever reports a position on it.
      this.scrollIntent = 'editor';
      this.lastEditorPosition = null;
    }
  }

  /**
   * Update back/forward button disabled states based on navigation capability
   */
  private updateButtonStates(canGoBack: boolean, canGoForward: boolean): void {
    if (this.backButton) {
      this.backButton.disabled = !canGoBack;
    }
    if (this.forwardButton) {
      this.forwardButton.disabled = !canGoForward;
    }
  }

  async goBack(): Promise<void> {
    if (!this.history.canGoBack()) return;
    if (!this.rpc) {
      error("❌ [NavigationManager] No RPC available for back navigation");
      return;
    }

    this.beginNavigation(this.history._stack[this.history._index - 1]);
    const request = this.pageRequest.current();
    try {
      this.pendingNavType = "back";
      await this.rpc.historyBack();
      if (this.pageRequest.isStale(request)) return;
      this.armTraversalDeadline("back");
    } catch (err) {
      if (this.pageRequest.isStale(request)) return;
      this.pendingNavType = null;
      warn(`⚠️ [NavigationManager] RPC failed, using direct navigation: ${err}`);

      // Fallback: update history and navigate iframe directly
      // This handles cases where iframe-bridge isn't available (e.g., RSS feed XML)
      this.history._index--;
      const prevUrl = this.history.getCurrentUrl();
      if (prevUrl && this.iframe) {
        // Stamp shell marker for chrome-clearance pre-paint (see shell-marker.ts).
        this.beginNavigation(prevUrl);
        this.iframe.src = withShellMarker(prevUrl);
      }
    }
  }

  async goForward(): Promise<void> {
    if (!this.history.canGoForward()) return;
    if (!this.rpc) {
      error("❌ [NavigationManager] No RPC available for forward navigation");
      return;
    }

    this.beginNavigation(this.history._stack[this.history._index + 1]);
    const request = this.pageRequest.current();
    try {
      this.pendingNavType = "forward";
      await this.rpc.historyForward();
      if (this.pageRequest.isStale(request)) return;
      this.armTraversalDeadline("forward");
    } catch (err) {
      if (this.pageRequest.isStale(request)) return;
      this.pendingNavType = null;
      warn(`⚠️ [NavigationManager] RPC failed, using direct navigation: ${err}`);

      // Fallback: update history and navigate iframe directly
      this.history._index++;
      const nextUrl = this.history.getCurrentUrl();
      if (nextUrl && this.iframe) {
        // Stamp shell marker for chrome-clearance pre-paint (see shell-marker.ts).
        this.beginNavigation(nextUrl);
        this.iframe.src = withShellMarker(nextUrl);
      }
    }
  }

  private armTraversalDeadline(dir: "back" | "forward"): void {
    this.armNavDeadline(NavigationManager.TRAVERSAL_WATCHDOG_MS, () => {
      // The echo consumed pendingNavType if the traversal happened.
      if (this.pendingNavType !== dir) return;
      this.pendingNavType = null;
      warn(
        `⚠️ [NavigationManager] ${dir} traversal never echoed — dropping phantom ${dir} entries`
      );
      if (dir === "forward") {
        this.history._stack.splice(this.history._index + 1);
      } else {
        this.history._stack.splice(0, this.history._index);
        this.history._index = 0;
      }
      this.updateButtonStates(this.history.canGoBack(), this.history.canGoForward());
    });
  }

  private sameRoute(a: string, b: string): boolean {
    const route = (url: string) => new URL(url).pathname.replace(/(?:\/index\.html)?\/?$/, '');
    return route(a) === route(b);
  }

  /**
   * Toggle between showing title and path
   */
  private toggleDisplayMode(): void {
    this.displayMode = this.displayMode === "path" ? "title" : "path";
    this.updateDisplay();
  }

  /**
   * Update the display based on current mode
   */
  private updateDisplay(): void {
    if (!this.urlText) return;

    if (this.displayMode === "title" && this.currentTitle) {
      this.urlText.textContent = this.currentTitle;
      this.urlText.dataset.mode = "title";
    } else {
      this.urlText.textContent = this.currentPath;
      this.urlText.dataset.mode = "path";
    }
  }

  /** A morph updates the title without changing navigation, history or scroll. */
  applyMorphedTitle(title: string | undefined): void {
    if (!title || title === this.currentTitle) return;
    this.currentTitle = title;
    this.updateDisplay();
  }

  // ===== URL Autocomplete =====

  /**
   * Setup the container for URL autocomplete dropdown
   */
  private setupAutocompleteContainer(): void {
    const urlContainer = this.urlText?.parentElement;
    if (!urlContainer) return;

    this.autocompleteContainer = document.createElement("div");
    this.autocompleteContainer.className = "moss-url-autocomplete-container";
    this.autocompleteContainer.style.display = "none";

    if (this.urlText?.nextSibling) {
      urlContainer.insertBefore(this.autocompleteContainer, this.urlText.nextSibling);
    } else {
      urlContainer.appendChild(this.autocompleteContainer);
    }
  }

  /**
   * Enter autocomplete mode - show the dropdown with page suggestions
   */
  private enterAutocompleteMode(): void {
    if (!this.urlText || !this.autocompleteContainer) return;
    if (this.isAutocompleteOpen) return;

    this.isAutocompleteOpen = true;
    this.urlText.style.display = "none";
    this.autocompleteContainer.style.display = "block";

    // Add editing class to nav pill for Safari-style focus ring
    const navPill = this.urlText.closest(".moss-nav-pill");
    if (navPill) navPill.classList.add("editing");

    this.autocomplete = new UrlAutocomplete(this.autocompleteContainer, {
      initialValue: this.currentPath,
      onSelect: (page: PageInfo) => this.handlePageSelect(page),
      onCancel: () => this.exitAutocompleteMode(),
    });
  }

  /**
   * Exit autocomplete mode - restore the URL text display
   */
  private exitAutocompleteMode(): void {
    if (!this.isAutocompleteOpen) return;

    this.isAutocompleteOpen = false;
    this.autocomplete?.destroy();
    this.autocomplete = null;

    if (this.autocompleteContainer) {
      this.autocompleteContainer.style.display = "none";
    }

    if (this.urlText) {
      this.urlText.style.display = "";
      // Remove editing class from nav pill
      const navPill = this.urlText.closest(".moss-nav-pill");
      if (navPill) navPill.classList.remove("editing");
    }
  }

  /**
   * Handle page selection from autocomplete
   */
  private handlePageSelect(page: PageInfo): void {
    this.exitAutocompleteMode();
    this.navigateToPage(page.url_path);
  }

  /**
   * Navigate iframe to the specified page path
   */
  navigateToPage(urlPath: string): void {
    if (!this.iframe) return;
    const url = this.buildNavigationUrl(urlPath);
    if (!url) return;
    // Stamp shell marker for chrome-clearance pre-paint (see shell-marker.ts).
    // The no-op comparison below uses the MARKED url so we compare like-for-
    // like with what we previously assigned.
    const markedUrl = withShellMarker(url);
    // A no-op when the preview is already on (or heading to) this page. That
    // is not `iframe.src`, which link clicks inside the frame never update:
    // comparing with it reloaded a page reached by a link, and ignored a
    // request to go back to a page the user had followed a link away from.
    // A #fragment always navigates, so the jump still happens. On a no-op,
    // clear any pendingEcho that was optimistically recorded — nothing will
    // bounce back, and leaving the token live would misclassify unrelated
    // in-frame link clicks to the same URL as echoes.
    const onOrHeadingTo = this.requestedUrl ?? this.activePage?.url ?? stripShellMarker(this.iframe.src);
    if (!new URL(url).hash && this.sameRoute(onOrHeadingTo, url)) {
      this.pendingEcho = null;
      // The preview is already on the page whose footer we were asked to
      // reveal, so do it now — no echo will arrive to do it for us.
      if (this.pendingFooterReveal && sameUrlPathname(url, this.pendingFooterReveal.url)) {
        this.pendingFooterReveal = null;
        void this.revealFooter();
      }
      return;
    }
    this.beginNavigation(url);
    this.iframe.src = markedUrl;
  }

  // ===== Device Mode (Desktop/Mobile Preview) =====

  /**
   * Toggle between desktop and mobile preview mode
   */
  togglePreviewMode(): void {
    this.isMobileMode = !this.isMobileMode;
    this.updatePreviewMode();
    this.saveModePreference();
  }

  private updatePreviewMode(): void {
    if (!this.previewContainer) return;

    if (this.isMobileMode) {
      this.previewContainer.classList.add("mobile-mode");
    } else {
      this.previewContainer.classList.remove("mobile-mode");
    }

    this.updateToggleButton();
    // Most mobile-mode geometry is shell-side (frame position, fake-scrollbar
    // track top). The ONE iframe-content concern is the titlebar-clearance
    // padding (`.moss-shell-frame body`): in mobile the frame sits below the
    // titlebar, so that padding must be dropped inside the iframe. Signal the
    // mode so the bridge toggles `.moss-mobile-frame` and site.css gates it.
    this.pushDeviceModeToIframe(this.isMobileMode);
    // Notify PreviewManager so it can re-apply zoom with the correct transform
    // branch (desktop: scale+width-compensation+origin:0 0; mobile: translate+scale).
    this.deviceModeChangeListener?.(this.isMobileMode);
  }

  private updateToggleButton(): void {
    if (!this.toggleButton) return;
    // ONE state carrier: html[data-preview-mode], the same attribute the
    // index.html pre-paint script writes from localStorage. The device-glyph
    // CSS block (style.css) maps it to the icons — no inline styles that
    // could go stale against the attribute. The glyph shown is the mode you
    // would SWITCH to (call to action), which the CSS mapping expresses.
    if (this.isMobileMode) document.documentElement.setAttribute("data-preview-mode", "mobile");
    else document.documentElement.removeAttribute("data-preview-mode");
    this.toggleButton.classList.toggle("mobile-mode", this.isMobileMode);
    this.toggleButton.setAttribute(
      "data-tooltip",
      t(this.isMobileMode ? 'preview.device_desktop' : 'preview.device_mobile')
    );
  }

  private saveModePreference(): void {
    localStorage.setItem(
      "moss-preview-mode",
      this.isMobileMode ? "mobile" : "desktop"
    );
  }

  private loadModePreference(): void {
    const savedMode = localStorage.getItem("moss-preview-mode");
    this.isMobileMode = savedMode === "mobile";
    this.updatePreviewMode();
  }

  /**
   * Get current preview mode
   */
  isMobilePreview(): boolean {
    return this.isMobileMode;
  }

  /** In-place reload preserves scroll; false lets the request choose its fallback. */
  async reload(): Promise<boolean> {
    if (!this.rpc) return false;
    // Handled: the pending navigation loads fresh content.
    if (this.requestedUrl) return true;
    try {
      await this.rpc.call("location.reload");
      return true;
    } catch {
      return false;
    }
  }

  /**
   * Morph the current document in place, preserving scroll, focus and undo.
   * A matching bridge acknowledgement resolves true; failure or timeout lets
   * the current request fall back to reload. Each morph has its own id.
   */
  async morph(): Promise<boolean> {
    const target = this.iframe?.contentWindow;
    if (!target) return false;
    if (this.requestedUrl) return false;
    const request = this.pageRequest.current();
    const morphId = ++this.morphSequence;
    return new Promise<boolean>((resolve) => {
      let settled = false;
      let timer = 0;
      const done = (ok: boolean) => {
        if (settled) return;
        settled = true;
        window.removeEventListener("message", onMessage);
        clearTimeout(timer);
        resolve(ok);
      };
      const onMessage = (e: MessageEvent) => {
        const t = e.data?.type;
        if (t !== "moss-morph-applied" && t !== "moss-morph-failed") return;
        if (e.source !== target) return;
        if (e.data?.gen !== morphId) return;
        if (this.pageRequest.isStale(request)) { done(false); return; }
        if (t === "moss-morph-applied") {
          this.applyMorphedTitle(e.data?.title);
          done(true);
        } else if (t === "moss-morph-failed") {
          // `changedScripts` names the scripts behind a `script-changed` verdict.
          const why = (e.data?.changedScripts as string[] | undefined)?.join(", ");
          warn(
            `[NavigationManager] morph failed: ${e.data?.error ?? "unknown"}${why ? ` [${why}]` : ""} → reload fallback`
          );
          done(false);
        }
      };
      window.addEventListener("message", onMessage);
      // Generous budget: bridge fetch + parse + morph on a large page.
      timer = window.setTimeout(() => {
        warn("[NavigationManager] morph timed out → reload fallback");
        done(false);
      }, 4000);
      target.postMessage({ type: "moss-morph", gen: morphId }, "*");
    });
  }

  /** One call into the previewed page; rejects when it has no bridge. */
  callBridge(method: string, ...args: unknown[]): Promise<unknown> { return this.rpc?.call(method, ...args) ?? Promise.reject(new Error('no preview bridge')); }

  /**
   * Scroll the preview iframe to the element nearest the given source line.
   * Used for editor-to-preview scroll sync.
   *
   * Invariant: any external call site that invokes this on behalf of the editor
   * must first call claimScrollForEditor() (or set scrollIntent = 'editor'
   * inline) so the intent model stays consistent. Internal calls inside
   * handleNavigationUpdate write scrollIntent inline as part of the transition.
   */
  /**
   * Where the preview is right now — the reading a scroll would report. Null
   * when the page has no bridge (non-HTML content) or one too old to answer.
   */
  async readScrollPosition(): Promise<{ line: number; atTop: boolean; atBottom: boolean } | null> {
    if (!this.rpc) return null;
    try {
      return await this.rpc.call<{ line: number; atTop: boolean; atBottom: boolean }>('scrollPosition');
    } catch {
      return null;
    }
  }

  async scrollToSourceLine(line: number, fallbackAnchorId: string | null, atTop = false, atBottom = false, onlyIfOffscreen = false): Promise<void> {
    if (!this.rpc) return;
    try {
      // atTop / atBottom flow to the bridge as args[1] / args[2]. When atTop is
      // set the bridge scrollTo({top:0}); when atBottom is set it scrolls to
      // its max. The line arg is informational in both cases.
      //
      // args[3] onlyIfOffscreen marks a CARET-driven request (the caret moved,
      // rather than the editor being scrolled). The bridge drops it when the
      // target is already comfortably in view — it is the only party that knows
      // that. Without it, following the caret would twitch the preview on every
      // keystroke.
      const matched = await this.rpc.call<number>('scrollToSourceLine', line, atTop, atBottom, onlyIfOffscreen);
      // Anchor fallback only when (a) the bridge truly found no element AND
      // (b) we are not at an edge (an edge short-circuit returns without a
      // line match but is authoritative — don't override it with the anchor).
      if (matched === 0 && fallbackAnchorId && !atTop && !atBottom) {
        await this.rpc.call('element.scrollIntoView', `#${fallbackAnchorId}`);
      }
    } catch {
      // iframe may not have the bridge (non-HTML content)
    }
  }

  /** Reveal the footer once `urlPath` has loaded. Call immediately before
   *  `navigateToPage(urlPath)` when the reveal target is a page the preview is
   *  not showing yet. No-op if the URL cannot be resolved (no iframe yet),
   *  which is also the case where nothing would navigate. See
   *  {@link revealFooter}. */
  armFooterReveal(urlPath: string): void {
    const url = this.buildNavigationUrl(urlPath);
    if (!url) return;
    this.pendingFooterReveal = { url, expiresAt: Date.now() + NavigationManager.ECHO_TTL_MS };
  }

  /**
   * Point at the previewed page's `<footer>`.
   *
   * A slot file (`footer.md`) becomes no page of its own, so the editor cannot
   * follow it the way it follows an article — opening it would otherwise leave
   * the preview looking untouched. This points at the thing the user is
   * actually editing.
   *
   * The selector keys off `data-source-none`, which the preview build already
   * stamps on the single `<footer>` (`crates/moss-build/src/build/page/shell.rs`). Deliberately
   * NOT a line anchor: footer slot HTML has its `data-source-*` annotations
   * stripped at collection time (`build/footer.rs`, regression-tested there),
   * and re-adding them to reach the footer would undo that.
   */
  revealFooter(): Promise<void> {
    return this.revealSource(NavigationManager.FOOTER_SELECTOR);
  }

  /**
   * Point at one thing on the previewed page: scroll it into view and run the
   * one-shot wash over it. The whole of "show me where that is."
   *
   * ONE primitive, so every answer looks the same wherever the question came
   * from — opening `footer.md` washes the footer, opening a `byline` chip
   * washes the byline. `selector` is matched inside the iframe by the bridge;
   * a selector that matches nothing rejects and is a no-op, which is what
   * makes it safe to ask for a field the current page does not render.
   *
   * The wash is a preview-only `@keyframes` served with the page
   * (`crates/moss-build/src/ops/serve/iframe_bridge.rs`), so this writes one `animation` property and
   * clears it once the run is over. Clearing matters: leaving the property set
   * would make the next idiomorph morph of this element replay the animation.
   *
   * The keyframes survive a morph (they are re-served with every response);
   * this inline `animation` property does not, so a rebuild landing inside the
   * wash cuts it short. Accepted: the wash is a pointer, and the rebuild that
   * truncates it is itself the user seeing their edit land.
   */
  async revealSource(selector: string): Promise<void> {
    if (!this.rpc) return;
    const rpc = this.rpc;
    const sel = selector;
    const token = ++this.revealToken;
    try {
      await rpc.call('element.scrollIntoView', sel);
      await rpc.call('element.setStyle', sel, 'animation', `moss-slot-reveal ${NavigationManager.REVEAL_MS}ms ease-out`);
    } catch {
      // No bridge (non-HTML content), or nothing on this page matches the
      // selector. Either way there is nothing to reveal and nothing to clean up.
      return;
    }
    setTimeout(() => {
      // Only the most recent reveal may clear the property. Two reveals inside
      // one wash (open footer.md, click away, come back) would otherwise have
      // the first one's timer cut the second one's animation short.
      if (token !== this.revealToken) return;
      // Best-effort cleanup; the iframe may have navigated away by now.
      void rpc.call('element.setStyle', sel, 'animation', '').catch(() => {});
    }, NavigationManager.REVEAL_MS + 200);
  }

  /**
   * The hover strength of {@link revealSource}: run the same wash over every
   * element `selector` matches, WITHOUT scrolling. A hover is a glance, not a
   * command — the flashed element may be off-viewport and that is fine;
   * scrolling the preview on mere hover would be scroll-jacking. Class-based
   * (`.moss-fm-flash`, served with the page next to the `moss-slot-reveal`
   * keyframes) so hover-out can cleanly undo it and so reduced-motion can hold
   * a static tint instead of an animation. No bridge / no match → silent no-op.
   */
  async flashSource(selector: string): Promise<void> {
    if (!this.rpc) return;
    try {
      await this.rpc.call('element.classAll', selector, NavigationManager.FM_FLASH_CLASS, true);
    } catch {
      // No bridge (non-HTML content) — nothing to flash.
    }
  }

  /** Hover-out half of {@link flashSource}. Always safe to call. */
  async clearFlashSource(selector: string): Promise<void> {
    if (!this.rpc) return;
    try {
      await this.rpc.call('element.classAll', selector, NavigationManager.FM_FLASH_CLASS, false);
    } catch {
      // No bridge — nothing to clear.
    }
  }

  /** Intent owns the issue-to-echo gap; the loaded page owns the settled URL. */
  getCurrentUrl(): string | null {
    return this.requestedUrl ?? this.activePage?.url ?? (this.history.getCurrentUrl() || null);
  }

  /**
   * Register a callback invoked after every device-mode change (desktop ↔ mobile).
   * PreviewManager uses this to re-apply zoom when the user toggles the device-toggle
   * pill, keeping the correct transform branch in sync. Pass null to unregister.
   */
  setDeviceModeChangeListener(cb: ((isMobile: boolean) => void) | null): void {
    this.deviceModeChangeListener = cb;
  }

  /**
   * Set the folder path for article detection.
   * Called by PreviewManager when the project path changes.
   */
  setFolderPath(path: string): void {
    if (path !== this.folderPath) pageSource.invalidate();
    this.folderPath = path;
  }

  /**
   * Reclaim scroll ownership for the editor. Call this before any
   * scrollToSourceLine invocation that originates from the editor — it
   * transfers intent from 'user' (hash-nav hold) back to 'editor'.
   */
  claimScrollForEditor(): void {
    this.scrollIntent = 'editor';
  }

  /**
   * Cache the editor's scroll position for post-rebuild restoration.
   * Called from app.ts on every editor-to-preview scroll sync event.
   */
  setLastEditorPosition(line: number, anchorId: string | null, atTop = false, atBottom = false): void {
    this.lastEditorPosition = { line, anchorId, atTop, atBottom };
  }
}
