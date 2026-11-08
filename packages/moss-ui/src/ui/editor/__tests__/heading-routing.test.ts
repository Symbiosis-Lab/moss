import { describe, it, expect, vi, beforeEach } from 'vitest';
import { routeHeadingCommit } from '../heading-routing';

describe('routeHeadingCommit', () => {
  const renameEntry = vi.fn().mockResolvedValue({ status: 'ok', data: null });
  const registerPendingRename = vi.fn();
  const setTitleField = vi.fn();

  beforeEach(() => {
    renameEntry.mockClear();
    registerPendingRename.mockClear();
    setTitleField.mockClear();
    renameEntry.mockResolvedValue({ status: 'ok', data: null });
  });

  const baseArgs = (overrides: Partial<Parameters<typeof routeHeadingCommit>[0]> = {}) => ({
    newText: 'X',
    currentFilePath: 'posts/foo.md',
    projectRoot: '/proj',
    source: 'Filename' as 'Filename' | 'Title',
    headingStateLoaded: true as boolean,
    renameEntry,
    registerPendingRename,
    setTitleField,
    ...overrides,
  });

  it('returns an error when headingStateLoaded is false (cache not yet loaded)', async () => {
    const result = await routeHeadingCommit(baseArgs({ headingStateLoaded: false }));
    expect(result).toEqual({ ok: false, error: 'Heading state not loaded yet, please retry' });
    expect(renameEntry).not.toHaveBeenCalled();
    expect(setTitleField).not.toHaveBeenCalled();
  });

  // ── Filename source: editing the heading renames the file ──────────────

  it('rejects empty in filename-mode (filenames must be non-empty)', async () => {
    const result = await routeHeadingCommit(baseArgs({ newText: '' }));
    expect(result.ok).toBe(false);
    expect(renameEntry).not.toHaveBeenCalled();
  });

  // renameEntry (Rust) requires ABSOLUTE paths (it checks
  // old.starts_with(project_root)). The router must convert at the FS
  // boundary, matching FileTree.commitRename. `renamedTo` stays relative
  // (the editor's domain). Regression guard for "Paths must be within the
  // project directory" — the bug eyes-on caught that mocked unit tests missed.
  it('renames file with ABSOLUTE paths and returns the relative new path', async () => {
    const result = await routeHeadingCommit(baseArgs({ newText: 'New Name' }));
    expect(result).toEqual({ ok: true, renamedTo: 'posts/New Name.md' });
    expect(renameEntry).toHaveBeenCalledWith('/proj/posts/foo.md', '/proj/posts/New Name.md');
    // Both sides, relative — the domain the watcher echo carries.
    expect(registerPendingRename).toHaveBeenCalledWith('posts/foo.md', 'posts/New Name.md');
    expect(setTitleField).not.toHaveBeenCalled();
  });

  it('preserves the file extension on rename', async () => {
    const result = await routeHeadingCommit(baseArgs({ newText: 'Title', currentFilePath: 'docs/README.md' }));
    expect(renameEntry).toHaveBeenCalledWith('/proj/docs/README.md', '/proj/docs/Title.md');
    expect(result).toEqual({ ok: true, renamedTo: 'docs/Title.md' });
  });

  it('handles root-level files (no slash)', async () => {
    const result = await routeHeadingCommit(baseArgs({ newText: 'About', currentFilePath: 'about.md' }));
    expect(renameEntry).toHaveBeenCalledWith('/proj/about.md', '/proj/About.md');
    expect(result).toEqual({ ok: true, renamedTo: 'About.md' });
  });

  it('returns rename error verbatim', async () => {
    renameEntry.mockResolvedValueOnce({ status: 'error', error: 'name in use' });
    const result = await routeHeadingCommit(baseArgs({ newText: 'Conflict' }));
    expect(result).toEqual({ ok: false, error: 'name in use' });
  });

  // ── Title source: editing the heading writes the frontmatter title ─────

  it('Title source: writes the title field, does not rename', async () => {
    const result = await routeHeadingCommit(baseArgs({ source: 'Title', newText: 'Custom Heading' }));
    expect(result).toEqual({ ok: true });
    expect(setTitleField).toHaveBeenCalledWith('Custom Heading');
    expect(renameEntry).not.toHaveBeenCalled();
    expect(registerPendingRename).not.toHaveBeenCalled();
  });

  it('Title source: empty text writes title: "" (suppresses heading), not a rename', async () => {
    const result = await routeHeadingCommit(baseArgs({ source: 'Title', newText: '' }));
    expect(result).toEqual({ ok: true });
    expect(setTitleField).toHaveBeenCalledWith('');
    expect(renameEntry).not.toHaveBeenCalled();
  });

  it('Title source still refuses commits before heading state loads', async () => {
    const result = await routeHeadingCommit(baseArgs({ source: 'Title', headingStateLoaded: false }));
    expect(result.ok).toBe(false);
    expect(setTitleField).not.toHaveBeenCalled();
  });
});
