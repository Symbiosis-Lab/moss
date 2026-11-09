import { describe, test, expect } from 'vitest';
import {
  applyOverride, buildActivePath, remapActivePathForRename, type Overrides,
  initialView, collapseAll, openFolders, setFolder, prune, forgetSubtree, anchorAt, withAnchor, parkAt, reanchorAfterMove, type TreeView,
} from '../model';
import type { EntryId } from '../../entry-registry';

describe('model', () => {
  describe('buildActivePath', () => {
    // buildActivePath
    // operates in the registry domain (project-root-relative). The root
    // segment is the empty string '' (PROJECT_ROOT_REL).

    test('root-level file', () => {
      const path = buildActivePath('index.md');
      expect(path).toEqual(['', 'index.md']);
    });

    test('nested file', () => {
      const path = buildActivePath('posts/2024/hello-world.md');
      expect(path).toEqual([
        '',
        'posts',
        'posts/2024',
        'posts/2024/hello-world.md',
      ]);
    });

    test('empty filePath returns just root', () => {
      const path = buildActivePath('');
      expect(path).toEqual(['']);
    });
  });

  describe('remapActivePathForRename', () => {
    // Keeps the breadcrumb VALID after a rename/move without re-anchoring it to
    // an unrelated open file. Rebuilds only when the anchor LEAF is affected
    // (it is the renamed path, or a descendant of a renamed folder); otherwise
    // returns the same array (the anchor stays frozen — stable layout).

    test('anchor leaf renamed in place → leaf segment updated, location kept', () => {
      const out = remapActivePathForRename(
        ['', 'posts', 'posts/old.md'], 'posts/old.md', 'posts/new.md',
      );
      expect(out).toEqual(['', 'posts', 'posts/new.md']);
    });

    test('anchor leaf MOVED to a different folder → breadcrumb re-rooted', () => {
      const out = remapActivePathForRename(
        ['', 'posts', 'posts/a.md'], 'posts/a.md', 'archive/a.md',
      );
      expect(out).toEqual(['', 'archive', 'archive/a.md']);
    });

    test('renamed folder is an anchor ANCESTOR → whole spine remapped', () => {
      const out = remapActivePathForRename(
        ['', 'posts', 'posts/a.md'], 'posts', 'articles',
      );
      expect(out).toEqual(['', 'articles', 'articles/a.md']);
    });

    test('renamed file is OFF the anchor path → returned UNCHANGED (same reference, frozen)', () => {
      const anchor = ['', 'posts', 'posts/a.md'];
      const out = remapActivePathForRename(anchor, 'notes/b.md', 'notes/c.md');
      expect(out).toBe(anchor); // identity: not rebuilt
    });

    test('sibling-prefix is NOT a false match (post vs posts)', () => {
      const anchor = ['', 'posts', 'posts/article.md'];
      const out = remapActivePathForRename(anchor, 'post', 'article');
      expect(out).toBe(anchor); // 'posts/...' does not start with 'post/'
    });

    test('empty activePath → unchanged', () => {
      const anchor: string[] = [];
      expect(remapActivePathForRename(anchor, 'a.md', 'b.md')).toBe(anchor);
    });
  });

  // The backend emits the home election as
  // TreeNode.is_home and the tree consumes the flag. Election coverage lives
  // in Rust (editor::filesystem is_home_* tests + moss_core::home); the
  // frontend CONSUMPTION path is covered by file-tree-home-parity.test.ts.
});

describe('applyOverride', () => {
  const posts = 7 as EntryId;

  test('opens a folder by setting overrides[id] = "open"', () => {
    const before: Overrides = new Map();
    const after = applyOverride(before, posts, 'open');
    expect(after.get(posts)).toBe('open');
    expect(before.size).toBe(0);
  });

  test('closes a folder by setting overrides[id] = "closed"', () => {
    const before: Overrides = new Map([[posts, 'open']]);
    const after = applyOverride(before, posts, 'closed');
    expect(after.get(posts)).toBe('closed');
  });

  test('clears an override with intent = "default"', () => {
    const before: Overrides = new Map([[posts, 'open']]);
    const after = applyOverride(before, posts, 'default');
    expect(after.has(posts)).toBe(false);
  });
});

describe('TreeView transitions', () => {
  const root = 1 as EntryId;
  const posts = 7 as EntryId;
  const drafts = 9 as EntryId;

  test('initialView: empty forest, no park, the anchor the caller names', () => {
    expect(initialView()).toEqual({ anchor: [], overrides: new Map(), park: null });
    expect(initialView(['']).anchor).toEqual(['']);
  });

  test('collapseAll clears the forest and the park but keeps the anchor', () => {
    const before: TreeView = { anchor: ['', 'posts'], overrides: new Map([[posts, 'open']]), park: 120 };
    const after = collapseAll(before);
    expect(after).toEqual({ anchor: ['', 'posts'], overrides: new Map(), park: null });
    expect(after.anchor).toBe(before.anchor);
    expect(before.overrides.size).toBe(1);
  });

  test('openFolders opens every id, including one marked closed, and leaves the rest', () => {
    const before: TreeView = { anchor: [''], overrides: new Map([[posts, 'closed']]), park: 60 };
    const after = openFolders(before, [root, posts]);
    expect(after.overrides).toEqual(new Map([[posts, 'open'], [root, 'open']]));
    expect(after.park).toBe(60);
    expect(after.anchor).toBe(before.anchor);
    expect(before.overrides.get(posts)).toBe('closed');
  });

  test('openFolders returns the same view when nothing needed opening', () => {
    const before: TreeView = { anchor: [''], overrides: new Map([[root, 'open']]), park: null };
    expect(openFolders(before, [root])).toBe(before);
    expect(openFolders(before, [])).toBe(before);
  });

  test('setFolder applies one intent through applyOverride', () => {
    const before: TreeView = { anchor: [''], overrides: new Map([[root, 'open']]), park: null };
    expect(setFolder(before, root, 'default').overrides.has(root)).toBe(false);
    expect(setFolder(before, posts, 'open').overrides.get(posts)).toBe('open');
    expect(before.overrides).toEqual(new Map([[root, 'open']]));
  });

  test('prune keeps only the overrides the predicate admits', () => {
    const before: TreeView = {
      anchor: [''], overrides: new Map([[root, 'open'], [posts, 'open'], [drafts, 'closed']]), park: 80,
    };
    const after = prune(before, (id) => id !== posts);
    expect(after.overrides).toEqual(new Map([[root, 'open'], [drafts, 'closed']]));
    expect(after.park).toBe(80);
    expect(before.overrides.size).toBe(3);
  });

  test('forgetSubtree drops every id in the gone set', () => {
    const before: TreeView = {
      anchor: [''], overrides: new Map([[root, 'open'], [posts, 'open'], [drafts, 'open']]), park: null,
    };
    const after = forgetSubtree(before, new Set([posts, drafts]));
    expect(after.overrides).toEqual(new Map([[root, 'open']]));
  });

  test('anchorAt re-points the anchor and touches nothing else', () => {
    const before: TreeView = { anchor: [''], overrides: new Map([[root, 'open']]), park: 42 };
    const after = anchorAt(before, 'posts/a.md');
    expect(after.anchor).toEqual(['', 'posts', 'posts/a.md']);
    expect(after.overrides).toBe(before.overrides);
    expect(after.park).toBe(42);
  });

  test('withAnchor takes the anchor as given; parkAt sets or clears the park; neither touches the forest', () => {
    const before: TreeView = { anchor: [''], overrides: new Map([[root, 'open']]), park: 42 };
    const anchored = withAnchor(before, ['', 'posts']);
    expect(anchored.anchor).toEqual(['', 'posts']);
    expect(anchored.overrides).toBe(before.overrides);
    expect(anchored.park).toBe(42);
    expect(parkAt(before, 120)).toEqual({ ...before, park: 120 });
    expect(parkAt(before, null)).toEqual({ ...before, park: null });
    expect(parkAt(before, null).overrides).toBe(before.overrides);
    expect(before.park).toBe(42);
  });

  test('reanchorAfterMove follows the anchor leaf or its ancestor to its real destination and stays frozen otherwise', () => {
    const before: TreeView = { anchor: ['', 'posts', 'posts/a.md'], overrides: new Map([[root, 'open']]), park: 42 };
    // The leaf itself moved.
    const leafMoved = reanchorAfterMove(before, [{ old_path: 'posts/a.md', new_path: 'archive/a.md' }]);
    expect(leafMoved.anchor).toEqual(['', 'archive', 'archive/a.md']);
    expect(leafMoved.overrides).toBe(before.overrides);
    expect(leafMoved.park).toBe(42);
    // An ancestor folder moved, to the project root.
    expect(reanchorAfterMove(before, [{ old_path: 'posts', new_path: 'posts' }]).anchor).toEqual(['', 'posts', 'posts/a.md']);
    expect(reanchorAfterMove(before, [{ old_path: 'posts', new_path: 'old/posts' }]).anchor).toEqual(['', 'old', 'old/posts', 'old/posts/a.md']);
    // A sibling moved: the anchor array is returned by reference (frozen).
    expect(
      reanchorAfterMove(before, [{ old_path: 'posts/b.md', new_path: 'archive/b.md' }, { old_path: 'notes', new_path: 'archive/notes' }]).anchor,
    ).toBe(before.anchor);
    // A destination collision-suffixed by the backend (name-2.md) is
    // followed exactly, not re-derived from a target dir + basename guess.
    const suffixed = reanchorAfterMove(before, [{ old_path: 'posts/a.md', new_path: 'archive/a-2.md' }]);
    expect(suffixed.anchor).toEqual(['', 'archive', 'archive/a-2.md']);
  });
});
