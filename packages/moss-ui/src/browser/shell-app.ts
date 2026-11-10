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
import { deliverEditorPosition, sourceTargetPosition } from '../ui/editor/session/editor-position';
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

  const link = new EditorLink({
    preview: () => preview,
    editorIsOpen,
    editorIsFullWidth: () => framesApi().layout() === 'full',
    folderPath: () => folder.path,
    openTarget: (target) => {
      const at = sourceTargetPosition(target);
      if (target.path && target.path !== link.node?.sourcePath) {
        openFileInEditor(panels, editor, target.path, folder.path);
      } else if (!editorIsOpen()) void openPreviewedPage();
      if (at) deliverEditorPosition(at);
    },
    // The desktop's preview menu offers its own actions; a browser has its own context menu.
    openContextMenu: () => {},
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
