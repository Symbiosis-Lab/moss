/**
 * Home detection is CONSUMED, not derived.
 *
 * The backend emits the per-folder home election as `TreeNode.is_home`
 * (marker › index stems › self-named note › first doc alphabetically, via
 * `moss_core::home::detect_home_file_in_folder_marked` with the folder's REAL
 * basename — so the project root works too). flattenTree carries the flag
 * onto the registry record, and `homeEntry` (tree/render.ts) reads
 * `children(folder).find(e => e.isHome)` — no filename re-derivation, no
 * entries[0] positional assumption. This replaced the TS `homeRank` mirror,
 * which drifted on the root basename (the original Part-1 parity bug this
 * file was born from) and never knew the `home: true` marker or the
 * alphabetical fallback.
 *
 * Election-rule coverage lives in Rust (editor::filesystem `is_home_*` tests
 * + moss_core::home). These tests exercise the private predicate
 * directly by seeding the registry and the controller's `dirIndex` (the exact
 * inputs the predicate reads), so they're independent of the mount/render
 * machinery.
 */

import { describe, test, expect, vi } from 'vitest';

import { FileTree } from '../controller';
import { EntryRegistry, type EntryId } from '../../entry-registry';

type Kid = { path: string; isDir?: boolean; isHome?: boolean };

/** Private surface of FileTree the predicate is reached through: the
 *  renderer owns `homeEntry` (tree/render.ts); the controller owns its inputs. */
type Private = {
  projectRoot: string;
  dirIndex: Map<EntryId, EntryId[]>;
  render: { homeEntry(folderPath: string): { path: string } | null };
};

/** Seed folders and their children the way flattenTree does: registry first, then ids into dirIndex. */
function seed(projectRoot: string, tree: Record<string, Kid[]>): Private {
  const registry = new EntryRegistry();
  const ft = new FileTree(registry) as unknown as Private;
  ft.projectRoot = projectRoot;
  const entries = Object.entries(tree).flatMap(([dir, kids]) => [
    { path: dir, isDir: true, fileId: null },
    ...kids.map(k => ({ path: k.path, isDir: k.isDir ?? false, fileId: null, isHome: k.isHome ?? false })),
  ]);
  registry.reconcileSnapshot(entries, { pathPrefix: '' });
  for (const [dir, kids] of Object.entries(tree)) {
    ft.dirIndex.set(registry.getByPath(dir)!, kids.map(k => registry.getByPath(k.path)!));
  }
  return ft;
}

describe('FileTree home detection — consumes the backend is_home election', () => {
  test('self-named root home (William Blake.md) flagged by the backend is detected', () => {
    // Regression for the original parity bug: the ROOT's home. The election
    // now happens backend-side with the real root basename; the frontend just
    // reads the flag.
    const ft = seed('/sites/William Blake', {
      '': [{ path: 'William Blake.md', isHome: true }, { path: 'poems', isDir: true }],
    });
    expect(ft.render.homeEntry('')?.path).toBe('William Blake.md');
  });

  test('index.md home at the root', () => {
    const ft = seed('/sites/my-blog', {
      '': [{ path: 'index.md', isHome: true }, { path: 'posts', isDir: true }],
    });
    expect(ft.render.homeEntry('')?.path).toBe('index.md');
  });

  test('flagged home is found even when it is NOT the first entry (no entries[0] assumption)', () => {
    // The backend sorts the elected home first, but the frontend must not
    // depend on position — a marker-elected home (`home: true` on welcome.md)
    // proves the flag, not the filename or the index, decides.
    const ft = seed('/sites/my-blog', {
      '': [{ path: 'index.md' }, { path: 'welcome.md', isHome: true }],
    });
    expect(ft.render.homeEntry('')?.path).toBe('welcome.md');
  });

  test('alpha-fallback home: a folder whose only doc is about.md HAS a home', () => {
    // Intentional behavior change vs. the old filename mirror: the election's
    // priority-5 alphabetical fallback means the build serves about.md at the
    // folder URL, so the tree agrees — the "Create home file" ctx item
    // (gated on homeEntry() === null) correctly hides, and
    // dispatchFolderSurface opens about.md's surface.
    const ft = seed('/sites/my-blog', {
      notes: [{ path: 'notes/about.md', isHome: true }],
    });
    expect(ft.render.homeEntry('notes')?.path).toBe('notes/about.md');
  });

  test('home-less folder returns false / null (backend flagged nothing)', () => {
    const ft = seed('/sites/my-blog', {
      assets: [{ path: 'assets/photo.png' }, { path: 'assets/style.css' }],
    });
    expect(ft.render.homeEntry('assets')).toBeNull();
  });

  test('a directory entry is never the home, even if flagged by mistake', () => {
    // Defensive: the backend never flags directories, but the consumer
    // filters on !isDir so a corrupt payload cannot open a folder as a file.
    const ft = seed('/sites/my-blog', {
      '': [{ path: 'docs', isDir: true, isHome: true }],
    });
    expect(ft.render.homeEntry('')).toBeNull();
  });

  test('non-root self-named home (Writings/Writings.md) via the flag', () => {
    const ft = seed('/sites/my-blog', {
      '': [{ path: 'Writings', isDir: true }],
      Writings: [{ path: 'Writings/Writings.md', isHome: true }, { path: 'Writings/draft.md' }],
    });
    expect(ft.render.homeEntry('Writings')?.path).toBe('Writings/Writings.md');
  });

  test('empty / missing dirIndex entry returns false / null', () => {
    const ft = seed('/sites/my-blog', { '': [] });
    expect(ft.render.homeEntry('')).toBeNull();
    expect(ft.render.homeEntry('nope')).toBeNull();
  });
});
