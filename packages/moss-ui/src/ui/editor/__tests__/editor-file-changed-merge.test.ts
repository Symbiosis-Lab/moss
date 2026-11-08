import { describe, test, expect, vi } from 'vitest';
import { dispatchEditorFileChanged } from '../editor-file-changed';
import type { FileChangeEvent } from '../../preview/preview-actions';
import { PendingRenames } from '../session/pending-renames';

function ev(p: Partial<FileChangeEvent>): FileChangeEvent {
  return {
    modified_paths: [], deleted_paths: [], moved_output_paths: [],
    changed_output_files: [], source_creates: [], source_deletes: [],
    source_renames: [], ...p,
  } as unknown as FileChangeEvent;
}

const OUTSIDE_EDIT_OF_FIRST = {
  changed_output_files: ['notes/first/index.html', 'llms.txt'],
  deleted_paths: null,
  moved_output_paths: null,
  source_creates: null,
  source_deletes: null,
  source_renames: null,
  modified_paths: ['notes/first.md'],
} as FileChangeEvent;

function baseDeps() {
  return {
    registry: { rename: vi.fn(), upsert: vi.fn(), delete: vi.fn(), getByPath: vi.fn((_p: string): number | null => 1) },
    projectRoot: '/proj',
    currentFilePath: (): string | null => 'post.md',
    currentEntryId: (): number | null => 1,
    pendingRenames: new PendingRenames(),
    reloadExternalDocument: vi.fn(async () => {}),
    flashDownloadTargets: vi.fn(),
    onboardingRefresh: vi.fn(async () => {}),
    documentDeletedOutside: vi.fn((_path: string) => false),
    holdsDetachedDocument: vi.fn(() => false),
  };
}

describe('dispatchEditorFileChanged', () => {
  test('a deleted open document with unsaved text stays open: no folder refresh', async () => {
    const d = baseDeps();
    let open: string | null = 'post.md';
    d.currentFilePath = () => open;
    d.registry.delete = vi.fn(() => { open = null; });
    d.documentDeletedOutside = vi.fn(() => true);
    await dispatchEditorFileChanged(ev({ source_deletes: ['post.md'] }), d);
    expect(d.documentDeletedOutside).toHaveBeenCalledWith('post.md');
    expect(d.onboardingRefresh).not.toHaveBeenCalled();
  });

  test('a deleted open document with nothing unsaved closes to its folder', async () => {
    const d = baseDeps();
    let open: string | null = 'post.md';
    d.currentFilePath = () => open;
    d.registry.delete = vi.fn(() => { open = null; });
    await dispatchEditorFileChanged(ev({ source_deletes: ['post.md'] }), d);
    expect(d.onboardingRefresh).toHaveBeenCalledWith('post.md');
  });

  test('while a deleted document is held, later file events leave it alone', async () => {
    const d = baseDeps();
    d.currentFilePath = () => null;
    d.holdsDetachedDocument = vi.fn(() => true);
    await dispatchEditorFileChanged(ev({ source_creates: ['other.md'] }), d);
    expect(d.onboardingRefresh).not.toHaveBeenCalled();
  });

  test('registry sync runs first (rename → create → delete)', async () => {
    const d = baseDeps();
    await dispatchEditorFileChanged(
      ev({ source_renames: [['a.md', 'b.md']], source_creates: ['c.md'], source_deletes: ['x.md'] }),
      d,
    );
    expect(d.registry.rename).toHaveBeenCalledWith('a.md', 'b.md');
    expect(d.registry.upsert).toHaveBeenCalledWith('c.md', false);
    expect(d.registry.delete).toHaveBeenCalledWith('x.md');
  });

  test('open file + external change → delegates to document owner', async () => {
    const d = baseDeps();
    d.currentFilePath = () => 'notes/first.md';
    d.registry.getByPath = vi.fn((p: string) => (p === 'notes/first.md' ? 1 : null));
    await dispatchEditorFileChanged(OUTSIDE_EDIT_OF_FIRST, d);
    expect(d.reloadExternalDocument).toHaveBeenCalled();
  });

  test('open non-markdown file + external change → delegates to document owner', async () => {
    const d = baseDeps();
    d.currentFilePath = () => '.moss/theme/style.css';
    d.registry.getByPath = vi.fn((p: string) => (p === '.moss/theme/style.css' ? 1 : null));
    await dispatchEditorFileChanged(ev({ modified_paths: ['.moss/theme/style.css'] }), d);
    expect(d.reloadExternalDocument).toHaveBeenCalled();
  });

  test('change not affecting the open file → no reload', async () => {
    const d = baseDeps();
    d.registry.getByPath = vi.fn(() => 99); // different entry id
    await dispatchEditorFileChanged(ev({ modified_paths: ['other.md'] }), d);
    expect(d.reloadExternalDocument).not.toHaveBeenCalled();
  });

  test('pending rename round-trip → suppressed', async () => {
    const d = baseDeps();
    d.pendingRenames.register('post.md', 'renamed.md');
    await dispatchEditorFileChanged(ev({ modified_paths: ['post.md'] }), d);
    expect(d.reloadExternalDocument).not.toHaveBeenCalled();
  });

  test('no file open + source_creates → onboarding refresh, no body reload', async () => {
    const d = baseDeps();
    d.currentFilePath = () => null;
    await dispatchEditorFileChanged(ev({ source_creates: ['new.md'] }), d);
    expect(d.flashDownloadTargets).toHaveBeenCalledWith(['new.md']);
    expect(d.onboardingRefresh).toHaveBeenCalled();
    expect(d.reloadExternalDocument).not.toHaveBeenCalled();
  });

  test('FILE OPEN + source_creates → flashDownloadTargets STILL fires (and body reloads)', async () => {
    const d = baseDeps(); // a file ('post.md') is open
    await dispatchEditorFileChanged(ev({ source_creates: ['new.md'], modified_paths: ['post.md'] }), d);
    expect(d.flashDownloadTargets).toHaveBeenCalledWith(['new.md']);
    expect(d.onboardingRefresh).not.toHaveBeenCalled(); // file open → no onboarding surface change
    expect(d.reloadExternalDocument).toHaveBeenCalled(); // body still reloads
  });

  test('a one-sided touch of a pending rename → suppressed, record kept', async () => {
    const d = baseDeps();
    // A rename of some OTHER file that happens to mention our old path on one
    // side is not the definitive echo, so the record must survive it.
    d.pendingRenames.register('post.md', 'renamed.md');
    await dispatchEditorFileChanged(ev({ source_renames: [['post.md', 'elsewhere.md']] }), d);
    expect(d.reloadExternalDocument).not.toHaveBeenCalled();
    await dispatchEditorFileChanged(ev({ modified_paths: ['renamed.md'] }), d);
    expect(d.reloadExternalDocument).not.toHaveBeenCalled();
  });

  test('the matching echo retires the record — the NEXT edit is a real one', async () => {
    // The 1500ms set could not do this: it suppressed every event about the
    // path until the clock ran out, so a genuine external edit landing inside
    // the window was silently dropped.
    const d = baseDeps();
    d.pendingRenames.register('post.md', 'renamed.md');
    await dispatchEditorFileChanged(ev({ source_renames: [['post.md', 'renamed.md']] }), d);
    expect(d.reloadExternalDocument).not.toHaveBeenCalled();

    await dispatchEditorFileChanged(ev({ modified_paths: ['post.md'] }), d);
    expect(d.reloadExternalDocument).toHaveBeenCalled();
  });

  test('one event carrying TWO echoes retires both records', async () => {
    // The debouncer coalesces, which is why source_renames is an array of
    // pairs. Retiring only the first left the second suppressing genuine
    // edits for the full backstop.
    const d = baseDeps();
    d.pendingRenames.register('post.md', 'a.md');
    d.pendingRenames.register('other.md', 'b.md');
    await dispatchEditorFileChanged(
      ev({ source_renames: [['post.md', 'a.md'], ['other.md', 'b.md']] }), d,
    );
    expect(d.reloadExternalDocument).not.toHaveBeenCalled();

    d.currentFilePath = () => 'b.md';
    await dispatchEditorFileChanged(ev({ modified_paths: ['b.md'] }), d);
    expect(d.reloadExternalDocument).toHaveBeenCalled();
  });

  test('null entry id → falls back to path inclusion (matches → reload)', async () => {
    const d = baseDeps();
    d.currentEntryId = () => null;
    // registry.getByPath must NOT be the decider here; path inclusion is.
    await dispatchEditorFileChanged(ev({ modified_paths: ['post.md'] }), d);
    expect(d.reloadExternalDocument).toHaveBeenCalled();
  });

  test('null entry id → path inclusion miss → no reload', async () => {
    const d = baseDeps();
    d.currentEntryId = () => null;
    await dispatchEditorFileChanged(ev({ modified_paths: ['other.md'] }), d);
    expect(d.reloadExternalDocument).not.toHaveBeenCalled();
  });

});

test('deleting the active source rechecks folder truth instead of reloading missing bytes', async () => {
  const d = baseDeps();
  d.currentFilePath = () => 'post.md';
  await dispatchEditorFileChanged(ev({ source_deletes: ['post.md'] }), d);
  expect(d.onboardingRefresh).toHaveBeenCalledWith('post.md');
  expect(d.reloadExternalDocument).not.toHaveBeenCalled();
});

test('directory-only changes recheck the selected folder when no source is open', async () => {
  const d = baseDeps();
  d.currentFilePath = () => null;
  await dispatchEditorFileChanged(ev({ deleted_paths: ['posts/index.html'] }), d);
  expect(d.onboardingRefresh).toHaveBeenCalledOnce();
});
