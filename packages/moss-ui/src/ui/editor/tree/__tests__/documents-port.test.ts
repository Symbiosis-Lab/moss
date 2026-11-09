/**
 * A rename, a move and an Undo of the open file go through the host's
 * documents: the pending save first, the open document told before and after
 * the write, and the write itself. A host whose own editor owns the text
 * (VS Code, Obsidian) supplies these; here a recording one stands in.
 */
import { describe, test, expect, vi, beforeEach, afterEach } from 'vitest';
import { installPointerEventPolyfill, dir, file, priv } from './test-helpers';

installPointerEventPolyfill();

const { mockListTree, mockGetProjectPath } = vi.hoisted(() => ({
  mockListTree: vi.fn(),
  mockGetProjectPath: vi.fn(),
}));

vi.mock('../../../engine', () => ({
  engine: {
    listTree: mockListTree,
    getProjectPath: mockGetProjectPath,
    resolveUrlForFile: vi.fn().mockResolvedValue({ status: 'ok', data: '/' }),
  },
}));

import { host, type Documents } from "../../../host";
import { FileTree } from '../controller';
import { EntryRegistry } from '../../entry-registry';
import { performRenameUndo } from '../rename-toast';

const ROOT = '/Users/me/Sites/Ada';
const summary = (moved: Array<{ old_path: string; new_path: string; is_dir: boolean }>) =>
  ({ moved, edited_link_count: 0, edited_pages: [], skipped_pages: [], undo_id: 'undo-1' });

let calls: string[];
const recording: Documents = {
  flush: async () => { calls.push('flush'); },
  rename: async (from, to) => {
    calls.push(`rename ${from} -> ${to}`);
    return { status: 'ok', data: summary([{ old_path: 'about.md', new_path: 'colophon.md', is_dir: false }]) };
  },
  move: async (paths, toDir) => {
    calls.push(`move ${paths.join(',')} -> ${toDir}`);
    return { status: 'ok', data: summary([{ old_path: 'about.md', new_path: 'posts/about.md', is_dir: false }]) };
  },
  retarget: (from, to, phase) => { calls.push(`retarget ${phase} ${from} -> ${to}`); },
};

let registry: EntryRegistry;
let tree: FileTree;
let container: HTMLElement;

beforeEach(async () => {
  calls = [];
  host.documents = recording;
  mockGetProjectPath.mockResolvedValue({ status: 'ok', data: ROOT });
  mockListTree.mockResolvedValue({ status: 'ok', data: dir(ROOT, [file(`${ROOT}/about.md`), dir(`${ROOT}/posts`, [])], { name: 'Ada' }) });
  registry = new EntryRegistry();
  tree = new FileTree(registry);
  container = document.createElement('div');
  document.body.appendChild(container);
  await tree.mount(container, ROOT, false);
  tree.highlight(`${ROOT}/about.md`);
});

afterEach(() => {
  tree.unmount();
  container.remove();
  delete host.documents;
});

describe("the host's documents", () => {
  test('renaming the open file flushes it, then retargets it around the rename', async () => {
    await tree.renameThroughTree(`${ROOT}/about.md`, `${ROOT}/colophon.md`);
    expect(calls).toEqual([
      'flush',
      'retarget starting about.md -> colophon.md',
      // An in-place rename is painted before the write returns, the open file with it.
      'retarget done about.md -> colophon.md',
      `rename ${ROOT}/about.md -> ${ROOT}/colophon.md`,
    ]);
  });

  test('moving the open file flushes it, then retargets it to where the engine put it', async () => {
    await priv<{ ops: { moveEntriesTo(paths: string[], to: string): Promise<unknown> } }>(tree).ops.moveEntriesTo(['about.md'], 'posts');
    expect(calls).toEqual([
      'flush',
      'retarget starting about.md -> posts/about.md',
      `move ${ROOT}/about.md -> ${ROOT}/posts`,
      'retarget done about.md -> posts/about.md',
    ]);
  });

  test("the toast's Undo flushes first and moves the open file back", async () => {
    const undo = vi.fn(async (_id: string) => { calls.push('undo'); return { status: 'ok' as const, data: { restored_files: [], skipped: [] } }; });
    registry.rename('about.md', 'posts/about.md');
    await performRenameUndo({
      registry: () => registry,
      documents: recording,
      broadcastRenameComplete: () => {},
      refresh: { refreshTree: async () => {} },
      reanchor: () => {},
      setEditorCommitLock: () => {},
      reloadActiveFileFromDisk: async () => {},
      undoRename: undo,
    }, summary([{ old_path: 'about.md', new_path: 'posts/about.md', is_dir: false }]));
    expect(undo).toHaveBeenCalledWith('undo-1');
    expect(calls).toEqual([
      'flush',
      'retarget starting posts/about.md -> about.md',
      'undo',
      'retarget done posts/about.md -> about.md',
    ]);
  });
});
