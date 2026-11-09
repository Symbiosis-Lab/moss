import { describe, it, test, expect } from 'vitest';
import {
  getFileChangeAction,
  hasAnyChanges,
  decideNewPageWait,
  FileChangeEvent,
  NewPageWaitState,
  linkedPaths,
  changedNonPages,
} from '../preview-actions';

// Helper: bindings-generated FileChangeEvent requires all four fields
// (Rust Option<...> → TS T | null). Test fixtures only care about a subset,
// so default the rest to null.
const fce = (overrides: Partial<FileChangeEvent>): FileChangeEvent => ({
  changed_output_files: null,
  deleted_paths: null,
  moved_output_paths: null,
  source_creates: null,
  source_deletes: null,
  source_renames: null,
  modified_paths: null,
  render_seq: null,
  ...overrides,
});

describe('getFileChangeAction', () => {
  describe('null event handling', () => {
    it('returns refresh for null event', () => {
      expect(getFileChangeAction(null, 'blog/post.html')).toEqual({ type: 'refresh' });
    });

    it('returns refresh for null event even without current path', () => {
      expect(getFileChangeAction(null, null)).toEqual({ type: 'refresh' });
    });
  });

  describe('null current path handling', () => {
    it('returns refresh when no current path but there are changes', () => {
      // When navigation history isn't initialized, we can't do smart diff
      // So we fall back to refreshing if there are any changes
      const event = fce({
        deleted_paths: ['blog/post.md'],
      });
      expect(getFileChangeAction(event, null)).toEqual({ type: 'refresh' });
    });

    it('returns none when no current path and no changes', () => {
      const event = fce({
        changed_output_files: [],
      });
      expect(getFileChangeAction(event, null)).toEqual({ type: 'none' });
    });
  });

  describe('deletion detection', () => {
    it('returns redirect-home when current page source is deleted', () => {
      const event = fce({
        deleted_paths: ['blog/post.md'],
      });
      expect(getFileChangeAction(event, 'blog/post.html')).toEqual({ type: 'redirect-home' });
    });

    it('returns redirect-home when current page html is deleted', () => {
      const event = fce({
        deleted_paths: ['blog/post.html'],
      });
      expect(getFileChangeAction(event, 'blog/post.html')).toEqual({ type: 'redirect-home' });
    });

    it('returns none when unrelated file is deleted', () => {
      const event = fce({
        deleted_paths: ['other/page.md'],
      });
      expect(getFileChangeAction(event, 'blog/post.html')).toEqual({ type: 'none' });
    });

    it('handles multiple deleted paths', () => {
      const event = fce({
        deleted_paths: ['other1.md', 'blog/post.md', 'other2.md'],
      });
      expect(getFileChangeAction(event, 'blog/post.html')).toEqual({ type: 'redirect-home' });
    });
  });

  describe('rename detection', () => {
    it('returns update-url when current page is renamed', () => {
      const event = fce({
        moved_output_paths: [['blog/old-post.md', 'blog/new-post.md']],
      });
      expect(getFileChangeAction(event, 'blog/old-post.html')).toEqual({
        type: 'update-url',
        newPath: 'blog/new-post.md',
      });
    });

    it('returns none when unrelated file is renamed', () => {
      const event = fce({
        moved_output_paths: [['other/old.md', 'other/new.md']],
      });
      expect(getFileChangeAction(event, 'blog/post.html')).toEqual({ type: 'none' });
    });

    it('handles multiple renamed paths and finds the match', () => {
      const event = fce({
        moved_output_paths: [
          ['other1/old.md', 'other1/new.md'],
          ['blog/old-post.md', 'blog/new-post.md'],
          ['other2/old.md', 'other2/new.md'],
        ],
      });
      expect(getFileChangeAction(event, 'blog/old-post.html')).toEqual({
        type: 'update-url',
        newPath: 'blog/new-post.md',
      });
    });
  });

  describe('content change detection', () => {
    it('returns refresh when current page is in changed_output_files', () => {
      const event = fce({
        changed_output_files: ['blog/post.html', 'index.html'],
      });
      expect(getFileChangeAction(event, 'blog/post.html')).toEqual({ type: 'refresh' });
    });

    it('returns none when current page is not in changed_output_files', () => {
      const event = fce({
        changed_output_files: ['other.html', 'index.html'],
      });
      expect(getFileChangeAction(event, 'blog/post.html')).toEqual({ type: 'none' });
    });

    it('returns refresh for index.html changes', () => {
      const event = fce({
        changed_output_files: ['index.html'],
      });
      expect(getFileChangeAction(event, 'index.html')).toEqual({ type: 'refresh' });
    });
  });

  describe('priority ordering', () => {
    it('deletion takes priority over rename', () => {
      const event = fce({
        deleted_paths: ['blog/post.md'],
        moved_output_paths: [['blog/post.md', 'blog/new-post.md']],
      });
      expect(getFileChangeAction(event, 'blog/post.html')).toEqual({ type: 'redirect-home' });
    });

    it('deletion takes priority over content change', () => {
      const event = fce({
        deleted_paths: ['blog/post.md'],
        changed_output_files: ['blog/post.html'],
      });
      expect(getFileChangeAction(event, 'blog/post.html')).toEqual({ type: 'redirect-home' });
    });

    it('rename takes priority over content change', () => {
      const event = fce({
        moved_output_paths: [['blog/old.md', 'blog/new.md']],
        changed_output_files: ['blog/old.html'],
      });
      expect(getFileChangeAction(event, 'blog/old.html')).toEqual({
        type: 'update-url',
        newPath: 'blog/new.md',
      });
    });
  });

  describe('empty arrays handling', () => {
    it('handles empty deleted_paths', () => {
      const event = fce({
        deleted_paths: [],
        changed_output_files: ['blog/post.html'],
      });
      expect(getFileChangeAction(event, 'blog/post.html')).toEqual({ type: 'refresh' });
    });

    it('handles empty moved_output_paths', () => {
      const event = fce({
        moved_output_paths: [],
        changed_output_files: ['blog/post.html'],
      });
      expect(getFileChangeAction(event, 'blog/post.html')).toEqual({ type: 'refresh' });
    });

    it('handles empty changed_output_files', () => {
      const event = fce({
        changed_output_files: [],
      });
      expect(getFileChangeAction(event, 'blog/post.html')).toEqual({ type: 'none' });
    });

    it('handles all empty arrays', () => {
      const event = fce({
        deleted_paths: [],
        moved_output_paths: [],
        changed_output_files: [],
      });
      expect(getFileChangeAction(event, 'blog/post.html')).toEqual({ type: 'none' });
    });
  });

  describe('null fields handling', () => {
    it('handles null deleted_paths', () => {
      const event = fce({
        deleted_paths: null,
        changed_output_files: ['blog/post.html'],
      });
      expect(getFileChangeAction(event, 'blog/post.html')).toEqual({ type: 'refresh' });
    });

    it('handles null moved_output_paths', () => {
      const event = fce({
        moved_output_paths: null,
        changed_output_files: ['blog/post.html'],
      });
      expect(getFileChangeAction(event, 'blog/post.html')).toEqual({ type: 'refresh' });
    });

    it('handles null changed_output_files', () => {
      const event = fce({
        changed_output_files: null,
      });
      expect(getFileChangeAction(event, 'blog/post.html')).toEqual({ type: 'none' });
    });
  });
});

describe('hasAnyChanges', () => {
  it('returns false for null event', () => {
    expect(hasAnyChanges(null)).toBe(false);
  });

  it('returns false for empty event', () => {
    const event = fce({});
    expect(hasAnyChanges(event)).toBe(false);
  });

  it('returns false for event with empty arrays', () => {
    const event = fce({
      deleted_paths: [],
      moved_output_paths: [],
      changed_output_files: [],
    });
    expect(hasAnyChanges(event)).toBe(false);
  });

  it('returns true when changed_output_files has entries', () => {
    const event = fce({
      changed_output_files: ['index.html'],
    });
    expect(hasAnyChanges(event)).toBe(true);
  });

  it('returns true when deleted_paths has entries', () => {
    const event = fce({
      deleted_paths: ['blog/post.md'],
    });
    expect(hasAnyChanges(event)).toBe(true);
  });

  it('returns true when moved_output_paths has entries', () => {
    const event = fce({
      moved_output_paths: [['old.md', 'new.md']],
    });
    expect(hasAnyChanges(event)).toBe(true);
  });

  it('ignores modified_paths (editor source-domain signal)', () => {
    const event = fce({
      modified_paths: ['blog/post.md'],
    });
    expect(hasAnyChanges(event)).toBe(false);
  });
});

describe('the new-page wait', () => {
  const state = (overrides: Partial<NonNullable<NewPageWaitState>> = {}): NewPageWaitState => ({
    pathname: '/notes/untitled/index.html',
    targetUrl: 'http://localhost:8080/notes/untitled/index.html',
    rechecks: 0,
    ...overrides,
  });

  describe('decideNewPageWait (a navigation was echoed)', () => {
    it('ignores a navigation when no wait is armed', () => {
      expect(
        decideNewPageWait(null, { pathname: '/anything/index.html', notFound: true })
      ).toEqual({ type: 'ignore' });
    });

    it('keeps waiting while the new page is still the synthetic 404', () => {
      // The 404 is the EXPECTED state here — it is what the wait exists to
      // cover. Only a build or the deadline may end it.
      expect(
        decideNewPageWait(state(), { pathname: state()!.pathname, notFound: true })
      ).toEqual({ type: 'ignore' });
    });

    it('ends the wait the moment the new page serves', () => {
      expect(
        decideNewPageWait(state(), { pathname: state()!.pathname, notFound: false })
      ).toEqual({ type: 'arrived' });
    });

    it('ends the wait when the author navigated somewhere else', () => {
      expect(
        decideNewPageWait(state(), { pathname: '/other/index.html', notFound: false })
      ).toEqual({ type: 'arrived' });
    });

    it('ignores a late echo from a create this wait superseded', () => {
      // Two creates in quick succession: the first one's navigation is still
      // in flight when the second arms, so its echo lands afterwards carrying
      // the OLD pathname. Read as rule 2 it means "the author clicked away",
      // which would uncover the new page for its whole build.
      expect(
        decideNewPageWait(state(), { pathname: '/superseded/', notFound: true }, '/superseded/')
      ).toEqual({ type: 'ignore' });
    });

    it('still ends the wait for a pathname that is not the superseded one', () => {
      expect(
        decideNewPageWait(state(), { pathname: '/other/index.html', notFound: false }, '/superseded/')
      ).toEqual({ type: 'arrived' });
    });

    it('ends the wait even if the page the author moved to is itself a 404', () => {
      // Not `give-up`: the window belongs to whatever is on screen now, and
      // holding "preparing this page" over an unrelated page would be a lie.
      expect(
        decideNewPageWait(state(), { pathname: '/other/index.html', notFound: true })
      ).toEqual({ type: 'arrived' });
    });
  });

});

describe('linkedPaths', () => {
  const page = 'http://localhost:3000/blog/post.html';

  test('resolves every URL attribute against the page, srcset candidates included', () => {
    const paths = linkedPaths([
      { attributes: { rel: 'stylesheet', href: '../notes/handout.css' } },
      { attributes: { src: '/_moss/js/theme.abc.js' } },
      { attributes: { srcset: 'photo.w800.webp 800w, photo.w1600.webp 1600w' } },
      { attributes: { poster: 'clip.thumb.jpg?_t=5' } },
    ], page);
    expect([...paths].sort()).toEqual([
      '_moss/js/theme.abc.js',
      'blog/clip.thumb.jpg',
      'blog/photo.w1600.webp',
      'blog/photo.w800.webp',
      'notes/handout.css',
    ]);
  });

  test('decodes percent-encoded segments, as changed_output_files are not encoded', () => {
    const paths = linkedPaths([{ attributes: { src: '%E9%97%9C%E6%96%BC/%E9%A0%AD%E5%83%8F.webp' } }], 'http://localhost:3000/');
    expect([...paths]).toEqual(['關於/頭像.webp']);
  });

  test('leaves out off-site URLs', () => {
    expect(linkedPaths([{ attributes: { href: 'https://fonts.example.com/a.css' } }], page).size).toBe(0);
  });
});

describe('changedNonPages', () => {
  test('keeps the outputs a page can load, not pages', () => {
    expect(changedNonPages({ changed_output_files: ['index.html', 'notes/handout.css', 'rss.xml'] } as FileChangeEvent))
      .toEqual(['notes/handout.css', 'rss.xml']);
  });
});
