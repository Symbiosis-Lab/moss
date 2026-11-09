import { describe, test, expect, beforeEach, vi } from 'vitest';
import { EntryRegistry } from '../entry-registry';
import type { EntryId } from '../entry-registry';

describe('EntryRegistry', () => {
  let registry: EntryRegistry;

  beforeEach(() => {
    registry = new EntryRegistry();
  });

  describe('upsert', () => {
    test('mints a new EntryId for an unknown path', () => {
      const id = registry.upsert('blog/post.md', false);
      expect(id).toBeGreaterThan(0);
      expect(registry.getById(id)).toEqual({
        id,
        path: 'blog/post.md',
        isDir: false,
        fileId: null,
        isHome: false,
        special: null,
        hidden: 0,
      });
    });

    test('returns the existing EntryId for a known path (idempotent)', () => {
      const id1 = registry.upsert('blog/post.md', false);
      const id2 = registry.upsert('blog/post.md', false);
      expect(id2).toBe(id1);
    });

    test('mints distinct EntryIds for distinct paths', () => {
      const id1 = registry.upsert('a.md', false);
      const id2 = registry.upsert('b.md', false);
      expect(id1).not.toBe(id2);
    });

    test('updates isDir on subsequent upserts of the same path', () => {
      const id = registry.upsert('foo', false);
      registry.upsert('foo', true);
      expect(registry.getById(id)?.isDir).toBe(true);
    });

    test('IDs are monotonically increasing', () => {
      const a = registry.upsert('a.md', false);
      const b = registry.upsert('b.md', false);
      const c = registry.upsert('c.md', false);
      expect(b).toBeGreaterThan(a);
      expect(c).toBeGreaterThan(b);
    });
  });

  describe('rename', () => {
    test('preserves the EntryId; updates path; moves byPath index', () => {
      const id = registry.upsert('old.md', false);
      const returned = registry.rename('old.md', 'new.md');
      expect(returned).toBe(id);
      expect(registry.getByPath('old.md')).toBeNull();
      expect(registry.getByPath('new.md')).toBe(id);
      expect(registry.getById(id)?.path).toBe('new.md');
    });

    test('returns null for an unknown oldPath (no-op)', () => {
      const result = registry.rename('never-existed.md', 'destination.md');
      expect(result).toBeNull();
      expect(registry.getByPath('destination.md')).toBeNull();
    });

    test('chained renames preserve a single EntryId across the chain', () => {
      const id = registry.upsert('a.md', false);
      registry.rename('a.md', 'b.md');
      registry.rename('b.md', 'c.md');
      expect(registry.getByPath('c.md')).toBe(id);
      expect(registry.getByPath('a.md')).toBeNull();
      expect(registry.getByPath('b.md')).toBeNull();
      expect(registry.getById(id)?.path).toBe('c.md');
    });

    test('renaming to a path that already exists overwrites the destination entry', () => {
      const idA = registry.upsert('a.md', false);
      registry.upsert('b.md', false);
      const returned = registry.rename('a.md', 'b.md');
      expect(returned).toBe(idA);
      expect(registry.getByPath('b.md')).toBe(idA);
      expect(registry.getById(idA)?.path).toBe('b.md');
    });

    test('renaming a folder carries every descendant with it, ids intact', () => {
      // The registry is the tree model: a folder rename on disk moves the
      // whole subtree, so the model does too, or every child path is stale
      // until the next listing and the tree paints rows disk cannot open.
      const dir = registry.upsert('posts', true);
      const child = registry.upsert('posts/a.md', false);
      const deep = registry.upsert('posts/2026/b.md', false);
      const sibling = registry.upsert('post', false); // `post` is not under `posts`
      registry.setActive(deep);
      const paths = vi.fn();
      registry.subscribeActivePath(paths);

      registry.rename('posts', 'writing');

      expect(registry.getByPath('writing')).toBe(dir);
      expect(registry.getByPath('writing/a.md')).toBe(child);
      expect(registry.getByPath('writing/2026/b.md')).toBe(deep);
      expect(registry.getByPath('posts/a.md')).toBeNull();
      expect(registry.getById(sibling)?.path).toBe('post');
      expect(paths).toHaveBeenCalledWith('writing/2026/b.md', 'posts/2026/b.md');
    });
  });

  describe('delete', () => {
    test('clears both byPath and byId indices', () => {
      const id = registry.upsert('doomed.md', false);
      registry.delete('doomed.md');
      expect(registry.getByPath('doomed.md')).toBeNull();
      expect(registry.getById(id)).toBeNull();
    });

    test('a subsequent upsert at the same path mints a NEW id', () => {
      const id1 = registry.upsert('foo.md', false);
      registry.delete('foo.md');
      const id2 = registry.upsert('foo.md', false);
      expect(id2).not.toBe(id1);
    });

    test('is a no-op for an unknown path', () => {
      registry.delete('never-existed.md');
      expect(registry.getByPath('never-existed.md')).toBeNull();
    });

    test('deleting a folder retires every descendant, and clears an active one', () => {
      registry.upsert('posts', true);
      const deep = registry.upsert('posts/2026/b.md', false);
      const sibling = registry.upsert('post', false);
      registry.setActive(deep);

      registry.delete('posts');

      expect(registry.getByPath('posts/2026/b.md')).toBeNull();
      expect(registry.getById(deep)).toBeNull();
      expect(registry.getById(sibling)?.path).toBe('post');
      expect(registry.activeEntryId).toBeNull();
    });
  });

  test('renaming an NFD-listed folder by its NFC spelling carries the subtree to clean paths', () => {
    // The typed spelling is one code point shorter than the listed one; the
    // descendant tail must be cut at the RECORD's length, not the argument's,
    // or every moved path ends in a stray combining accent.
    const registry = new EntryRegistry();
    const NFD = 'café';
    registry.reconcileSnapshot([
      { path: NFD, isDir: true, fileId: null },
      { path: `${NFD}/a.md`, isDir: false, fileId: null },
    ], { pathPrefix: '' });
    const child = registry.getByPath(`${NFD}/a.md`);
    registry.rename('caf\u00e9', 'nouveau');
    expect(registry.getById(child!)?.path).toBe('nouveau/a.md');
    expect(registry.getByPath('nouveau')).not.toBeNull();
  });

  describe('Unicode spelling', () => {
    // 'café' precomposed (what a keyboard emits) vs decomposed (what a macOS
    // listing emits): one folder, two strings. The index compares in one form;
    // the stored path is always the spelling disk reported.
    const NFC = 'caf\u00e9';   // precomposed
    const NFD = 'cafe\u0301';  // decomposed

    test('getByPath finds a record under either spelling', () => {
      expect(NFC).not.toBe(NFD);
      const id = registry.upsert(NFC, true);
      expect(registry.getByPath(NFD)).toBe(id);
      expect(registry.getByPath(`${NFD}/a.md`)).toBeNull();
    });

    test('a listing in the other spelling preserves the id and the stored path follows the listing', () => {
      const id = registry.upsert(NFC, true, null);
      const result = registry.reconcileSnapshot([{ path: NFD, isDir: true, fileId: null }]);
      expect(result.preserved).toEqual([id]);
      expect(result.retired).toEqual([]);
      expect(registry.getById(id)?.path).toBe(NFD);
      expect(registry.allIds().map(id => registry.getById(id)!.path)).toEqual([NFD]);
    });

    test('rename to the other spelling of the same name keeps one record', () => {
      const id = registry.upsert(NFD, true);
      expect(registry.rename(NFD, NFC)).toBe(id);
      expect(registry.allIds()).toEqual([id]);
      expect(registry.getById(id)?.path).toBe(NFC);
    });
  });

  describe('lookups', () => {
    test('getByPath returns null for an unknown path', () => {
      expect(registry.getByPath('nothing.md')).toBeNull();
    });

    test('getById returns null for an unknown id', () => {
      expect(registry.getById(9999 as EntryId)).toBeNull();
    });

    test('two indices stay consistent after multi-op sequence', () => {
      const a = registry.upsert('a.md', false);
      const b = registry.upsert('b.md', false);
      registry.rename('a.md', 'a-renamed.md');
      registry.delete('b.md');
      const c = registry.upsert('c.md', false);

      expect(registry.getByPath('a-renamed.md')).toBe(a);
      expect(registry.getByPath('a.md')).toBeNull();
      expect(registry.getByPath('b.md')).toBeNull();
      expect(registry.getById(b)).toBeNull();
      expect(registry.getByPath('c.md')).toBe(c);
      expect(registry.getById(c)?.path).toBe('c.md');
    });
  });

  describe('allIds', () => {
    test('returns empty array on a fresh registry', () => {
      expect(registry.allIds()).toEqual([]);
    });

    test('returns every upserted id, with duplicates deduped to one entry', () => {
      const a = registry.upsert('a.md', false);
      const b = registry.upsert('b.md', false);
      registry.upsert('a.md', false);   // duplicate upsert
      const ids = registry.allIds();
      expect(ids.length).toBe(2);
      expect(ids).toContain(a);
      expect(ids).toContain(b);
    });

    test('drops deleted ids', () => {
      registry.upsert('a.md', false);
      const b = registry.upsert('b.md', false);
      registry.delete('a.md');
      const ids = registry.allIds();
      expect(ids.length).toBe(1);
      expect(ids).toContain(b);
    });

    test('survives rename without changing id count', () => {
      const a = registry.upsert('a.md', false);
      registry.upsert('b.md', false);
      registry.rename('a.md', 'a-renamed.md');
      const ids = registry.allIds();
      expect(ids.length).toBe(2);
      expect(ids).toContain(a);   // id is stable across rename
    });
  });

  describe('activeEntryId ownership', () => {
    test('starts null', () => {
      expect(registry.activeEntryId).toBeNull();
    });

    test('setActive updates the active id', () => {
      const id = registry.upsert('foo.md', false);
      registry.setActive(id);
      expect(registry.activeEntryId).toBe(id);
    });

    test('setActive(null) clears the active id', () => {
      const id = registry.upsert('foo.md', false);
      registry.setActive(id);
      registry.setActive(null);
      expect(registry.activeEntryId).toBeNull();
    });

    test('setActive is idempotent: no event when value unchanged', () => {
      const id = registry.upsert('foo.md', false);
      const handler = vi.fn();
      registry.subscribeActive(handler);
      registry.setActive(id);
      registry.setActive(id);
      expect(handler).toHaveBeenCalledTimes(1);
    });

    test('subscribeActive fires with (newId, oldId) on change', () => {
      const a = registry.upsert('a.md', false);
      const b = registry.upsert('b.md', false);
      const handler = vi.fn();
      registry.subscribeActive(handler);
      registry.setActive(a);
      expect(handler).toHaveBeenLastCalledWith(a, null);
      registry.setActive(b);
      expect(handler).toHaveBeenLastCalledWith(b, a);
      registry.setActive(null);
      expect(handler).toHaveBeenLastCalledWith(null, b);
    });

    test('subscribeActive returns an unsubscribe function', () => {
      const id = registry.upsert('foo.md', false);
      const handler = vi.fn();
      const unsub = registry.subscribeActive(handler);
      registry.setActive(id);
      expect(handler).toHaveBeenCalledTimes(1);
      unsub();
      registry.setActive(null);
      expect(handler).toHaveBeenCalledTimes(1);
    });

    test('multiple subscribers all fire in registration order', () => {
      const id = registry.upsert('foo.md', false);
      const order: string[] = [];
      registry.subscribeActive(() => order.push('a'));
      registry.subscribeActive(() => order.push('b'));
      registry.setActive(id);
      expect(order).toEqual(['a', 'b']);
    });

    test('one subscriber throwing does not abort other subscribers', () => {
      const id = registry.upsert('foo.md', false);
      const a = vi.fn(() => { throw new Error('boom'); });
      const b = vi.fn();
      const errSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
      try {
        registry.subscribeActive(a);
        registry.subscribeActive(b);
        registry.setActive(id);
        expect(a).toHaveBeenCalledOnce();
        expect(b).toHaveBeenCalledOnce();
        expect(errSpy).toHaveBeenCalled();
      } finally {
        errSpy.mockRestore();
      }
    });

    test('rename preserves activeEntryId: path changes, id stays', () => {
      const id = registry.upsert('old.md', false);
      registry.setActive(id);
      registry.rename('old.md', 'new.md');
      expect(registry.activeEntryId).toBe(id);
      expect(registry.getById(id)?.path).toBe('new.md');
    });

    test('delete of active path nulls activeEntryId AND fires subscriber once', () => {
      const id = registry.upsert('foo.md', false);
      registry.setActive(id);
      const handler = vi.fn();
      registry.subscribeActive(handler);
      registry.delete('foo.md');
      expect(registry.activeEntryId).toBeNull();
      expect(handler).toHaveBeenCalledTimes(1);
      expect(handler).toHaveBeenCalledWith(null, id);
    });
  });

  describe('reconcileSnapshot', () => {
    test('mints EntryIds for new paths', () => {
      const result = registry.reconcileSnapshot([
        { path: 'a.md', isDir: false, fileId: null },
        { path: 'b.md', isDir: false, fileId: null },
      ]);
      expect(result.minted.length).toBe(2);
      expect(result.retired.length).toBe(0);
      expect(result.preserved.length).toBe(0);
      expect(registry.getByPath('a.md')).not.toBeNull();
    });

    test('preserves existing EntryIds for paths still present', () => {
      const id = registry.upsert('a.md', false);
      const result = registry.reconcileSnapshot([
        { path: 'a.md', isDir: false, fileId: null },
        { path: 'b.md', isDir: false, fileId: null },
      ]);
      expect(result.preserved).toEqual([id]);
      expect(result.minted.length).toBe(1);
      expect(registry.getByPath('a.md')).toBe(id);
    });

    test('retires EntryIds for paths no longer present', () => {
      const a = registry.upsert('a.md', false);
      const b = registry.upsert('b.md', false);
      const result = registry.reconcileSnapshot([{ path: 'a.md', isDir: false, fileId: null }]);
      expect(result.preserved).toEqual([a]);
      expect(result.retired).toEqual([b]);
      expect(registry.getByPath('a.md')).toBe(a);
      expect(registry.getByPath('b.md')).toBeNull();
      expect(registry.getById(b)).toBeNull();
    });

    test('clears activeEntryId when active entry is retired and fires subscriber', () => {
      const a = registry.upsert('a.md', false);
      const b = registry.upsert('b.md', false);
      registry.setActive(b);
      const handler = vi.fn();
      registry.subscribeActive(handler);
      registry.reconcileSnapshot([{ path: 'a.md', isDir: false, fileId: null }]);
      expect(registry.activeEntryId).toBeNull();
      expect(handler).toHaveBeenCalledOnce();
      expect(handler).toHaveBeenCalledWith(null, b);
    });

    test('updates isDir on existing entries; reports them as preserved, not retired', () => {
      const id = registry.upsert('foo', false);
      const result = registry.reconcileSnapshot([{ path: 'foo', isDir: true, fileId: null }]);
      expect(registry.getById(id)?.isDir).toBe(true);
      expect(result.preserved).toContain(id);
      expect(result.retired).toEqual([]);
      expect(result.minted).toEqual([]);
    });

    test('pathPrefix: entries outside prefix are not retired', () => {
      const inside = registry.upsert('blog/post.md', false);
      const outside = registry.upsert('archive/old.md', false);
      registry.reconcileSnapshot(
        [{ path: 'blog/post.md', isDir: false, fileId: null }],
        { pathPrefix: 'blog' },
      );
      expect(registry.getByPath('archive/old.md')).toBe(outside);
      expect(registry.getById(inside)?.path).toBe('blog/post.md');
    });

    test('pathPrefix: trailing slash is normalized', () => {
      const id = registry.upsert('blog/post.md', false);
      registry.reconcileSnapshot(
        [{ path: 'blog/post.md', isDir: false, fileId: null }],
        { pathPrefix: 'blog/' },
      );
      expect(registry.getByPath('blog/post.md')).toBe(id);
    });

    test('excludePrefixes: paths matching any exclude prefix survive', () => {
      const main = registry.upsert('blog/post.md', false);
      const internal = registry.upsert('blog/.moss/config.toml', false);
      registry.reconcileSnapshot(
        [{ path: 'blog/post.md', isDir: false, fileId: null }],
        { pathPrefix: 'blog', excludePrefixes: ['blog/.moss'] },
      );
      expect(registry.getByPath('blog/post.md')).toBe(main);
      expect(registry.getByPath('blog/.moss/config.toml')).toBe(internal);
    });

    test('carries the home election, and refreshes it on a preserved record', () => {
      // The backend elects at most one home per folder and the election can
      // move between listings (a marker added, an index.md created), so a
      // preserved record takes the new flag rather than keeping its first.
      registry.reconcileSnapshot([
        { path: 'index.md', isDir: false, fileId: null, isHome: true },
        { path: 'about.md', isDir: false, fileId: null },
      ]);
      const index = registry.getByPath('index.md')!;
      const about = registry.getByPath('about.md')!;
      expect(registry.getById(index)?.isHome).toBe(true);
      expect(registry.getById(about)?.isHome).toBe(false);

      registry.reconcileSnapshot([
        { path: 'index.md', isDir: false, fileId: null },
        { path: 'about.md', isDir: false, fileId: null, isHome: true },
      ]);
      expect(registry.getByPath('index.md')).toBe(index);
      expect(registry.getById(index)?.isHome).toBe(false);
      expect(registry.getById(about)?.isHome).toBe(true);
    });

    test('carries the special kind moss reports, keeps it across upsert, and refreshes it', () => {
      registry.reconcileSnapshot([
        { path: 'AGENTS.md', isDir: false, fileId: null, special: 'agent-instructions' },
        { path: 'about.md', isDir: false, fileId: null },
      ]);
      const agents = registry.getByPath('AGENTS.md')!;
      expect(registry.getById(agents)?.special).toBe('agent-instructions');
      expect(registry.getById(registry.getByPath('about.md')!)?.special).toBeNull();

      registry.upsert('AGENTS.md', false);
      expect(registry.getById(agents)?.special).toBe('agent-instructions');

      registry.reconcileSnapshot([
        { path: 'AGENTS.md', isDir: false, fileId: null },
        { path: 'about.md', isDir: false, fileId: null, special: 'footer' },
      ]);
      expect(registry.getByPath('AGENTS.md')).toBe(agents);
      expect(registry.getById(agents)?.special).toBeNull();
      expect(registry.getById(registry.getByPath('about.md')!)?.special).toBe('footer');
    });

    test('empty snapshot with empty prefix retires all entries', () => {
      registry.upsert('a.md', false);
      registry.upsert('b.md', false);
      registry.reconcileSnapshot([]);
      expect(registry.getByPath('a.md')).toBeNull();
      expect(registry.getByPath('b.md')).toBeNull();
    });
  });

  describe('reconcileSnapshot — byFileId identity reuse', () => {
    test('rename to a new path with same fileId preserves the EntryId', () => {
      const x = registry.upsert('old.md', false, '42');
      registry.setActive(x);

      const result = registry.reconcileSnapshot([
        { path: 'new.md', isDir: false, fileId: '42' },
      ]);

      expect(registry.getByPath('new.md')).toBe(x);
      expect(registry.getByPath('old.md')).toBeNull();
      expect(registry.getById(x)?.path).toBe('new.md');
      expect(registry.getById(x)?.fileId).toBe('42');
      expect(registry.activeEntryId).toBe(x);  // identity preserved
      expect(result.preserved).toContain(x);
      expect(result.minted).toEqual([]);
      expect(result.retired).toEqual([]);
    });

    test('different fileId at same path: path-match wins, fileId refreshes', () => {
      const x = registry.upsert('foo.md', false, '42');
      registry.reconcileSnapshot([
        { path: 'foo.md', isDir: false, fileId: '99' },
      ]);
      expect(registry.getByPath('foo.md')).toBe(x);  // path-based preserve
      expect(registry.getById(x)?.fileId).toBe('99');  // inode updated
    });

    test('null fileId does NOT trigger reuse', () => {
      const x = registry.upsert('old.md', false, null);
      const result = registry.reconcileSnapshot([
        { path: 'new.md', isDir: false, fileId: null },
      ]);
      expect(registry.getByPath('old.md')).toBeNull();
      expect(registry.getByPath('new.md')).not.toBeNull();
      expect(registry.getByPath('new.md')).not.toBe(x);
      expect(result.retired).toContain(x);
      expect(result.minted).toHaveLength(1);
    });

    test('hardlink-like case: two snapshot entries share fileId, second mints fresh', () => {
      // /a/file already exists at fileId '42'. New snapshot contains BOTH
      // /a/file (still) and /b/file (new) with the same fileId '42'.
      //
      // Pass 1 iterates entries in order:
      //  1. /a/file: byPath match → preserve A's EntryId.
      //  2. /b/file: byPath miss → byFileId.get('42') maps to A → A's path
      //     is also in the snapshot, so we DON'T move (would corrupt the
      //     path-preserved entry). Fall through to mint fresh for /b/file.
      const a = registry.upsert('a/file', false, '42');

      const result = registry.reconcileSnapshot([
        { path: 'a/file', isDir: false, fileId: '42' },
        { path: 'b/file', isDir: false, fileId: '42' },
      ]);

      expect(registry.getByPath('a/file')).toBe(a);  // path-preserved
      const b = registry.getByPath('b/file');
      expect(b).not.toBeNull();
      expect(b).not.toBe(a);  // fresh mint (NOT the byFileId move)
      expect(result.minted).toContain(b!);
    });

    test('byFileId is cleaned on delete()', () => {
      const id = registry.upsert('foo.md', false, '42');
      expect(registry.getByPath('foo.md')).toBe(id);
      registry.delete('foo.md');
      // After delete, a subsequent reconcile with same fileId should NOT reuse.
      const result = registry.reconcileSnapshot([
        { path: 'foo.md', isDir: false, fileId: '42' },
      ]);
      expect(result.minted).toHaveLength(1);
      expect(registry.getByPath('foo.md')).not.toBe(id);
    });

    test('byFileId is cleaned on reconcileSnapshot Pass 2 retirement', () => {
      const id = registry.upsert('foo.md', false, '42');
      // Retire 'foo.md' via reconcile with empty snapshot.
      registry.reconcileSnapshot([], { pathPrefix: '' });
      // byFileId should no longer have '42'. Verify by adding a NEW path
      // with the same fileId — should mint, not reuse.
      const result = registry.reconcileSnapshot([
        { path: 'bar.md', isDir: false, fileId: '42' },
      ]);
      expect(result.minted).toHaveLength(1);
      expect(registry.getByPath('bar.md')).not.toBe(id);
    });

    test('rename() cleans byFileId of orphaned destExisting record', () => {
      // Setup: two entries each with their own fileId.
      //   eOld:  path='other.md', fileId='99'
      //   eDest: path='dest.md',  fileId='42'
      // Now rename other.md → dest.md. This overwrites the dest.md entry,
      // and rename's defensive cleanup should remove byFileId['42'].
      const eOld = registry.upsert('other.md', false, '99');
      const eDest = registry.upsert('dest.md', false, '42');
      expect(eOld).not.toBe(eDest);

      registry.rename('other.md', 'dest.md');

      // eOld survives at dest.md
      expect(registry.getByPath('dest.md')).toBe(eOld);
      // eDest is gone from byId
      expect(registry.getById(eDest)).toBeNull();
      // byFileId['42'] (which used to point at eDest) is cleaned. Verify
      // via reconcile: a new entry with fileId='42' should mint, NOT reuse.
      const result = registry.reconcileSnapshot([
        { path: 'dest.md', isDir: false, fileId: '42' },   // path-match wins
        { path: 'fresh.md', isDir: false, fileId: '42' },   // should NOT find stale eDest
      ]);
      // The fresh.md entry can't reuse eDest (it's gone). Since dest.md
      // also has fileId='42' (path-preserved), Pass 1's path-match
      // refreshes eOld's fileId to '42' and byFileId['42'] = eOld.
      // For fresh.md: byPath miss; byFileId.get('42') = eOld, whose path
      // (dest.md) IS in snapshot, so guard fires → mint fresh.
      expect(result.minted).toHaveLength(1);
      expect(registry.getByPath('fresh.md')).not.toBe(eOld);
    });

    test('folder rename: directory file_id also preserves EntryId across rename', () => {
      // Folders have file_ids too (TreeNode.file_id includes both files and
      // directories, populated by extract_file_id on directory metadata).
      // Verify rename-via-reconcileSnapshot preserves a folder's EntryId.
      const dirId = registry.upsert('old-folder', true, '1000');
      // External rename of the directory: path changes, inode stable.
      const result = registry.reconcileSnapshot([
        { path: 'new-folder', isDir: true, fileId: '1000' },
      ]);
      expect(registry.getByPath('new-folder')).toBe(dirId);
      expect(registry.getByPath('old-folder')).toBeNull();
      expect(registry.getById(dirId)?.isDir).toBe(true);
      expect(result.preserved).toContain(dirId);
    });

    test('guardRelative: upsert with absolute path throws', () => {
      // All callers pass relative paths.
      // guardRelative now enforces (throws) rather than diagnosing (warns),
      // so an absolute path reaching the registry is a programming error.
      expect(() => {
        registry.upsert('/Users/me/Sites/blog/post.md', false);
      }).toThrow('absolute path');
    });

    test('guardRelative: relative path does NOT throw', () => {
      // All three mutating ops accept relative paths without error.
      expect(() => {
        registry.upsert('works/post.md', false);
        registry.rename('works/post.md', 'works/renamed.md');
        registry.delete('works/renamed.md');
      }).not.toThrow();
    });

    test('mtime-OR-path collision guard is NOT implemented (documented limitation)', () => {
      // Zed's `reuse_entry_id` (worktree.rs:3035) uses mtime as a tiebreaker
      // to avoid misidentifying delete-then-create as rename when the OS
      // recycles inodes. moss v1 does NOT implement this guard.
      //
      // Cross-reconcile safety: when 'a.md' is deleted via registry.delete(),
      // byFileId['42'] is cleaned, so the next reconcile with a NEW file
      // at the recycled inode mints fresh. Test asserts this safe path.
      const a = registry.upsert('a.md', false, '42');
      registry.delete('a.md');  // byFileId['42'] cleaned

      const result = registry.reconcileSnapshot([
        { path: 'b.md', isDir: false, fileId: '42' },
      ]);
      expect(registry.getByPath('b.md')).not.toBe(a);
      expect(result.minted).toHaveLength(1);

      // The UNSAFE path (within-single-reconcile inode recycling) is not
      // tested here — it requires the entry to still be in byFileId during
      // Pass 1. Accepted as v1 limitation; future PR may add mtime guard.
    });
  });
});
