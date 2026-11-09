/**
 * The two frames' bus is one MessageChannel: what one frame says reaches the
 * other and itself, and nothing reaches a bus on another channel (another tab).
 */
import { describe, it, expect, vi } from 'vitest';
import { portBus } from '../port-bus';

describe('the bus between the shell and the editor', () => {
  it('carries a message to the other frame and to the one that sent it', async () => {
    const { port1, port2 } = new MessageChannel();
    const shell = portBus(port1);
    const editor = portBus(port2);
    const inEditor: unknown[] = [];
    const inShell: unknown[] = [];
    editor.on('editor:load-source', (p) => inEditor.push(p));
    shell.on('editor:load-source', (p) => inShell.push(p));

    shell.emit('editor:load-source', { sourcePath: 'a.md' });
    expect(inShell).toEqual([{ sourcePath: 'a.md' }]);
    await vi.waitFor(() => expect(inEditor).toEqual([{ sourcePath: 'a.md' }]));
    port1.close();
  });

  it('keeps one page to its own channel', async () => {
    const tab = new MessageChannel();
    const other = new MessageChannel();
    const heard: unknown[] = [];
    portBus(other.port2).on('editor:goto-line', (p) => heard.push(p));
    const sameTab: unknown[] = [];
    portBus(tab.port2).on('editor:goto-line', (p) => sameTab.push(p));
    portBus(tab.port1).emit('editor:goto-line', { line: 3 });
    await vi.waitFor(() => expect(sameTab).toEqual([{ line: 3 }]));
    expect(heard).toEqual([]);
    tab.port1.close();
    other.port1.close();
  });
});
