/**
 * The browser shell: the chrome, the preview, and the editor beside it. The
 * desktop's own features (publishing, settings, plugins) are not here.
 */
import { host } from '../ui/host';
import { t } from '../ui/i18n/index';
import { bootShellChrome, createPreview } from '../ui/shell/boot';
import { EditorLink } from '../ui/shell/editor-link';
import { ActionPanelManager } from '../ui/editor/action-panel-manager';
import { EditorPanelView } from '../ui/editor/editor-panel-view';
import { openFileInEditor } from '../ui/editor/session/open-file-request';
import { deliverEditorPosition, EditorHandshake, sourceTargetPosition, type EditorPosition } from '../ui/editor/session/editor-position';
import { openPreviewCtxMenu } from '../ui/preview/preview-context-menu';
import { toRelative } from '../ui/editor/path-domain';
import type { PreviewManager } from '../ui/preview/preview-manager';
import { framesApi } from './frames';
import { mountDevicePanel } from './device-panel';
import { narrow, setLayout, type ServeConfig } from './shell-host';

export async function startShell(
  folder: { path: string; name: string | null } | null | 'signed-out',
  config: ServeConfig,
): Promise<void> {
  if (folder === null || folder === 'signed-out') {
    showNotice(t(folder ? 'browser.signed_out' : 'browser.no_folder'));
    return;
  }
  let preview: PreviewManager | null = null;
  const panels = new ActionPanelManager();
  const editor = new EditorPanelView({ onMountError: () => setLayout('closed') });
  const editorIsOpen = () => panels.getCurrentView() === editor && framesApi().layout() !== 'closed';

  const openPreviewedPage = async () => {
    editor.setNode(link.openNode());
    // Reopening a hidden editor mounts it again, so it shows this page.
    if (panels.getCurrentView() === editor) editor.mount();
    else await panels.switchTo(editor);
  };
  const close = async () => {
    await panels.close();
    setLayout('closed');
  };

  const handshake = new EditorHandshake((path) => toRelative(path, folder.path));
  host.bus.on<{ path: string }>('editor:file-ready', (payload) => handshake.ready(payload.path));

  /** Open the editor on `path` (null: the previewed page) with the caret at `at`. */
  const openAt = (path: string | null, at: EditorPosition | null) => {
    // A file that still has to load takes its caret once the editor says it is ready.
    if (path) {
      handshake.expect(path, at);
      // An open editor loads by name, which reports ready even for the file it already shows.
      if (editorIsOpen()) host.bus.emit('editor:load-file', { filePath: path });
      else openFileInEditor(panels, editor, path, folder.path);
    } else if (!editorIsOpen()) {
      handshake.expect(null, at);
      void openPreviewedPage();
    } else if (at) deliverEditorPosition(at);
  };

  const link = new EditorLink({
    preview: () => preview,
    editorIsOpen,
    editorIsFullWidth: () => framesApi().layout() === 'full',
    folderPath: () => folder.path,
    openTarget: (target) => openAt(target?.path ?? null, target && sourceTargetPosition(target)),
    openContextMenu: (ctx) => openPreviewCtxMenu(ctx, link, {
      // The browser cannot read the published address, so copies are site paths and say so.
      siteUrl: null,
      copy: (text) => void navigator.clipboard?.writeText(text).catch(() => {}),
      openExternal: (url) => void window.open(url, '_blank', 'noopener'),
      openStyle: (selector) => openAt('.moss/theme/style.css', { kind: 'rule', selector }),
    }),
  });

  bootShellChrome({ themeChanged: () => {}, preview: () => preview });
  link.connect();
  host.bus.on('editor:request-full-width', () => editorIsOpen() && setLayout('full'));
  // On a narrow screen "split" goes back to the preview.
  host.bus.on('editor:request-split', () => editorIsOpen() && (narrow() ? void close() : setLayout('split')));
  window.parent.addEventListener('resize', () => editorIsOpen() && setLayout(framesApi().layout()));

  const edit = document.getElementById('moss-edit-button');
  edit?.addEventListener('click', () => void (editorIsOpen() ? close() : openPreviewedPage()));
  mountDevicePanel(document.getElementById('moss-device-button'), config);

  preview = createPreview({ onEmptyFolder: () => void (editorIsOpen() || openPreviewedPage()) });
  await preview.initialize();
}

/** One line in place of the preview, for a browser that cannot show it. */
function showNotice(text: string): void {
  const overlay = document.getElementById('moss-preview-overlay');
  const notice = document.createElement('p');
  notice.className = 'moss-browser-notice';
  notice.textContent = text;
  overlay?.replaceChildren(notice);
}
