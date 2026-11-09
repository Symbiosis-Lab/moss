/**
 * The entry page: the shell frame (chrome and the preview) and the editor
 * frame side by side, as the desktop shows its two webviews. It joins them
 * with one `MessageChannel`, scoped to this page, and owns their layout.
 */
import { PORT_MESSAGE } from './port-bus';

/** Whether the editor is shown, and beside the preview or over it. */
export type EditorLayout = 'closed' | 'split' | 'full';

/** The resolved node the editor opens on, as the desktop's open delivers it. */
export interface EditorNode {
  sourcePath: string | null;
  isRoot: boolean;
  takeover: unknown;
}

/** What the frames ask of the page they are in. Same origin, so a direct call. */
export interface FramesApi {
  openEditor(node: EditorNode): void;
  setLayout(layout: EditorLayout): void;
  layout(): EditorLayout;
}

type EditorWindow = Window & {
  __moss_editor_open?: (node: EditorNode) => void;
  __moss_pending_open?: EditorNode;
};

/** The page's API, from inside one of its frames. */
export function framesApi(): FramesApi {
  return (window.parent as Window & { mossFrames: FramesApi }).mossFrames;
}

export function mountFrames(root: HTMLElement): FramesApi {
  const channel = new MessageChannel();
  const shell = frame('shell.html', 'moss-shell-frame');
  const editor = frame('editor.html', 'moss-editor-frame');
  handPort(shell, channel.port1);
  handPort(editor, channel.port2);
  root.append(shell, editor);

  const api: FramesApi = {
    openEditor(node) {
      // The editor defines its entry point at the end of its init; until then the node waits for it.
      const win = editor.contentWindow as EditorWindow | null;
      if (win?.__moss_editor_open) win.__moss_editor_open(node);
      else if (win) win.__moss_pending_open = node;
    },
    setLayout(layout) {
      root.dataset.editor = layout;
    },
    layout: () => (root.dataset.editor as EditorLayout | undefined) ?? 'closed',
  };
  (window as Window & { mossFrames?: FramesApi }).mossFrames = api;
  return api;
}

function frame(src: string, id: string): HTMLIFrameElement {
  const el = document.createElement('iframe');
  el.id = id;
  el.src = src;
  el.title = '';
  return el;
}

/**
 * Sends the frame its port once its document has loaded. A frame that loads
 * again has lost its end of the channel, and the other frame's end cannot be
 * handed twice, so the whole page starts over.
 */
function handPort(el: HTMLIFrameElement, port: MessagePort): void {
  let sent = false;
  el.addEventListener('load', () => {
    if (sent) {
      location.reload();
      return;
    }
    sent = true;
    el.contentWindow?.postMessage(PORT_MESSAGE, location.origin, [port]);
  });
}
