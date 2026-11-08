import { describe, it, expect, vi } from 'vitest';
import { resolveAgainstFolder, openFileInEditor } from '../open-file-request';

describe('resolveAgainstFolder', () => {
  it('completes the site-root-relative path an advisory carries', () => {
    expect(resolveAgainstFolder('travel/kyoto.md', '/Users/y/Sites/blog'))
      .toBe('/Users/y/Sites/blog/travel/kyoto.md');
  });

  it('does not double the separator when the folder has a trailing slash', () => {
    expect(resolveAgainstFolder('a.md', '/Sites/blog/')).toBe('/Sites/blog/a.md');
  });

  it('leaves an already-absolute path alone, POSIX or Windows', () => {
    expect(resolveAgainstFolder('/Sites/blog/a.md', '/Sites/other')).toBe('/Sites/blog/a.md');
    expect(resolveAgainstFolder('C:\\Sites\\blog\\a.md', '/Sites/other'))
      .toBe('C:\\Sites\\blog\\a.md');
  });

  it('passes a relative path through untouched when no folder is open', () => {
    // Better a path the editor rejects than a wrong one invented from nothing.
    expect(resolveAgainstFolder('a.md', null)).toBe('a.md');
  });
});

describe('openFileInEditor', () => {
  function slots(currentView: unknown = null) {
    const editor = { setNode: vi.fn(), mount: vi.fn() };
    const panel = { getCurrentView: () => currentView ?? null, switchTo: vi.fn() };
    return { editor, panel };
  }

  it('opens the panel on the file when the panel is closed', () => {
    const { editor, panel } = slots();
    openFileInEditor(panel, editor, 'a.md', '/Sites/blog');

    expect(editor.setNode).toHaveBeenCalledWith({
      sourcePath: '/Sites/blog/a.md', isRoot: false, takeover: null,
    });
    expect(panel.switchTo).toHaveBeenCalledWith(editor);
    expect(editor.mount).not.toHaveBeenCalled();
  });

  it('re-mounts an editor that is already the open view, so the file actually changes', () => {
    // switchTo is a no-op for the same instance — the bug this guards is a click
    // that appears to do nothing because the previous file stays on screen.
    const editor = { setNode: vi.fn(), mount: vi.fn() };
    const panel = { getCurrentView: () => editor, switchTo: vi.fn() };

    openFileInEditor(panel, editor, 'b.md', '/Sites/blog');

    expect(editor.setNode).toHaveBeenCalled();
    expect(editor.mount).toHaveBeenCalledTimes(1);
    expect(panel.switchTo).not.toHaveBeenCalled();
  });

  it('does nothing at all for an empty path', () => {
    const { editor, panel } = slots();
    openFileInEditor(panel, editor, '', '/Sites/blog');
    expect(editor.setNode).not.toHaveBeenCalled();
    expect(panel.switchTo).not.toHaveBeenCalled();
  });
});
