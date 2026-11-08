/**
 * The shell frame's host members. The server `moss edit` runs is already
 * building and serving the folder, so this host's "build" attaches to that
 * server rather than starting one.
 */
import type { ProgressUpdate } from '@symbiosis-lab/moss-ui';
import { host, type UiHost } from '../ui/host';
import { framesApi, type EditorLayout } from './frames';

/** What `/__moss/config` answers. */
export interface ServeConfig {
  /** Where this browser reaches the preview. */
  preview_url: string;
  /** The HTTPS origin a proxy serves the UI at, when there is one. */
  public_url: string | null;
}

/** The shell's own host members, over the running server's answers. */
export function shellHost(config: ServeConfig, folder: { path: string; name: string | null }): Partial<UiHost> {
  const ok = { status: 'ok', data: null } as const;
  const preview = new URL(config.preview_url);
  const port = Number(preview.port) || (preview.protocol === 'https:' ? 443 : 80);
  return {
    previewUrl: () => config.preview_url,
    openedFolder: () => ({ folder_path: folder.path, auto_serve: true, target_file: null, folder_name: folder.name ?? undefined }),
    build: async (_folder, _autoServe, onProgress: (update: ProgressUpdate) => void) => {
      onProgress({ step: 'serving', message: '', percentage: 100, completed: true, port, is_empty: false });
      return '';
    },
    editorPanel: {
      open: async (sourcePath, isRoot, takeover) => {
        framesApi().openEditor({ sourcePath, isRoot, takeover });
        if (framesApi().layout() === 'closed') setLayout('split');
        return ok;
      },
      close: async () => {
        setLayout('closed');
        return ok;
      },
    },
  };
}

/** Too narrow for two panes: the editor and the preview take turns. */
export const narrow = () => window.parent.matchMedia('(max-width: 760px)').matches;

/** Tells the page, and the editor, how the editor is shown. */
export function setLayout(requested: EditorLayout): void {
  const layout = requested === 'split' && narrow() ? 'full' : requested;
  const wasFull = framesApi().layout() === 'full';
  framesApi().setLayout(layout);
  document.getElementById('moss-edit-button')?.setAttribute('aria-pressed', String(layout !== 'closed'));
  if (wasFull !== (layout === 'full')) host.bus.emit('editor:full-width-changed', { fullWidth: layout === 'full' });
}
