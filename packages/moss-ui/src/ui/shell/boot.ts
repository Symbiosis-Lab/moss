/**
 * The shell's composition root: the chrome and the preview every host shows.
 * A host calls `bootShellChrome` before anything renders, `createPreview` once
 * its own listeners are up (then the preview's `initialize`, which awaits the
 * first build), and `EditorLink.connect` where it wires the editor;
 * the desktop's own features are `app.ts`'s.
 */
import { warn } from "../log";
import { followTheme } from "../shared/theme";
import { installControlCharGuard } from "../shared/ui/control-char-guard";
import { initLocale } from "../i18n/locale-detect";
import { applyStaticStrings } from "../i18n/apply-static-strings";
import { installLocaleReactivity } from "../i18n/live-locale";
import { initTooltips } from "../components/tooltip";
import { initEditHistoryMenu } from "../utils/edit-history";
import { onMossSignal } from "../utils/moss-events";
import { PreviewManager } from '../preview/preview-manager';
import { PreviewScrollbar } from '../preview/scrollbar';

export interface ShellChromeHost {
  /** Called once the theme is applied, and on every change to it. */
  themeChanged(): void;
  preview(): PreviewManager | null;
}

export function bootShellChrome(host: ShellChromeHost): void {
  // Theme first: apply, then follow the OS and the other views.
  followTheme();
  host.themeChanged();
  new MutationObserver(() => host.themeChanged()).observe(document.documentElement, {
    attributes: true,
    attributeFilter: ['data-theme'],
  });

  // The locale is resolved before any string renders, and the static markup is
  // localized before the tooltips read its data-tooltip attributes.
  initLocale();
  applyStaticStrings();
  void installLocaleReactivity();
  initTooltips();

  // Edit ▸ Undo/Redo arrive as an event, not a keystroke (edit-history.ts).
  initEditHistoryMenu();
  // View ▸ Zoom acts on the preview whatever has focus; ⌘+/−/0 stay focus-routed.
  void onMossSignal('menu:view-zoom-in', () => host.preview()?.adjustZoom(1));
  void onMossSignal('menu:view-zoom-out', () => host.preview()?.adjustZoom(-1));
  void onMossSignal('menu:view-zoom-actual', () => host.preview()?.adjustZoom(0));

  // macOS multi-webview arrow keys insert control characters into plain inputs.
  installControlCharGuard();
}

export interface PreviewHost {
  /** A build finished on an empty folder. */
  onEmptyFolder(): void;
}

/** The preview and its scrollbar, before its first build. */
export function createPreview(host: PreviewHost): PreviewManager {
  const preview = new PreviewManager('moss-preview-iframe');
  preview.setOnEmptyFolder(() => host.onEmptyFolder());

  // The frame reports its scroll geometry from the first build on, and the scrollbar must hear it.
  const iframe = document.getElementById('moss-preview-iframe') as HTMLIFrameElement | null;
  const previewContainer = document.getElementById('moss-preview-container');
  const track = document.getElementById('moss-fake-scrollbar');
  const thumb = document.getElementById('moss-fake-scrollbar-thumb');
  if (iframe && previewContainer && track && thumb) {
    new PreviewScrollbar({ iframe, track, thumb, titlebar: document.getElementById('moss-titlebar'), previewContainer });
  } else {
    warn('[App] PreviewScrollbar not wired — required DOM nodes missing');
  }
  return preview;
}
