/**
 * context-menu.ts — right-click context for the preview iframe.
 *
 * Design-vocabulary rule: "Every surface owns its right-click; the native
 * webview menu never leaks" (docs/reference/design-vocabulary.md, provenance
 * docs/archive/2026-08-15-context-menu-vocabulary.md). Inside the previewed
 * page the native WebKit menu offered Reload/Back/Forward (bypassing the
 * shell's navigation orchestration) and "Copy Link" (handing out localhost
 * dev-server URLs), so the bridge suppresses it and reports the click's
 * context UP to the shell, which renders the moss menu with the shared
 * ctx-menu primitive.
 *
 * The one deliberate non-suppression: editable targets (inputs, textareas,
 * contenteditable — e.g. a site's search box). WebKit's editable menu carries
 * Paste, which no shell-rendered menu can offer (the clipboard belongs to the
 * focused webview), so suppressing there would take a capability away and
 * give nothing back. Same reasoning as the known-deferred editor-undo item in
 * the decision doc.
 *
 * Coordinates are the iframe's own viewport CSS px (event.clientX/Y). The
 * shell translates them into shell-viewport coordinates using the iframe
 * element's bounding rect — which already reflects the preview-zoom CSS
 * transform — so no zoom math lives here.
 *
 * Standalone module (swipe-gesture.ts pattern): pure payload construction is
 * unit-tested in __tests__/context-menu.test.ts; iframe-bridge.ts only calls
 * `installContextMenu`.
 */

/** What the shell needs to build the menu. Posted to window.parent. */
export interface PreviewContextPayload {
  type: "moss-context-menu";
  /** Pointer position in the IFRAME's viewport CSS px (pre-zoom). */
  x: number;
  y: number;
  /** The iframe document's current URL at click time. */
  page: string;
  /** Resolved source target under the cursor (click-vocabulary shape), or null. */
  target: object | null;
  /** Absolute URL of the enclosing <a>, or null. */
  link: string | null;
  /** Absolute URL of the <img> under the cursor, or null. */
  image: string | null;
  /** Current selection text (trimmed), or null when collapsed/empty. */
  selection: string | null;
}

/**
 * True when the native menu should stay: the click landed in an editable
 * region whose WebKit menu offers Paste/spelling — capabilities a
 * shell-rendered menu cannot replace.
 */
export function isEditableTarget(el: Element | null): boolean {
  if (!el) return false;
  const tag = el.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return true;
  // `=== true` (not truthy): jsdom leaves isContentEditable undefined.
  return el instanceof HTMLElement && el.isContentEditable === true;
}

/** Href schemes that must never surface as a copyable link. */
function isCopyableHref(href: string): boolean {
  return !(
    href.startsWith("#") ||
    href.startsWith("javascript:") ||
    href.startsWith("data:") ||
    href.startsWith("vbscript:")
  );
}

export interface BuildContextOptions {
  x: number;
  y: number;
  /** The iframe document's URL (location.href). */
  pageHref: string;
  /** Raw selection string (window.getSelection()?.toString()). */
  selection: string | null | undefined;
  /** The bridge's resolveSourceTarget, injected to avoid a circular import. */
  resolveSourceTarget(el: Element | null): object | null;
}

/**
 * Build the context payload for a right-click on `el`. Pure: no listeners,
 * no postMessage — jsdom-testable.
 */
export function buildContextPayload(
  el: Element | null,
  opts: BuildContextOptions,
): PreviewContextPayload {
  let link: string | null = null;
  const anchor = el?.closest("a") ?? null;
  const href = anchor?.getAttribute("href");
  if (href && isCopyableHref(href)) {
    try {
      // mailto: et al. resolve fine and ARE copyable addresses.
      link = new URL(href, opts.pageHref).href;
    } catch {
      link = null;
    }
  }

  let image: string | null = null;
  const img = el?.closest("img") as HTMLImageElement | null;
  if (img) {
    // currentSrc reflects source-set selection (the <picture> ladder); fall
    // back to the raw attribute resolved against the document.
    const raw = img.currentSrc || img.getAttribute("src") || "";
    if (raw) {
      try {
        image = new URL(raw, opts.pageHref).href;
      } catch {
        image = null;
      }
    }
  }

  const selText = (opts.selection ?? "").trim();

  return {
    type: "moss-context-menu",
    x: opts.x,
    y: opts.y,
    page: opts.pageHref,
    target: opts.resolveSourceTarget(el),
    link,
    image,
    selection: selText.length > 0 ? selText : null,
  };
}

/**
 * Suppress the native context menu and post the moss context payload to the
 * shell. Editable targets keep the native menu (see isEditableTarget).
 */
export function installContextMenu(
  win: Window,
  resolveSourceTarget: (el: Element | null) => object | null,
): void {
  win.document.addEventListener("contextmenu", (event: MouseEvent) => {
    const el = event.target instanceof Element ? event.target : null;
    if (isEditableTarget(el)) return; // native editable menu stays (Paste)

    event.preventDefault();
    win.parent.postMessage(
      buildContextPayload(el, {
        x: event.clientX,
        y: event.clientY,
        pageHref: win.location.href,
        selection: win.getSelection()?.toString(),
        resolveSourceTarget,
      }),
      "*",
    );
  });
}
