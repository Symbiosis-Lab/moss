/**
 * Keep `document.body` in sync with the native (macOS) fullscreen state by
 * toggling the `fullscreen-mode` class. Both the app shell and the editor
 * webview call this so their title-bar chrome — the corner Edit button, the
 * editor breadcrumb, the email-composer titlebar — moves in lockstep when the
 * traffic lights appear/disappear. The editor is a SEPARATE webview/document,
 * so without its own call its breadcrumb would keep the now-empty
 * traffic-light reserve in fullscreen.
 */
import { onMossEvent } from './moss-events';
import { host } from '../host';
import { isMacPlatform } from './kbd-label';

let installed = false;

/**
 * Subscribe the calling document to native-fullscreen transitions and apply the
 * current state immediately. Idempotent (guards against a double-mount), matching
 * the `installLogBridge` precedent.
 *
 * Two event sources, both received because `onMossEvent`'s `listen()` uses the
 * default `Any` target:
 *  - `FullscreenWillChange` — emitted on the AppHandle at the START of the native
 *    transition, before macOS hides/reveals the traffic lights. PRIMARY signal:
 *    moves chrome in sync with the lights (no overlap/flicker on exit).
 *  - `FullscreenChanged` — emitted on the Window from the Resized handler. Backstop.
 *
 * The initial `isFullscreen()` PULL is the race-free source of truth for a
 * freshly-loaded document: a backend page-load PUSH would race this document's
 * async listener setup. Without it, a webview that loads while already in
 * fullscreen (e.g. the editor navigating into `?compose`) would keep the
 * traffic-light inset until the next transition. Degrades gracefully if the
 * window `isFullscreen` capability is absent (events still drive subsequent
 * transitions).
 */
export async function installFullscreenBodyClass(): Promise<void> {
  if (installed) return;
  installed = true;

  const sync = (on: boolean): void => {
    document.body.classList.toggle('fullscreen-mode', on);
  };

  // A host with no native window (a browser tab) has no traffic lights at
  // all — the 76px reserve is pure dead inset that misaligns the breadcrumb
  // spine off the 16px chrome spine. Same state as fullscreen, permanently.
  const win = host.window;
  if (!win) {
    sync(true);
    return;
  }

  await onMossEvent('FullscreenWillChange', (msg) => sync(msg.payload.will_be_fullscreen === true));
  await onMossEvent('FullscreenChanged', (msg) => sync(msg.payload.is_fullscreen === true));

  try {
    sync(await win.isFullscreen());
  } catch {
    // No is-fullscreen capability (or a non-Tauri test env): rely on events.
  }
}

/** Test-only: reset the idempotency latch between unit tests. */
export function __resetFullscreenClassForTest(): void {
  installed = false;
}

/** True on Windows, the one other platform (besides macOS) that ends up
 *  drawing its own caption — see `applyOwnTitlebarClass()` below. Same
 *  userAgentData-with-navigator.platform-fallback shape as `isMacPlatform()`;
 *  kept local rather than folded into kbd-label.ts because nothing outside
 *  this predicate needs it. */
function isWindowsPlatform(): boolean {
  const p =
    (navigator as Navigator & { userAgentData?: { platform?: string } }).userAgentData?.platform ||
    navigator.platform ||
    '';
  return p.toLowerCase().includes('win');
}

/**
 * `body.own-titlebar`: does THIS webview have to draw `[icon] moss` itself.
 * `body.controls-trailing`: does the window's OWN controls (native or
 * moss-drawn) sit at the trailing/right edge, so `.launcher__gear` and its
 * kin mirror to the left.
 *
 * Two platform facts, not one. macOS keeps its title bar but empties it (the
 * native window is created with `TitleBarStyle::Overlay` + `hidden_title`)
 * and its traffic lights sit leading (left), so it gets `own-titlebar` alone.
 * Windows has no hidden-title option, so moss drops the native caption
 * outright (`decorations(false)`) and draws both the `[icon] moss` pair AND
 * its own minimize/close — `own-titlebar` AND `controls-trailing`, since
 * Windows' own caption buttons sit trailing.
 * Linux keeps its native caption (which already shows the app name) but its
 * window controls also sit trailing, so it gets `controls-trailing` alone.
 *
 * `isMacPlatform()` (kbd-label.ts) answers the own-titlebar half for
 * keyboard-shortcut labels too. Call once at boot in every window: the
 * launcher, the app shell and the editor.
 */
export function applyOwnTitlebarClass(): void {
  const mac = isMacPlatform();
  document.body.classList.toggle('own-titlebar', mac || isWindowsPlatform());
  document.body.classList.toggle('controls-trailing', !mac);
}

/**
 * Wire the main shell's own Windows caption — minimize, maximize, close.
 * Drawn under body.own-titlebar.controls-trailing exactly like the
 * launcher's minimize/close pair, plus a real maximize button: unlike the
 * launcher this window is resizable and people maximize it, so it needs a
 * third button the launcher has no use for.
 *
 * Minimize and close are ordinary DOM buttons — nothing else covers them, so
 * a plain click listener is the whole story, exactly like the launcher's.
 *
 * Maximize is different. The host's native Snap Layouts overlay plants an
 * invisible native HWND flush over this button so Windows answers
 * `WM_NCHITTEST` with `HTMAXBUTTON` there and offers the Snap flyout — which
 * means a REAL click or hover in that rectangle is caught by the overlay, not
 * by this DOM element (WebView2 never sees a pointer event there at all once
 * the overlay wins the hit-test). The overlay forwards what it saw as two
 * moss events (`SnapOverlayHover` / `SnapOverlayClick`): hover mirrors the
 * button's highlight since real `:hover` can't fire there, and click drives
 * the actual toggle. A direct click listener stays on the button too, as pure
 * defence rather than the primary path — if the overlay HWND ever fails to
 * create (`CreateWindowExW` failure, logged `warn!` in Rust) the button still
 * works as an ordinary undecorated maximize button, just without the flyout.
 *
 * There is no pressed-state mirror for maximize: the overlay deliberately
 * swallows `WM_NCLBUTTONDOWN` rather than forwarding it (that non-client
 * button state machine belongs to a real TOP-LEVEL caption), so no event
 * exists to drive one. Minimize and close still get a real `:active` from
 * the browser itself.
 */

const MINIMIZE_ID = 'moss-caption-minimize';
const MAXIMIZE_ID = 'moss-caption-maximize';
const CLOSE_ID = 'moss-caption-close';

/** The class the stylesheet keys the maximize button's hover paint off of, since
 *  real `:hover` never reaches it once the Snap Layouts overlay wins the
 *  hit-test (see the doc comment above). Exported for the render gate, which
 *  drives it directly to prove the CSS side without going through the event
 *  plumbing this module owns. */
export const MAXIMIZE_HOVER_CLASS = 'moss-caption-hovering';

let captionInstalled = false;

async function refreshMaximizeGlyph(): Promise<void> {
  const btn = document.getElementById(MAXIMIZE_ID);
  if (!btn) return;
  const maximized = (await host.window?.isMaximized()) ?? false;
  btn.setAttribute('data-maximized', maximized ? 'true' : 'false');
  // TODO: English-only, like the launcher's minimize/close pair. Restore and
  // Maximize need keys in ui-strings.ts.
  btn.setAttribute('aria-label', maximized ? 'Restore' : 'Maximize');
}

async function toggleMaximize(): Promise<void> {
  host.window?.toggleMaximize();
    // The glyph refresh is NOT here. A maximize that does not come from this
  // button (double-click, Win+Up, drag-to-top, Snap Layouts) never calls
  // toggleMaximize(), so the glyph has to follow the window itself. Every real
  // maximize/restore, including this button's own, fires a native Resized,
  // which the FullscreenChanged subscription below turns into a refresh.
}

/**
 * Wire the caption row. Idempotent and safe to call unconditionally on every
 * platform — the buttons are CSS-hidden (`display:none`) everywhere but
 * `body.own-titlebar.controls-trailing`, exactly like the launcher's pair, so
 * a hidden button can never receive the click these listeners answer.
 */
export async function wireMainShellCaptionButtons(): Promise<void> {
  if (captionInstalled) return;
  captionInstalled = true;

  document.getElementById(MINIMIZE_ID)?.addEventListener('click', () => host.window?.minimize());
  document.getElementById(CLOSE_ID)?.addEventListener('click', () => host.window?.close());
  const maximizeBtn = document.getElementById(MAXIMIZE_ID);
  maximizeBtn?.addEventListener('click', () => void toggleMaximize());

  // Gated on `owns-caption`, or one click toggles twice: see
  // applyCaptionOwnerClass().
  await onMossEvent('SnapOverlayHover', (msg) => {
    if (!document.body.classList.contains('owns-caption')) return;
    maximizeBtn?.classList.toggle(MAXIMIZE_HOVER_CLASS, msg.payload.hovering === true);
  });
  await onMossEvent('SnapOverlayClick', () => {
    if (!document.body.classList.contains('owns-caption')) return;
    void toggleMaximize();
  });
  // Backstop for every OS-driven maximize/restore the two listeners above
  // never see: double-click, Win+Up, drag-to-top, Snap Layouts. All of them
  // still fire a native Resized (the host emits FullscreenChanged on every
  // one, unconditionally) even though none of them goes through
  // toggleMaximize().
  await onMossEvent('FullscreenChanged', () => void refreshMaximizeGlyph());

  await refreshMaximizeGlyph();
}

/** Test-only: reset the idempotency latch between unit tests. */
export function __resetCaptionControlsForTest(): void {
  captionInstalled = false;
}

/**
 * Toggle `body.owns-caption` — does THIS webview draw the Windows caption
 * row right now. Split and preview-only: the preview webview owns it.
 * Full-width editor and document mode: the editor webview owns it, because
 * the preview is hidden or was never mounted at all and the window would
 * otherwise have no controls. `body.own-titlebar.controls-trailing
 * .moss-caption-btn` (primitives.css) already gates the buttons' base
 * visibility on PLATFORM; this class is the STATE half — the editor's row
 * stays `display:none` unless both apply, so split mode never shows a second
 * row behind the preview's own.
 *
 * Also the gate `wireMainShellCaptionButtons` reads before acting on a
 * `SnapOverlayHover`/`SnapOverlayClick` event — both webviews wire that
 * listener, since both may draw the row at different times, but only the
 * owner should act on any one event. The gate is NOT redundant with the
 * native side sending via `emit_moss_event_to` to the owning webview label:
 * `preview` and `action-panel` are both CHILD webviews of the same
 * `main` window (`window.add_child`), and `emit_to` does not disambiguate
 * siblings of the same parent, so both webviews still see the event.
 * `emit_moss_event_to` only narrows away from unrelated top-level windows;
 * this gate is what prevents the double-toggle between siblings.
 *
 * The editor calls it on every `editor:full-width-changed` and once at boot
 * (document mode never fires that event, having no preview); the preview
 * calls it at boot (`true`) and where it emits `editor:full-width-changed`
 * (`false` entering full-width, `true` leaving it).
 */
export function applyCaptionOwnerClass(owns: boolean): void {
  document.body.classList.toggle('owns-caption', owns);
}
