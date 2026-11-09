import { describe, test, expect, vi } from 'vitest';

vi.mock('../../engine', () => ({ engine: { getFileInfo: async () => ({ status: 'error', error: 'n/a' }) } }));
import { showFileViewer, destroyFileViewer } from '../file-viewer';

describe('file viewer', () => {
  test('an image opens from the file itself, not a built rung, since the viewer zooms to full size', async () => {
    const container = document.createElement('div');
    showFileViewer(container, '/site/photos/beach.jpg', '/site', 'image');
    await vi.waitFor(() => expect(container.querySelector('img')).not.toBeNull());
    expect(container.querySelector('img')!.getAttribute('src')).toBe('moss-source://localhost/photos/beach.jpg?original');
    destroyFileViewer();
  });
});
