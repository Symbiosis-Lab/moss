import { describe, test, expect, vi } from 'vitest';

type Info = { status: 'ok'; data: { size: number } } | { status: 'error'; error: string };
const pendingInfo: Array<(info: Info) => void> = [];
vi.mock('../../engine', () => ({ engine: { getFileInfo: () => new Promise<Info>((resolve) => pendingInfo.push(resolve)) } }));
import { showFileViewer, destroyFileViewer, fileViewerIsOpen } from '../file-viewer';

describe('file viewer', () => {
  test('an image opens from the file itself, not a built rung, since the viewer zooms to full size', () => {
    const container = document.createElement('div');
    showFileViewer(container, '/site/photos/beach.jpg', '/site', 'image');
    expect(container.querySelector('img')!.getAttribute('src')).toBe('moss-source://localhost/photos/beach.jpg?original');
    destroyFileViewer();
  });

  test('a picture is on screen and counted open before its file info arrives', () => {
    const container = document.createElement('div');
    showFileViewer(container, '/site/photos/beach.jpg', '/site', 'image');
    expect(container.querySelector('img')).not.toBeNull();
    expect(fileViewerIsOpen()).toBe(true);
    destroyFileViewer();
  });

  test('file info that arrives late does not put its picture over the file opened since', async () => {
    const container = document.createElement('div');
    pendingInfo.length = 0;
    showFileViewer(container, '/site/photos/beach.jpg', '/site', 'image');
    showFileViewer(container, '/site/doc.pdf', '/site', 'pdf');
    pendingInfo[0]({ status: 'ok', data: { size: 10 } });
    await new Promise((r) => setTimeout(r));
    expect(container.querySelector('img')).toBeNull();
    expect(container.querySelectorAll('.moss-file-viewer')).toHaveLength(1);
    destroyFileViewer();
    expect(fileViewerIsOpen()).toBe(false);
  });
});
