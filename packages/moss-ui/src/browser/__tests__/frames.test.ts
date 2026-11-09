/**
 * The entry page holds the shell and the editor as two frames of its origin,
 * hands each one end of a single channel, and opens the editor on a node the
 * way the desktop's open does.
 */
import { describe, it, expect, vi } from 'vitest';
import { mountFrames } from '../frames';
import { PORT_MESSAGE } from '../port-bus';

describe('the entry page', () => {
  it('hands each frame one end of the same channel once it has loaded', () => {
    const root = document.createElement('div');
    document.body.append(root);
    mountFrames(root);
    const [shell, editor] = [...root.querySelectorAll('iframe')];
    expect(shell.getAttribute('src')).toBe('shell.html');
    expect(editor.getAttribute('src')).toBe('editor.html');

    const sent = [shell, editor].map((frame) => {
      const post = vi.fn();
      Object.defineProperty(frame, 'contentWindow', { value: { postMessage: post }, configurable: true });
      frame.dispatchEvent(new Event('load'));
      return post.mock.calls[0];
    });
    for (const [message, origin, transfer] of sent) {
      expect(message).toBe(PORT_MESSAGE);
      expect(origin).toBe(location.origin);
      expect(transfer).toHaveLength(1);
    }
    expect(sent[0][2][0]).not.toBe(sent[1][2][0]);
  });

  it('delivers an open to a ready editor, and leaves it for one still starting', () => {
    const root = document.createElement('div');
    const api = mountFrames(root);
    const editor = root.querySelectorAll('iframe')[1];
    const node = { sourcePath: '/site/a.md', isRoot: false, takeover: null };
    const starting: Record<string, unknown> = {};
    Object.defineProperty(editor, 'contentWindow', { value: starting, configurable: true });
    api.openEditor(node);
    expect(starting.__moss_pending_open).toBe(node);

    const open = vi.fn();
    Object.defineProperty(editor, 'contentWindow', { value: { __moss_editor_open: open }, configurable: true });
    api.openEditor(node);
    expect(open).toHaveBeenCalledWith(node);
    expect(api.layout()).toBe('closed');
    api.setLayout('split');
    expect(root.dataset.editor).toBe('split');
  });
});
