/**
 * iframe-bridge.ts - Cross-origin RPC bridge for preview iframe
 *
 * This script is built to JavaScript and injected into HTML responses
 * by the preview server. It provides:
 * 1. Dynamic RPC handler - calls any window property/method via postMessage
 * 2. URL change reporting - notifies parent when location changes
 * 3. External link interception - workaround for Tauri issue #9912
 *
 * Note: Navigation history tracking is handled in the parent window
 * (NavigationManager) because JavaScript globals don't persist across
 * page navigations within the iframe.
 */

import { Idiomorph } from "idiomorph";
import { pickThemeFromMessage, startPageColor } from "./iframe-bridge-theme";
import { describeScriptChange, syncBodyAttributes, captureServerBody } from "./morph-guard";
import { createAssetSwapBuffer } from "./asset-swap-buffer";
import { bustSrcset, normPath, srcsetMatches, stripBust } from "./asset-urls";
import { FOCUS_FRACTION, getSourceLine, interpolatedScrollTop, isAlreadyShowing, readScrollPosition } from "./scroll-interpolate";
import { installBlueprintFallback } from "./blueprint-fallback";
import { installSwipeNavigation } from "./swipe-gesture";
import { startChromeAmbient } from "./chrome-ambient";
import { startSiteAccent } from "./site-accent";
import type { ThemeSettled } from "./theme-settled";
import { classifyClickGesture } from "./click-vocabulary";
import { installContextMenu } from "./context-menu";

(function () {
  "use strict";

  // G3 probe (morph keystone): the bridge IIFE must run EXACTLY ONCE per loaded
  // document. A morph must NOT re-execute this <script> — idiomorph reuses the
  // node, so the IIFE's closures/listeners persist (that is what makes the
  // in-bridge morph viable). window.__bridgeInitCount must stay 1 across morphs.
  // It is a DIAGNOSTIC, not a gate — it rides in the morph ack, and nothing under
  // tests/ asserts on it. The keystone's coverage is the jsdom harness in
  // __tests__/iframe-bridge-morph.test.ts, which embeds the shipped bundle.
  (window as unknown as { __bridgeInitCount?: number }).__bridgeInitCount =
    ((window as unknown as { __bridgeInitCount?: number }).__bridgeInitCount || 0) + 1;

  // Record <body>'s server-sent attributes NOW, while the document is still
  // exactly the bytes the server sent. A morph reconciles <body>'s attributes
  // against a freshly-fetched document, and telling a stale server attribute
  // (remove it) from a runtime one (never touch it) needs this baseline. Site
  // scripts run after this point and would poison it.
  captureServerBody();

  // ============================================================
  // Chrome clearance: `moss-shell-frame` class on <html>
  // ============================================================
  // INERT now: the shell insets the preview iframe, so the served
  // document needs no clearance and site.css defines no rule for this class.
  // The reconciliation below still runs, kept as the revert path in case
  // this insetting is ever undone. It formerly gated
  // `body{padding-top:48px}`, stamped server-side before first
  // paint off the `?__moss_shell=1` marker; this runtime topology check is the
  // defense-in-depth half, recovering the class when user-page JS navigates and
  // drops the marker, and REMOVING it when the marker leaks into a nested
  // iframe (chrome geometry must never reach one — the load-bearing
  // nested-iframe invariant). On the
  // real web the bridge is never injected at all, so nothing here applies.
  const isShellMounted =
    window !== window.top && window.parent === window.top;
  const hasFrameClass =
    document.documentElement.classList.contains("moss-shell-frame");
  if (isShellMounted && !hasFrameClass) {
    // Case 2a: marker-loss recovery. Server didn't add the class, but
    // we are in fact shell-mounted. Add it now. (The browser may have
    // already painted one frame without the padding — visible jump —
    // but only on this rare path.)
    document.documentElement.classList.add("moss-shell-frame");
  } else if (!isShellMounted && hasFrameClass) {
    // Case 2b: nested-iframe defense. Server thought this was shell-
    // mounted (marker presumably leaked) but topology says otherwise.
    // Remove the class so chrome geometry doesn't leak into the
    // nested iframe.
    document.documentElement.classList.remove("moss-shell-frame");
  }

  // ============================================================
  // Theme bridge (shell-mounted preview only). The moon toggle in the
  // site (theme.ts) is the single theme control inside moss: relay its
  // choice UP to the shell, and apply the shell's choice when pushed DOWN.
  // Never runs for real visitors — the bridge isn't injected without
  // ?__moss_shell=1, and the listeners below are gated on isShellMounted.
  // ============================================================
  let currentShellTheme: "light" | "dark" | null = null;

  // Device-preview mode (mobile phone-frame vs desktop), pushed DOWN by the
  // shell. INERT now — no document-side clearance left to drop, and
  // site.css defines no rule for either class. Kept as the revert path in
  // case this insetting is ever undone; it formerly undid the
  // clearance, which showed as a gap inside the phone
  // frame because that frame already sits BELOW the titlebar.
  // The iframe CANNOT infer this from its own width: a desktop preview dragged
  // to MIN_PANEL_WIDTH (390px) is byte-identical in width to the phone frame,
  // so the shell must signal the mode explicitly.
  let currentDeviceMobile = false;

  // The page-colour and ambient reporters (shell-mounted only). The morph path
  // re-fires them: a morph repaints without touching `data-theme`.
  let pageColor: ThemeSettled | null = null;
  let chromeAmbient: ThemeSettled | null = null;

  function applyDeviceMode(mobile: boolean): void {
    document.documentElement.classList.toggle("moss-mobile-frame", mobile);
  }

  function applyShellTheme(theme: "light" | "dark"): void {
    // Set the attribute WITHOUT dispatching moss-theme-change (no echo loop);
    // a same-value write would still fire the reporters' observer, so skip it.
    const root = document.documentElement;
    if (root.getAttribute("data-theme") !== theme) root.setAttribute("data-theme", theme);
    // The site's own key, so the next document's pre-paint (shell.html) shows
    // what the shell last pushed instead of what the site last toggled. The
    // preview origin is loopback-only; this never reaches a visitor's browser.
    try { localStorage.setItem("moss-theme", theme); } catch { /* private mode */ }
    // No reporter re-fire here: the attribute write above is their trigger.
  }

  if (isShellMounted) {
    // Up: the site flipped its theme (moon) → tell the shell. theme.ts sets
    // data-theme before dispatching, so detail.theme is the new value.
    document.documentElement.addEventListener("moss-theme-change", (e: Event) => {
      const theme = (e as CustomEvent<{ theme?: string }>).detail?.theme;
      if (theme === "light" || theme === "dark") {
        window.parent.postMessage({ type: "moss-theme-change", theme }, "*");
      }
    });

    // Down: the shell pushed a theme (every iframe load, every shell flip).
    window.addEventListener("message", (e: MessageEvent) => {
      const theme = pickThemeFromMessage(e.data);
      if (theme) {
        currentShellTheme = theme;
        applyShellTheme(theme);
      }
    });

    // Cold start: this script sits before </body> and the shell's next push
    // waits for window `load`; ask now so a disagreeing page flips at parse end.
    window.parent.postMessage({ type: "moss-theme-request" }, "*");

    // Down: the shell pushes device mode on load and whenever the user toggles
    // the device-preview button. The bridge listener is registered during parse
    // (before the shell's iframe `load` push fires), so no handshake is needed.
    window.addEventListener("message", (e: MessageEvent) => {
      const data = e.data;
      if (data && typeof data === "object" && data.type === "moss-set-device-mode") {
        currentDeviceMobile = data.mobile === true;
        applyDeviceMode(currentDeviceMobile);
      }
    });
  }

  // Scroll sync lock — shared between the RPC handler (which sets it after
  // programmatic scrolls) and the scroll observer (which reads it to avoid echo).
  let _scrollSyncLock = false;

  // ============================================================
  // Helper Functions
  // ============================================================

  function serializeElement(el: Element): object {
    const rect = el.getBoundingClientRect();
    return {
      tagName: el.tagName.toLowerCase(),
      id: el.id || null,
      className: el.className || null,
      textContent: el.textContent?.substring(0, 500) || null,
      innerHTML: el.innerHTML.substring(0, 1000),
      href: (el as HTMLAnchorElement).href || null,
      src: (el as HTMLImageElement).src || null,
      value: (el as HTMLInputElement).value || null,
      boundingRect: {
        top: rect.top,
        left: rect.left,
        width: rect.width,
        height: rect.height,
        bottom: rect.bottom,
        right: rect.right
      },
      attributes: Object.fromEntries(
        Array.from(el.attributes).map(attr => [attr.name, attr.value])
      ),
      childElementCount: el.childElementCount
    };
  }

  // ============================================================
  // RPC Handler (explicit method dispatch)
  // ============================================================
  // Handles RPC calls from the parent window for navigation control.
  // Uses explicit method dispatch to prevent arbitrary code execution.

  window.addEventListener("message", (e: MessageEvent) => {
    if (e.data?.type !== "moss-rpc-call") return;

    const { id, method, args = [] } = e.data;
    let result: unknown;
    let error: string | undefined;

    try {
      // Explicit dispatch table - no dynamic property access
      // This prevents arbitrary code execution via user-controlled method strings
      switch (method) {
        case "history.back":
          history.back();
          result = undefined;
          break;
        case "history.forward":
          history.forward();
          result = undefined;
          break;
        case "history.go":
          history.go(args[0] as number);
          result = undefined;
          break;
        case "location.href":
          result = location.href;
          break;
        case "location.reload":
          location.reload();
          result = undefined;
          break;

        // DOM Query Methods
        case "document.title":
          result = document.title;
          break;

        case "document.querySelector":
          const qsEl = document.querySelector(args[0] as string);
          result = qsEl ? serializeElement(qsEl) : null;
          break;

        case "document.querySelectorAll":
          const qsaElements = document.querySelectorAll(args[0] as string);
          result = Array.from(qsaElements).map(serializeElement);
          break;

        case "document.body.outerHTML":
          result = document.body.outerHTML.substring(0, 100000); // Cap at 100KB
          break;

        // DOM Modification Methods
        case "element.click":
          const clickEl = document.querySelector(args[0] as string) as HTMLElement;
          if (!clickEl) throw new Error(`Element not found: ${args[0]}`);
          clickEl.click();
          result = true;
          break;

        case "element.classAll": {
          // Toggle a class on EVERY match (element.setStyle touches only the
          // first). Used by the chip-hover flash: a frontmatter field can
          // render in several places (data-source-fm on title, byline, and a
          // colophon row), and pointing at one of them would misanswer the
          // question. Zero matches is a fine answer — the field simply isn't
          // rendered on this page — so result is the count, never an error.
          const [classSelector, className, classOn] = args as [string, string, boolean];
          const classEls = document.querySelectorAll(classSelector);
          classEls.forEach((el) => el.classList.toggle(className, classOn === true));
          result = classEls.length;
          break;
        }

        case "element.setStyle":
          const [styleSelector, styleProp, styleValue] = args as [string, string, string];
          const styleEl = document.querySelector(styleSelector) as HTMLElement;
          if (!styleEl) throw new Error(`Element not found: ${styleSelector}`);
          styleEl.style.setProperty(styleProp, styleValue);
          result = true;
          break;

        case "input.setValue":
          const [inputSelector, inputValue] = args as [string, string];
          const inputEl = document.querySelector(inputSelector) as HTMLInputElement;
          if (!inputEl) throw new Error(`Input not found: ${inputSelector}`);
          inputEl.value = inputValue;
          inputEl.dispatchEvent(new Event('input', { bubbles: true }));
          inputEl.dispatchEvent(new Event('change', { bubbles: true }));
          result = true;
          break;

        case "window.scrollTo":
          window.scrollTo(args[0] as number, args[1] as number);
          result = true;
          break;

        case "element.scrollIntoView":
          const scrollEl = document.querySelector(args[0] as string);
          if (!scrollEl) throw new Error(`Element not found: ${args[0]}`);
          scrollEl.scrollIntoView({ behavior: 'smooth', block: 'center' });
          result = true;
          break;

        // Where the preview is right now, for an editor that has just opened
        // and must follow it. The same reading a scroll reports, asked for
        // instead of waited for: an unscrolled page never reports at all.
        // Unlike the report it ignores _scrollSyncLock — that lock hides the
        // echo of a scroll the editor caused, and a question is not an echo.
        case "scrollPosition":
          result = readScrollPosition(window);
          break;

        case "scrollToSourceLine": {
          const targetLine = args[0] as number;
          // Strict === true (not truthy). Coercing 1/"true"/{} to "go to
          // top" would silently honor buggy callers; ignoring them keeps
          // the RPC honest to its typed wire contract.
          const atTop = args[1] === true;
          const atBottom = args[2] === true;
          // Caret-driven request: the caret moved, rather than the editor
          // being scrolled. Honour it only when the target is NOT already on
          // screen — see the check further down, and QUIET_BAND.
          const onlyIfOffscreen = args[3] === true;

          // Named signal: caller said "go to literal top." This is now
          // the ONLY path to scrollTop=0 — the previous PR's
          // `targetLine <= 1` backstop is removed because every sender
          // sets atTop explicitly in this binary. atTop is checked FIRST so a
          // document too short to scroll (both flags true) resolves to top.
          if (atTop) {
            window.scrollTo({ top: 0, behavior: 'smooth' });
            _scrollSyncLock = true;
            setTimeout(() => { _scrollSyncLock = false; }, 400);
            result = 0;
            break;
          }

          // Symmetric signal: caller said "editor is at its bottom." Park the
          // preview at its own max scroll, so reaching the end of the file in
          // the editor reaches the end of the page in the preview (a line-based
          // sync would stop short — the top-visible line is before the last).
          if (atBottom) {
            const maxTop = document.documentElement.scrollHeight - window.innerHeight;
            window.scrollTo({ top: Math.max(0, maxTop), behavior: 'smooth' });
            _scrollSyncLock = true;
            setTimeout(() => { _scrollSyncLock = false; }, 400);
            result = -1; // sentinel: edge short-circuit, no line matched
            break;
          }

          // Search both data-source-line and data-source-range elements
          const els = document.querySelectorAll('[data-source-line], [data-source-range]');
          let best: Element | null = null;
          let bestLine = 0;
          let next: Element | null = null;
          let nextLine = Infinity;
          for (const el of els) {
            const l = getSourceLine(el);
            if (l <= targetLine && l > bestLine) {
              bestLine = l;
              best = el;
            }
            if (l > targetLine && l < nextLine) {
              nextLine = l;
              next = el;
            }
          }
          if (best) {
            // ── "Do not move if you do not have to" ────────────────────────
            //
            // A caret-driven request is fired on every keystroke, so centring
            // the line each time would twitch the page continuously while the
            // author typed — worse than a preview that does not follow at all.
            // The editor cannot make this call: it has no idea what is visible
            // over here. So it asks, and this decides.
            //
            // The band itself, and why it is as wide as it is, live with the
            // rest of this sync's geometry in scroll-interpolate.ts.
            if (onlyIfOffscreen
                && isAlreadyShowing(best.getBoundingClientRect().top, window.innerHeight)) {
              result = bestLine;
              break;
            }

            // Align the target line at the FOCUS line — FOCUS_FRACTION of the
            // viewport down — so it lands at the same fraction the editor uses
            // (block:'center' for 0.5). Both branches use the helper, never
            // scrollIntoView({block:'center'}) — that centres inside the
            // scrollport MINUS root scroll-padding (island clearance, site.css).
            const focusOffset = window.innerHeight * FOCUS_FRACTION;
            const scrollToFocusLine = (t: number): void => window.scrollTo({ top: Math.max(0, t - focusOffset), behavior: 'smooth' });
            if (next && targetLine > bestLine && nextLine > bestLine) {
              // Proportional interpolation between two annotated elements. The
              // span is the distance to the NEXT anchor (nextRect.top -
              // bestRect.top), which correctly includes any UNannotated tall
              // block sitting in the gap (e.g. a folder-to-site.html iframe
              // embed) — using best.offsetHeight instead would undershoot and
              // never cross the embed, driving a top↔bottom scroll oscillation.
              const fraction = (targetLine - bestLine) / (nextLine - bestLine);
              const bestRect = best.getBoundingClientRect();
              const nextRect = (next as HTMLElement).getBoundingClientRect();
              scrollToFocusLine(interpolatedScrollTop(window.scrollY, bestRect.top, nextRect.top, fraction));
            } else {
              // Exact / no-next match: the element's own top.
              scrollToFocusLine(best.getBoundingClientRect().top + window.scrollY);
            }
            _scrollSyncLock = true;
            setTimeout(() => { _scrollSyncLock = false; }, 400);
          }
          result = bestLine;
          break;
        }

        case "find":
          // window.find(aString, caseSensitive, backwards, wrapAround)
          // Used by PreviewFindController to highlight matches in the rendered page.
          result = window.find(args[0] as string, args[1] as boolean, args[2] as boolean, args[3] as boolean);
          break;

        default:
          throw new Error(`Method not allowed: ${method}`);
      }
    } catch (err: unknown) {
      error = err instanceof Error ? err.message : String(err);
    }

    window.parent.postMessage(
      { type: "moss-rpc-result", id, result, error },
      "*"
    );
  });

  // ============================================================
  // URL Change Reporter
  // ============================================================

  // True when the current document is the preview server's synthetic 404 page,
  // detected via the `<meta name="moss-not-found">` marker the server stamps
  // (router.rs). The shell uses this to retry a rename-navigation that raced
  // ahead of the generation being published — see preview-actions.ts
  // `decideRenameRetry`. Matching the marker (not the "Not Found" title) avoids
  // false positives from a real page that happens to be titled "Not Found".
  function isNotFoundPage(): boolean {
    return !!document.querySelector('meta[name="moss-not-found"]');
  }

  // The document's REAL navigation type ("navigate" | "back_forward" |
  // "reload"). Sent ONLY with the document-load notification: for
  // popstate/hashchange re-notifications the performance entry still
  // describes the original load, not the same-document traversal, so those
  // omit it. The shell uses this to classify echoes by what the iframe
  // actually did — guessing from pendingNavType alone let the parent stack
  // drift from the iframe's session history (dead forward button).
  function documentNavType(): string | undefined {
    try {
      const entry = performance.getEntriesByType("navigation")[0] as
        | PerformanceNavigationTiming
        | undefined;
      return entry?.type;
    } catch {
      return undefined;
    }
  }

  function notifyNavigation(navType?: string): void {
    window.parent.postMessage(
      {
        type: "moss-navigation",
        url: location.href,
        title: document.title,
        notFound: isNotFoundPage(),
        navType,
      },
      "*"
    );
  }

  // ============================================================
  // WKWebView Repaint Workaround
  // ============================================================
  // WKWebView on macOS sometimes fails to repaint after location.reload()
  // inside an iframe, leaving the viewport blank until a user scroll.
  // Toggling translateZ(0) forces GPU layer promotion → compositor repaint.
  // Ref: https://bugs.webkit.org/show_bug.cgi?id=258375

  function forceRepaint(): void {
    requestAnimationFrame(() => {
      document.documentElement.style.transform = 'translateZ(0)';
      requestAnimationFrame(() => {
        document.documentElement.style.transform = '';
      });
    });
  }

  // Initial notification — the only one that carries the document's real
  // navigation type (see documentNavType).
  notifyNavigation(documentNavType());
  forceRepaint();

  // ============================================================
  // Missing-image placeholder upgrade (preview/editor ONLY)
  // ============================================================
  // The site shell swaps any broken <img> to a STATIC blueprint-grid SVG
  // (class .moss-img-fallback) — that is what real visitors of the published
  // site always see. This bridge is injected ONLY into moss's preview/editor
  // webviews (never into published output), so here — and only here — we
  // upgrade those static placeholders to the launcher's live, mouse-reactive
  // blueprint grid, reusing the <img>'s exact box. See ./blueprint-fallback.ts.
  installBlueprintFallback();

  // Handle popstate (back/forward navigation). Wrapped: the listener must
  // not leak the Event object into notifyNavigation's navType parameter.
  window.addEventListener("popstate", () => notifyNavigation());

  // Handle hashchange
  window.addEventListener("hashchange", () => notifyNavigation());

  // ============================================================
  // Link Click Interceptor
  // ============================================================
  // Handles two cases:
  // 1. External links (http/https or data-external) -> open in browser
  // 2. Internal links -> navigate via resolved URL

  document.addEventListener(
    "click",
    (event: MouseEvent) => {
      const anchor = (event.target as Element).closest("a");
      if (!anchor) return;

      const href = anchor.getAttribute("href");
      if (!href) return;

      // Skip special links (including potentially dangerous URL schemes)
      if (
        href.startsWith("#") ||
        href.startsWith("javascript:") ||
        href.startsWith("mailto:") ||
        href.startsWith("data:") ||
        href.startsWith("vbscript:")
      ) {
        return;
      }

      // Check if link should open externally:
      // 1. Absolute HTTP/HTTPS URLs
      // 2. Links with data-external attribute (e.g., RSS feeds)
      const isAbsoluteExternal =
        href.startsWith("http://") || href.startsWith("https://");
      const hasExternalAttr = anchor.hasAttribute("data-external");

      if (isAbsoluteExternal || hasExternalAttr) {
        event.preventDefault();
        event.stopPropagation();

        // Resolve relative URLs to absolute for data-external links
        const absoluteUrl = isAbsoluteExternal
          ? href
          : new URL(href, window.location.href).href;

        window.parent.postMessage(
          { type: "open-external-link", url: absoluteUrl },
          "*"
        );
        return;
      }

      // Internal link: navigate via URL resolution.
      //
      // No cache-busting needed — server sets Cache-Control: no-cache, no-store
      // and strips conditional request headers, so every request hits the server.
      //
      // Propagate the shell marker (`__moss_shell`) so the next document
      // also gets the `moss-shell-frame` class server-side and avoids the
      // first-paint flash. We only propagate when WE are shell-mounted
      // (isShellMounted from the topology check above) — never propagate
      // out of a nested iframe. This keeps nested cover/embed iframes
      // free of the marker even if the bridge were somehow loaded inside
      // them. See `iframe_bridge.rs::SHELL_MARKER` for the contract.
      event.preventDefault();
      const url = new URL(href, window.location.href);
      if (isShellMounted) {
        url.searchParams.set("__moss_shell", "1");
      }
      window.location.href = url.href;
    },
    true
  );

  // ============================================================
  // Source Target Resolution (unified click-to-source mapping)
  // ============================================================
  // Resolves a DOM element to its source target by walking up the DOM
  // checking for annotation attributes in priority order:
  //   1. data-source-fm="field"   → frontmatter field
  //   2. data-source-range="N-M"  → shortcode line range
  //   3. data-source-line="N"     → body markdown line
  //   4. data-source-none         → template chrome (unmappable)
  //   5. no annotation            → null (ignore)

  type SourceTarget =
    | { kind: 'frontmatter'; field: string }
    | { kind: 'body-range'; startLine: number; endLine: number }
    | { kind: 'body-line'; line: number }
    | { kind: 'none' };

  /** Extract the start source line from an element with data-source-line or data-source-range. */
  function resolveSourceTarget(el: Element | null): SourceTarget | null {
    if (!el) return null;

    const fm = el.closest('[data-source-fm]');
    if (fm) return { kind: 'frontmatter', field: fm.getAttribute('data-source-fm')! };

    const range = el.closest('[data-source-range]');
    if (range) {
      const parts = range.getAttribute('data-source-range')!.split('-');
      const start = Number(parts[0]);
      const end = Number(parts[1]);
      if (isNaN(start) || isNaN(end)) return null;
      return { kind: 'body-range', startLine: start, endLine: end };
    }

    const line = el.closest('[data-source-line]');
    if (line) {
      const n = parseInt(line.getAttribute('data-source-line')!, 10);
      return n > 0 ? { kind: 'body-line', line: n } : null;
    }

    const none = el.closest('[data-source-none]');
    if (none) return { kind: 'none' };

    return null;
  }

  // ============================================================
  // Click-to-Source Handler (Preview → Editor navigation)
  // ============================================================
  // Preview-click vocabulary: single click = reveal (editor scrolls + flashes, focus stays here),
  // double click = jump (editor focused, caret placed). Disambiguation is by
  // click count — see click-vocabulary.ts for why there is no delay timer.
  // Native word-selection from the double click is left alone: we only post a
  // message, we never touch the selection.

  function postSourceTarget(event: MouseEvent, eventType: 'click' | 'dblclick'): void {
    // Don't interfere with link navigation
    if ((event.target as Element).closest('a')) return;

    const interaction = classifyClickGesture(eventType, event.detail);
    if (!interaction) return;

    const target = resolveSourceTarget(event.target as Element);
    if (!target) return;

    window.parent.postMessage({
      type: 'moss-source-target',
      target,
      interaction,
    }, '*');
  }

  document.addEventListener('click', (event: MouseEvent) => postSourceTarget(event, 'click'));
  document.addEventListener('dblclick', (event: MouseEvent) => postSourceTarget(event, 'dblclick'));

  // Right-click: suppress the native WebKit menu (whose Reload/Back/Forward
  // bypassed the shell's navigation orchestration and whose "Copy Link"
  // handed out localhost URLs) and report the click's context to the shell,
  // which renders the moss menu. Logic lives in bridge/context-menu.ts.
  installContextMenu(window, resolveSourceTarget);

  // ============================================================
  // Selection Sync: Preview → Editor (text selection mapping)
  // ============================================================
  // When the user finishes selecting text, resolve the source target
  // and include the selected text for precise in-source highlighting.

  // Injected once per page load; no teardown path needed.
  let _selectionDebounce: ReturnType<typeof setTimeout> | null = null;

  document.addEventListener('selectionchange', () => {
    // Read selection synchronously NOW — WKWebView collapses cross-origin iframe
    // selections when focus shifts (e.g. on mouse-up). Reading inside a setTimeout
    // would always see a collapsed selection and drop the message.
    const sel = window.getSelection();
    if (!sel || sel.isCollapsed || !sel.rangeCount) {
      if (_selectionDebounce) { clearTimeout(_selectionDebounce); _selectionDebounce = null; }
      return;
    }

    const text = sel.toString().trim();
    if (!text || text.length > 500) {
      if (_selectionDebounce) { clearTimeout(_selectionDebounce); _selectionDebounce = null; }
      return;
    }

    // Capture the start element now (stable during drag) so resolveSourceTarget's
    // DOM walk runs once when the timer fires, not on every selectionchange frame.
    const range = sel.getRangeAt(0);
    const startEl = range.startContainer instanceof Element
      ? range.startContainer
      : range.startContainer.parentElement;

    if (_selectionDebounce) clearTimeout(_selectionDebounce);
    _selectionDebounce = setTimeout(() => {
      const target = resolveSourceTarget(startEl);
      if (!target || target.kind === 'none') return;

      window.parent.postMessage({
        type: 'moss-source-target',
        target: { ...target, text },
        interaction: 'select',
      }, '*');
    }, 50);
  });

  // A press here is a different document: it never bubbles into the shell, and
  // blur cannot stand in once focus is already in this iframe — the state a
  // right-click leaves behind, which is why the shell's own preview context
  // menu could not be dismissed by clicking the page it was about. Capture
  // phase, so a page that stops propagation cannot strand the menu. See
  // dismiss-outside.ts.
  document.addEventListener("mousedown", () => {
    window.parent.postMessage({ type: "moss-preview-pointerdown" }, "*");
  }, true);

  // ============================================================
  // Keyboard Shortcut Handler (Cmd+R / Ctrl+R to refresh)
  // ============================================================
  // When the iframe has focus, keyboard events don't reach the parent
  // window. We capture Cmd+R/Ctrl+R here and notify the parent to refresh.

  document.addEventListener("keydown", (event: KeyboardEvent) => {
    // Cmd+R (macOS) or Ctrl+R (Windows/Linux)
    if ((event.metaKey || event.ctrlKey) && event.key === "r") {
      event.preventDefault();
      window.parent.postMessage({ type: "moss-refresh-request" }, "*");
    }

    // Escape key forwarding (for immersive fullscreen exit)
    // When the interactive iframe has focus, Escape events don't reach the
    // article page where immersive-mode.ts listens. Forward via postMessage.
    if (event.key === "Escape") {
      window.parent.postMessage({ type: "moss-escape-key" }, "*");
    }

    // Preview zoom forwarding (iframe focus path).
    // Cmd+=, Cmd++ → zoom in; Cmd+- → zoom out; Cmd+0 → reset.
    // These are consumed here so the shell's initializeKeyboardShortcuts does
    // not also fire (it only fires when the SHELL window has focus, not the iframe).
    if (event.metaKey || event.ctrlKey) {
      if (event.key === "=" || event.key === "+") {
        event.preventDefault();
        window.parent.postMessage({ type: "moss-zoom-request", delta: 1 }, "*");
      } else if (event.key === "-") {
        event.preventDefault();
        window.parent.postMessage({ type: "moss-zoom-request", delta: -1 }, "*");
      } else if (event.key === "0") {
        event.preventDefault();
        window.parent.postMessage({ type: "moss-zoom-request", delta: 0 }, "*");
      }
    }
  });

  // Trackpad swipe back/forward (macOS): wheel events over the preview land
  // in this iframe, never the shell — see bridge/swipe-gesture.ts.
  installSwipeNavigation(window);

  // Ambient chrome colour. Shell-mounted only: this reports the page's own
  // colours up so the titlebar can look translucent over content that no
  // longer passes beneath it. See bridge/chrome-ambient.ts.
  if (isShellMounted) {
    chromeAmbient = startChromeAmbient(window, (colors) => {
      window.parent.postMessage({ type: "moss-chrome-ambient", colors }, "*");
    });
    // Site identity: the accent token, for the shell's chrome veil and mobile
    // room wash. Nulls are posted too — a token-less page must clear the
    // previous page's sticky tint. See bridge/site-accent.ts.
    startSiteAccent(window, (color) => {
      window.parent.postMessage({ type: "moss-site-accent", color }, "*");
    });
    // The page's backdrop, which the shell turns into `data-chrome-theme`.
    // Owned here rather than in link-preview.ts so it cannot be switched off by
    // `[site].link_preview` — see iframe-bridge-theme.ts for the full story.
    pageColor = startPageColor(window, (color) => {
      window.parent.postMessage({ type: "moss-page-color", color }, "*");
    });
  }


  // ============================================================
  // Asset Path Resolution
  // ============================================================
  // HTML attributes use document-relative paths (e.g., "../clip.mp4") set by
  // adjust_relative_paths_for_pretty_urls, while AssetReady events use
  // site-root-relative paths (e.g., "videos/clip.mp4") from build.rs.
  //
  // toAbsPath resolves document-relative attributes to absolute URL paths
  // using the browser's own URL resolution, making them comparable to
  // event paths (which just need a "/" prefix).
  //
  // The conversion itself is `normPath` in ./asset-urls — shared with the
  // blueprint placeholder rather than reimplemented, which is what this helper's
  // two former hand-synced twins were.
  const toAbsPath = (rel: string | null): string => normPath(rel ?? "");

  // ============================================================
  // Asset Ready Handler (Placeholder Swapping)
  // ============================================================
  // Parent posts moss-asset-ready messages when background assets
  // (images, video thumbnails, converted MP4s) become available.
  // Images: cache-bust src, remove LQIP background on load.
  // Thumbnails: set poster on matching video elements.
  // Videos: force reload of matching video elements.

  // Apply a single asset swap against the CURRENT DOM. Returns the number of
  // elements matched (0 = nothing in the DOM references this asset yet). Pure
  // w.r.t. the buffering below — the message handler decides whether to buffer.
  function applyAssetSwap(assetType: string, eventAbsPath: string): number {
    let matched = 0;
    if (assetType === "thumbnail") {
      // Thumbnail event sends the actual .thumb.jpg path directly
      // (no more re-deriving from .mov source — see asset_paths::to_thumb in Rust)
      document
        .querySelectorAll<HTMLVideoElement>("video[data-thumb-src]")
        .forEach((video) => {
          if (toAbsPath(video.getAttribute("data-thumb-src")) === eventAbsPath) {
            video.setAttribute("poster", eventAbsPath);
            matched++;
          }
        });
    } else if (assetType === "video") {
      // Force reload to pick up the now-available MP4
      document
        .querySelectorAll<HTMLVideoElement>("video[data-placeholder-src]")
        .forEach((video) => {
          if (toAbsPath(video.getAttribute("src")) === eventAbsPath) {
            video.load();
            video.removeAttribute("data-placeholder-src");
            matched++;
          }
        });
    } else if (assetType === "image") {
      // 2026-05-20: matching is by URL against <img src> and <source srcset>,
      // not the legacy data-placeholder-src attribute. Mutating a srcset
      // triggers fresh source-set selection on the parent <picture> per HTML
      // spec; combined with Cache-Control: no-store on the placeholder response
      // this re-fetches the real bytes. For <img src>: cache-bust + LQIP cleanup.
      const bust = "?_t=" + Date.now();

      // srcset matches: <source srcset> inside a <picture>, and the bare
      // <img srcset> ladder html_post.rs also emits.
      //
      // Match per CANDIDATE, not against the whole attribute. moss emits
      // ladders (`photo.w800.webp 800w, photo.w1600.webp 1600w`), so the old
      // whole-string compare matched only single-candidate sources — every
      // laddered cover silently failed to swap and sat on its placeholder until
      // a reload.
      document
        .querySelectorAll<HTMLElement>("source[srcset], img[srcset]")
        .forEach((el) => {
          const srcset = el.getAttribute("srcset") || "";
          if (srcsetMatches(srcset, eventAbsPath)) {
            el.setAttribute("srcset", bustSrcset(srcset, bust));
            matched++;
          }
        });

      // <img src> matches (original URLs and raw-HTML markdown <img>)
      document
        .querySelectorAll<HTMLImageElement>("img[src]")
        .forEach((img) => {
          const cleanSrc = stripBust(img.getAttribute("src") || "");
          if (toAbsPath(cleanSrc) === eventAbsPath) {
            img.src = cleanSrc + bust;
            img.addEventListener("load", () => {
              img.style.removeProperty("background-image");
              img.style.removeProperty("background-size");
            }, { once: true });
            matched++;
          }
        });
    }

    // Finally: undo any blueprint placeholder standing on this asset.
    //
    // A placeheld <img> is INVISIBLE to every selector above — the placeholder
    // moves `src`/`srcset` into `data-moss-ph-*` so the browser stops
    // re-selecting a URL it already failed on. Only the placeholder module knows
    // those attribute names, so we call through the handle it publishes rather
    // than duplicating them here. This is the whole fix for LOG-8D03-T0528-08-08:
    // without it a pending image that errored once matched nothing forever, the
    // swap buffer evicted it after 20 futile drains, and the blueprint grid
    // survived until the user pressed Cmd+R.
    //
    // Runs for EVERY asset type: a missing video poster is an <img> too (the old
    // thumb-swap.js existed only to handle that one case).
    const placeholder = (window as unknown as {
      __mossAssetPlaceholder?: { restore(absPath: string): number };
    }).__mossAssetPlaceholder;
    matched += placeholder?.restore(eventAbsPath) ?? 0;

    return matched;
  }

  // Buffer for swaps that race ahead of the reload's parse (the signal arrives
  // before the matching element exists). The bookkeeping (dedup, bound,
  // clear-on-navigation, drain-and-evict) lives in the unit-tested
  // asset-swap-buffer module; here we wire it to the real DOM apply + a
  // MutationObserver that replays as elements appear.
  const swapBuffer = createAssetSwapBuffer({
    apply: applyAssetSwap,
    currentUrl: () => location.href,
  });
  let pendingObserver: MutationObserver | null = null;

  window.addEventListener("message", (e: MessageEvent) => {
    if (e.data?.type !== "moss-asset-ready") return;

    const { assetType, path } = e.data;
    // Event paths are site-root-relative ("videos/clip.thumb.jpg").
    // Prepend "/" to get the absolute URL path that toAbsPath produces.
    const eventAbsPath = "/" + path;

    // Try immediately; if nothing in the DOM references this asset yet, the
    // buffer holds it and the observer replays once the element appears.
    if (swapBuffer.handle(assetType, eventAbsPath) === "buffered" && !pendingObserver) {
      pendingObserver = new MutationObserver(() => {
        swapBuffer.drain();
        if (swapBuffer.size() === 0 && pendingObserver) {
          pendingObserver.disconnect();
          pendingObserver = null;
        }
      });
      pendingObserver.observe(document.documentElement, { childList: true, subtree: true });
    }
  });

  // ============================================================
  // Scroll Sync: Preview → Editor
  // ============================================================
  // Observes user scroll and reports {url, line, atTop} to the parent
  // window, which forwards to the editor. The atTop flag carries
  // "user is in the chrome-visible region of the preview" — independent
  // of whether any data-source-line element is currently in view.

  let _scrollThrottleTimer: ReturnType<typeof setTimeout> | null = null;

  function reportScrollPosition(): void {
    if (_scrollSyncLock) return;
    const { line, atTop, atBottom } = readScrollPosition(window);
    // Send when there's a line to report OR an edge flag — atBottom must post
    // even when no annotated element sits near the focus line (line stays 0;
    // the receiver's atBottom branch ignores the line).
    if (line > 0 || atBottom) {
      window.parent.postMessage(
        { type: 'moss-scroll-position', url: location.href, line, atTop, atBottom },
        '*'
      );
    }
  }

  window.addEventListener('scroll', () => {
    if (_scrollSyncLock || _scrollThrottleTimer) return;
    _scrollThrottleTimer = setTimeout(() => {
      _scrollThrottleTimer = null;
      reportScrollPosition();
    }, 100);
  }, { passive: true });

  // ============================================================
  // Scroll-coordination protocol (iframe ↔ shell)
  // ============================================================
  // The parent shell paints the fake scrollbar (chrome). This iframe is
  // the single source of scroll truth: it reports scroll geometry to the
  // shell and accepts scroll-to commands. The shell never reads scroll
  // from us — it only reflects what we report.
  //
  // Outgoing message: { type: 'moss-iframe-scroll', scrollTop,
  //                     scrollHeight, clientHeight, gen }
  //   - rAF-coalesced: at most one message per frame.
  //   - Sent on scroll, on resize, on ResizeObserver fires, and once
  //     synchronously at startup so the shell can size the thumb before
  //     the user touches anything.
  //
  // Incoming message: { type: 'moss-iframe-scroll-to', scrollTop }
  //   - Shell sends this when the user drags the scrollbar thumb or
  //     wheels over chrome regions (titlebar, scrollbar track).
  //   - We call window.scrollTo synchronously; the resulting scroll
  //     event fires our reporter, closing the loop.
  //
  // `gen` (navigation generation): a counter the shell uses to drop
  // late messages after the iframe navigates. iframe-resizer #847 is
  // the canonical bug class this prevents.
  // ============================================================

  let _scrollGen = 0;
  let _scrollRafScheduled = false;

  function reportIframeScroll(): void {
    if (_scrollRafScheduled) return;
    _scrollRafScheduled = true;
    requestAnimationFrame(() => {
      _scrollRafScheduled = false;
      const doc = document.documentElement;
      window.parent.postMessage(
        {
          type: "moss-iframe-scroll",
          scrollTop: window.scrollY,
          scrollHeight: doc.scrollHeight,
          clientHeight: window.innerHeight,
          gen: _scrollGen,
        },
        "*"
      );
    });
  }

  window.addEventListener("scroll", reportIframeScroll, { passive: true });
  window.addEventListener("resize", reportIframeScroll, { passive: true });
  // pageshow fires on bfcache restore as well as initial load
  window.addEventListener("pageshow", (e: PageTransitionEvent) => {
    _scrollGen++;
    reportIframeScroll();
    // bfcache restore: document scripts do NOT re-execute, so the init-time
    // notifyNavigation never fires for this traversal. Without an echo the
    // shell's URL pill goes stale and the traversal watchdog would treat a
    // SUCCESSFUL traversal as phantom and drop real history entries. No
    // navType: the performance entry describes the original load, not this
    // restore — the legacy pendingNavType hint classifies it.
    if (e.persisted) notifyNavigation();
  });
  // Document layout can change without window resize (images load,
  // late-injected content). Track scrollHeight changes too.
  if (document.documentElement) {
    new ResizeObserver(reportIframeScroll).observe(document.documentElement);
  }
  // Initial sync push so the shell has dimensions before any user input
  reportIframeScroll();

  window.addEventListener("message", (e: MessageEvent) => {
    if (e.data?.type !== "moss-iframe-scroll-to") return;
    const top = Number(e.data.scrollTop);
    if (!Number.isFinite(top)) return;
    window.scrollTo(0, top);
  });

  // ============================================================
  // In-place morph (Fix A) — apply a rebuilt page WITHOUT location.reload()
  // ============================================================
  //
  // After a rebuild the parent shell posts { type:'moss-morph', gen }
  // instead of reassigning `iframe.src` / calling location.reload(). We
  // re-FETCH our own current URL (same-origin with the preview server — no
  // CORS, unlike a parent fetch from tauri://localhost) and reconcile our
  // document in place with idiomorph. No cross-origin Window teardown, so
  // no WebKit blank-until-scroll (bug #135275); scroll / undo / focus survive.
  //
  // KEYSTONE: this <script> (the bridge IIFE) is NOT re-executed by a
  // morph — idiomorph matches the unchanged <script> node and REUSES it
  // rather than re-inserting, so every closure and listener in this file
  // persists across morphs. That is what makes an in-bridge morph viable
  // at all. Verified by the jsdom harness, which embeds the shipped bundle and
  // morphs it three times. On ANY failure we tell the parent to fall back to
  // location.reload() — the bridge stays alive to receive the next message.
  //
  // Model: Eleventy Dev Server `reload-client.js` morph branch — it too
  // `fetch`es its own `location.href` and idiomorphs the live document
  // (full <head>+<body>, restore focus). Phase-1 scope is REFRESH (same
  // URL); morph-driven navigation is future work.
  window.addEventListener("message", (e: MessageEvent) => {
    if (e.data?.type !== "moss-morph") return;
    const { gen } = e.data as { gen?: number };
    const url = location.href; // same-origin; carries ?__moss_shell=1
    // `cache: "reload"` forces a fresh fetch of the just-rebuilt page.
    fetch(url, { cache: "reload" })
      .then((r) => {
        if (!r.ok) throw new Error(`morph fetch ${r.status}`);
        return r.text();
      })
      .then((html) => {
        const doc = new DOMParser().parseFromString(html, "text/html");
        // A morph REUSES matched <script> nodes and never re-executes them (the
        // keystone above). So if the rebuild changed a site script — the author
        // edited the theme/custom JS, so its content-hashed src changed, or a
        // script was added/removed — a morph would silently NOT pick it up.
        // Bail to a full reload (via the same failed→reload path as a real
        // error) so every script re-runs. Permanent scripts (the bridge) are
        // excluded, so this never false-fires on the injected bridge itself.
        const changedScripts = describeScriptChange(document, doc);
        if (changedScripts) {
          window.parent.postMessage(
            {
              type: "moss-morph-failed",
              url,
              error: "script-changed",
              changedScripts,
              initCount: (window as unknown as { __bridgeInitCount?: number })
                .__bridgeInitCount,
            },
            "*"
          );
          return;
        }
        // NOTE: the morph calls below (veto + head/body innerHTML) are mirrored
        // by a jsdom local harness in
        // __tests__/iframe-bridge-morph.test.ts — keep that
        // `applyMorph()` in sync if you change this block (the shipped bundle is
        // also embedded and morphed there).
        // Escape hatch: never reconcile a JS-managed / third-party subtree
        // (anything under [data-moss-permanent]).
        const beforeNodeMorphed = (oldNode: Node) =>
          !(
            oldNode instanceof Element &&
            oldNode.closest("[data-moss-permanent]")
          );
        // Same escape hatch for REMOVAL. beforeNodeMorphed only guards nodes
        // that MATCH a served counterpart; a JS-appended [data-moss-permanent]
        // node (e.g. a theme's sunlight leaf overlay) has no counterpart in the
        // served bytes, so idiomorph treats it as surplus and removes it —
        // removal is gated ONLY here. Without this veto such overlays vanish on
        // the first content morph while data-theme survives on <html>, leaving
        // "theme on, effect gone."
        // `querySelector` as well as `closest`: idiomorph fires this only on the
        // OUTERMOST surplus node, then removeChild()s its whole subtree without
        // per-descendant callbacks — so a permanent node wrapped in an unmarked
        // surplus container is only protected if we also look downward.
        const beforeNodeRemoved = (node: Node) =>
          !(
            node instanceof Element &&
            (node.closest("[data-moss-permanent]") ||
              node.querySelector("[data-moss-permanent]"))
          );
        // Preserve user-toggled attributes that the baked HTML never carries.
        // idiomorph 0.7.4's morphAttributes removes any attribute absent from
        // the served bytes (it treats the served snapshot as ground truth).
        // `<details open>` is the canonical case: the baked page never has the
        // `open` attr, so idiomorph would remove it on every watch-rebuild
        // morph — slamming the user-opened comments panel shut the moment new
        // comments arrived. Veto the remove so user state survives morphs.
        const beforeAttributeUpdated = (
          name: string,
          node: Node,
          _type: "update" | "remove",
        ) => !(name === "open" && (node as Element).tagName === "DETAILS");
        // Morph <head> and <body> SEPARATELY — NOT an outerHTML morph of
        // `document.documentElement`, which makes idiomorph manage `document`'s
        // own child list and WebKit forbids (`NotSupportedError`). Per-region
        // keeps every mutation inside <head>/<body>. Unchanged nodes — the
        // injected bridge <script>, every <link>/<style> — are matched and
        // REUSED (no script re-exec, no CSS reflash). See idiomorph's
        // morphOuterHTML + normalizeParent.
        //
        // Hand over a DocumentFragment, NOT an innerHTML STRING.
        // idiomorph sniffs a string for a literal `</html>|</head>|</body>` and
        // on a hit parses it as a WHOLE DOCUMENT. `doc.body` always hit: Rust
        // injects this bundle inline before `</body>` and the bundle contains
        // idiomorph, whose fallback parse path holds `"</template></body>"` as
        // a string literal. Every morph therefore re-wrapped the page as
        // `<body><head></head><body>…`, falsifying every `body[...]` descendant
        // rule — loudest the one hiding `.moss-subscribe-form[data-moss-
        // pending-site]`. A fragment has no text to sniff and, unlike `doc.body`
        // itself, no element for `normalizeParent` to wrap. Import into the
        // template's own INERT contents document, not `document`: the live
        // registry would upgrade every matching custom element on a detached
        // clone that is then discarded — a constructor run per morph with no
        // matching `disconnectedCallback` — and it saves one full-tree adoption.
        const fragmentOf = (source: HTMLElement): DocumentFragment => {
          const holder = document.createElement("template");
          const inert = holder.content.ownerDocument;
          for (const node of Array.from(source.childNodes)) {
            holder.content.appendChild(inert.importNode(node, true));
          }
          return holder.content;
        };
        Idiomorph.morph(document.head, fragmentOf(doc.head), {
          morphStyle: "innerHTML",
          ignoreActiveValue: true,
          callbacks: { beforeNodeMorphed, beforeNodeRemoved, beforeAttributeUpdated },
        });
        Idiomorph.morph(document.body, fragmentOf(doc.body), {
          morphStyle: "innerHTML",
          ignoreActiveValue: true,
          restoreFocus: true,
          callbacks: { beforeNodeMorphed, beforeNodeRemoved, beforeAttributeUpdated },
        });
        // Sync <body>'s OWN attributes: `morphStyle: "innerHTML"` reconciles a
        // target's children and never its attributes, so the body attribute set
        // was frozen at page load. shell.html emits `data-content-width` /
        // `data-typesetting` there from frontmatter (the editor's Width and
        // Typesetting chips), so a chip changed the content and not the layout
        // until a manual reload.
        syncBodyAttributes(doc.body);
        // Sync <html> attributes (lang, data-theme, class) — the separate
        // head/body morphs don't touch the root element itself.
        const newRoot = doc.documentElement;
        for (const attr of Array.from(newRoot.attributes)) {
          if (document.documentElement.getAttribute(attr.name) !== attr.value) document.documentElement.setAttribute(attr.name, attr.value);
        }
        // Re-assert the shell-driven theme: the fetched HTML may carry a
        // different (or no) data-theme; the shell's choice wins inside moss.
        // No-op when not shell-mounted; only on a real difference, since a
        // same-value write still queues a mutation record (reporters re-fire).
        if (currentShellTheme && document.documentElement.getAttribute("data-theme") !== currentShellTheme) {
          document.documentElement.setAttribute("data-theme", currentShellTheme);
        }
        // Re-assert shell chrome. The fetched HTML carries `moss-shell-frame`
        // via the ?__moss_shell=1 marker, but re-apply defensively (case 2a).
        if (isShellMounted) document.documentElement.classList.add("moss-shell-frame");
        // Re-assert device mode: the fetched HTML never carries
        // `moss-mobile-frame` (it's runtime-only), and the root-attribute sync
        // above clobbered it from the fetched <html class>. No-op (removes the
        // class) when not in mobile / not shell-mounted.
        applyDeviceMode(currentDeviceMobile);
        // Announce to SITE SCRIPTS (theme.ts et al.) — running in this same
        // iframe document — that the DOM was just patched, so anything that
        // binds listeners once at script-init time can re-scan for elements
        // that only now exist. This closes a real gap: an id-bearing subtree
        // with no prior counterpart (e.g. `.font-anchor`/`.font-trigger`/
        // `#fontPill` appearing for the first time because `date:`
        // frontmatter was just added) is CREATED fresh by idiomorph above —
        // these nodes have never been seen by any init routine, so they
        // start with zero listeners bound. `moss-morph-patched` is a plain
        // in-document CustomEvent (distinct from the cross-frame
        // `moss-morph`/`moss-morph-applied`/`moss-morph-failed` postMessage
        // protocol below, which talks to the PARENT shell, not to sibling
        // site scripts). Listeners are expected to be idempotent per-element
        // (see theme.ts's `initFontPanel`) since this can fire on every morph.
        document.dispatchEvent(new CustomEvent("moss-morph-patched"));
        // A morph is a PURE in-place content update: idiomorph reconciles the
        // DOM without destroying the Window, so scroll, focus and undo survive
        // on their own. Emit NO navigation or scroll signal — a morph is not a
        // navigation and must not move the editor or the preview. Re-emitting
        // notifyNavigation()/reportScrollPosition() here made the shell treat a
        // same-URL re-notify as a reload, run scroll-restore, and yank the
        // editor. The ResizeObserver above refreshes the scrollbar thumb if the
        // height changed. _scrollGen is NOT bumped: same page version/URL.
        //
        // A morph swaps content in place: the page repaints, but `data-theme`
        // is untouched, so the reporters' observer never fires. This is the
        // one path that must ask for a re-read explicitly.
        pageColor?.fire();
        chromeAmbient?.fire();
        void gen; // reserved for Fix D gen-unification; ack-echoed below
        window.parent.postMessage(
          {
            type: "moss-morph-applied",
            url,
            gen,
            // Post-morph title so the shell's nav pill can recover when a morph
            // changed it. A morph is deliberately NOT a navigation (no
            // moss-navigation event), so without this the pill keeps whatever
            // title the last real navigation captured — e.g. it stays stuck on
            // "Not Found" after a refresh heals a transient 404. The shell
            // applies this to the pill only (no scroll/history side effects).
            title: document.title,
            // G3 keystone: the IIFE run-count (must stay 1). Cross-origin, so
            // this ack is the only way to read it from outside — diagnostic
            // only; navigation-manager receives and drops it.
            initCount: (window as unknown as { __bridgeInitCount?: number })
              .__bridgeInitCount,
          },
          "*"
        );
      })
      .catch((err: unknown) => {
        // Bridge stays alive; parent falls back to location.reload().
        const eo = err as { name?: string; message?: string };
        const detail = eo?.name
          ? `${eo.name}: ${eo.message ?? ""}`
          : String(err);
        window.parent.postMessage(
          {
            type: "moss-morph-failed",
            url,
            error: detail,
            initCount: (window as unknown as { __bridgeInitCount?: number })
              .__bridgeInitCount,
          },
          "*"
        );
      });
  });
})();
