/**
 * Tests for the os-drag-in.ts module.
 *
 * Covers:
 * - createOsDragInController factory: instance-scoped highlight state,
 *   proper test isolation between independent controllers
 * - handleOsDragOver: hit-testing + highlight + spring-load
 * - handleOsDrop: asks the host to copy the files in, with the correct target dir,
 *   falls back to projectRoot when no folder under cursor
 * - clearOsDragHighlight: clears highlight + spring-load
 * - Legacy free-function exports: backward-compat delegation
 */

import { describe, test, expect, vi, beforeEach } from 'vitest';

// ---------------------------------------------------------------------------
// Hoisted mocks
// ---------------------------------------------------------------------------
const { mockShowToast, mockCopyFilesToProject, mockResolveAttachmentDir, mockOpenAssetDialog } = vi.hoisted(() => ({
  mockShowToast: vi.fn(),
  mockCopyFilesToProject: vi.fn(),
  mockResolveAttachmentDir: vi.fn(),
  mockOpenAssetDialog: vi.fn(),
}));


vi.mock('../../engine', () => ({ engine: { resolveAttachmentDir: mockResolveAttachmentDir } }));

vi.mock("../../shared/ui/toast/toast-manager", () => ({
  showToast: mockShowToast,
  showAck: vi.fn(),
}));

import { host } from "../../host";
host.copyIn = mockCopyFilesToProject;
host.pickFiles = mockOpenAssetDialog;

import { createOsDragInController, handleOsDragOver, handleOsDrop, clearOsDragHighlight, importIntoVault, pickMediaForBar } from '../os-drag-in';
import type { EntryId } from '../entry-registry';

// ---------------------------------------------------------------------------
// FileTree mock factory
// ---------------------------------------------------------------------------

function makeFileTree(opts: {
  projectRoot?: string;
  folderAtPosition?: EntryId | null;
  pathOfResult?: string | null;
} = {}) {
  const {
    projectRoot = '/projects/test-site',
    folderAtPosition = null,
    pathOfResult = null,
  } = opts;

  return {
    projectRootPath: projectRoot,
    getFolderAtPosition: vi.fn().mockReturnValue(folderAtPosition),
    pathOf: vi.fn().mockReturnValue(pathOfResult),
    setDropHighlight: vi.fn(),
    springLoadIfHoverIs: vi.fn(),
    refreshAndRender: vi.fn().mockResolvedValue(undefined),
  };
}

// ---------------------------------------------------------------------------
// createOsDragInController factory
// ---------------------------------------------------------------------------

describe('createOsDragInController', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  test('returns an object with handleOsDragOver, handleOsDrop, clearOsDragHighlight methods', () => {
    const tree = makeFileTree();
    const ctrl = createOsDragInController(tree as any);
    expect(typeof ctrl.handleOsDragOver).toBe('function');
    expect(typeof ctrl.handleOsDrop).toBe('function');
    expect(typeof ctrl.clearOsDragHighlight).toBe('function');
  });

  test('highlight state is isolated between two controllers on separate trees', () => {
    const folderId = 42 as EntryId;
    const treeA = makeFileTree({ folderAtPosition: folderId, pathOfResult: '/root/assets' });
    const treeB = makeFileTree({ folderAtPosition: null });

    const ctrlA = createOsDragInController(treeA as any);
    const ctrlB = createOsDragInController(treeB as any);

    // Hover over folderId in A — B should not be affected
    ctrlA.handleOsDragOver(100, 50);
    expect(treeA.setDropHighlight).toHaveBeenCalledWith(folderId);
    expect(treeB.setDropHighlight).not.toHaveBeenCalled();

    // Hovering A again never reaches B, whatever A does.
    ctrlA.handleOsDragOver(105, 50);
    expect(treeB.setDropHighlight).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// handleOsDragOver (controller method)
// ---------------------------------------------------------------------------

describe('OsDragInController.handleOsDragOver', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  test('calls getFolderAtPosition with the provided logical coordinates', () => {
    const tree = makeFileTree();
    const ctrl = createOsDragInController(tree as any);
    ctrl.handleOsDragOver(120, 80);
    expect(tree.getFolderAtPosition).toHaveBeenCalledWith(120, 80);
  });

  test('calls setDropHighlight with the folder id when one is found', () => {
    const folderId = 42 as EntryId;
    const tree = makeFileTree({ folderAtPosition: folderId, pathOfResult: '/projects/test-site/assets' });
    const ctrl = createOsDragInController(tree as any);
    ctrl.handleOsDragOver(100, 50);
    expect(tree.setDropHighlight).toHaveBeenCalledWith(folderId);
  });

  test('calls setDropHighlight(null) when no folder is under cursor', () => {
    const tree = makeFileTree({ folderAtPosition: null });
    const ctrl = createOsDragInController(tree as any);
    ctrl.handleOsDragOver(100, 50);
    expect(tree.setDropHighlight).toHaveBeenCalledWith(null);
  });

  test('calls springLoadIfHoverIs with folder path when folder is found', () => {
    const folderId = 7 as EntryId;
    const tree = makeFileTree({ folderAtPosition: folderId, pathOfResult: '/projects/test-site/notes' });
    const ctrl = createOsDragInController(tree as any);
    ctrl.handleOsDragOver(10, 20);
    expect(tree.springLoadIfHoverIs).toHaveBeenCalledWith('/projects/test-site/notes');
  });

  test('calls springLoadIfHoverIs(null) when no folder is under cursor', () => {
    const tree = makeFileTree({ folderAtPosition: null });
    const ctrl = createOsDragInController(tree as any);
    ctrl.handleOsDragOver(10, 20);
    expect(tree.springLoadIfHoverIs).toHaveBeenCalledWith(null);
  });

  test('re-asks for the same highlight rather than remembering it', () => {
    // The controller keeps no memo of what it highlighted last. It could
    // disagree with the tree — anything that cleared the tree's feedback left
    // the memo naming a folder whose class was already gone, and the next
    // hover over that folder then skipped the repaint. setDropHighlight is a
    // clear-then-set, so asking twice is free and cannot drift.
    const folderId = 3 as EntryId;
    const tree = makeFileTree({ folderAtPosition: folderId, pathOfResult: '/projects/test-site/assets' });
    const ctrl = createOsDragInController(tree as any);
    ctrl.handleOsDragOver(100, 50);
    ctrl.handleOsDragOver(105, 50);
    expect(tree.setDropHighlight).toHaveBeenNthCalledWith(2, folderId);
  });
});

// ---------------------------------------------------------------------------
// handleOsDrop (controller method)
// ---------------------------------------------------------------------------

describe('OsDragInController.handleOsDrop', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  test('calls copyFilesToProject with the folder path when folder is under cursor', async () => {
    const folderId = 5 as EntryId;
    const folderPath = '/projects/test-site/drafts';
    mockCopyFilesToProject.mockResolvedValue({ status: 'ok', data: { copied: [], skipped: [] } });

    const tree = makeFileTree({ folderAtPosition: folderId, pathOfResult: folderPath });
    const ctrl = createOsDragInController(tree as any);
    await ctrl.handleOsDrop(['/tmp/a.md', '/tmp/b.png'], 100, 60);

    expect(mockCopyFilesToProject).toHaveBeenCalledWith(
      ['/tmp/a.md', '/tmp/b.png'],
      folderPath,
    );
  });

  test('falls back to projectRoot when no folder is under cursor', async () => {
    const projectRoot = '/projects/test-site';
    mockCopyFilesToProject.mockResolvedValue({ status: 'ok', data: { copied: [], skipped: [] } });

    const tree = makeFileTree({ folderAtPosition: null, projectRoot });
    const ctrl = createOsDragInController(tree as any);
    await ctrl.handleOsDrop(['/tmp/photo.jpg'], 500, 300);

    expect(mockCopyFilesToProject).toHaveBeenCalledWith(
      ['/tmp/photo.jpg'],
      projectRoot,
    );
  });

  test('calls refreshAndRender after successful import', async () => {
    mockCopyFilesToProject.mockResolvedValue({ status: 'ok', data: { copied: [], skipped: [] } });

    const tree = makeFileTree({ folderAtPosition: null });
    const ctrl = createOsDragInController(tree as any);
    await ctrl.handleOsDrop(['/tmp/file.md'], 0, 0);

    expect(tree.refreshAndRender).toHaveBeenCalled();
  });

  test('does NOT call refreshAndRender on import error', async () => {
    mockCopyFilesToProject.mockResolvedValue({ status: 'error', error: 'Permission denied' });

    const tree = makeFileTree({ folderAtPosition: null });
    const ctrl = createOsDragInController(tree as any);
    await ctrl.handleOsDrop(['/tmp/file.md'], 0, 0);

    expect(tree.refreshAndRender).not.toHaveBeenCalled();
  });

  test('does nothing when paths array is empty', async () => {
    const tree = makeFileTree();
    const ctrl = createOsDragInController(tree as any);
    await ctrl.handleOsDrop([], 0, 0);

    expect(mockCopyFilesToProject).not.toHaveBeenCalled();
    expect(tree.refreshAndRender).not.toHaveBeenCalled();
  });

  test('does nothing when projectRoot is empty (no project open)', async () => {
    const tree = makeFileTree({ projectRoot: '', folderAtPosition: null });
    const ctrl = createOsDragInController(tree as any);
    await ctrl.handleOsDrop(['/tmp/file.md'], 0, 0);

    expect(mockCopyFilesToProject).not.toHaveBeenCalled();
  });

  test('shows a Blocking toast when copyFilesToProject returns an error', async () => {
    mockCopyFilesToProject.mockResolvedValue({ status: 'error', error: 'Permission denied' });

    const tree = makeFileTree({ folderAtPosition: null });
    const ctrl = createOsDragInController(tree as any);
    await ctrl.handleOsDrop(['/tmp/file.md'], 0, 0);

    expect(mockShowToast).toHaveBeenCalledWith(
      expect.objectContaining({ severity: 'Blocking' }),
    );
    expect(tree.refreshAndRender).not.toHaveBeenCalled();
  });

  test('a tree drop that skips a file says so, and still refreshes', async () => {
    // A `?` on the
    // first failure, so an evicted iCloud photo returned `Err` for the whole
    // gesture and the files that HAD landed went unmentioned. Now it reports
    // per file, through the same toast the editor drop uses.
    mockCopyFilesToProject.mockResolvedValue({
      status: 'ok',
      data: {
        copied: [{ original_name: 'a.jpg', final_name: 'a.jpg', final_path: '/p/a.jpg', relative_path: 'a.jpg' }],
        skipped: [{ original_name: 'b.jpg', reason: 'still_in_cloud' }],
      },
    });
    const tree = makeFileTree({ folderAtPosition: null });
    const ctrl = createOsDragInController(tree as any);

    await ctrl.handleOsDrop(['/tmp/a.jpg', '/tmp/b.jpg'], 0, 0);

    expect(mockShowToast).toHaveBeenCalledWith(
      expect.objectContaining({ severity: 'NeedsAction' }),
    );
    expect(tree.refreshAndRender).toHaveBeenCalled();
  });

  test('clears the drop highlight before dispatching import', async () => {
    mockCopyFilesToProject.mockResolvedValue({ status: 'ok', data: { copied: [], skipped: [] } });
    const tree = makeFileTree({ folderAtPosition: null });
    const ctrl = createOsDragInController(tree as any);

    // Verify setDropHighlight is called before copyFilesToProject resolves
    const calls: string[] = [];
    tree.setDropHighlight.mockImplementation(() => calls.push('setDropHighlight'));
    mockCopyFilesToProject.mockImplementation(() => {
      calls.push('copyFilesToProject');
      return Promise.resolve({ status: 'ok', data: { copied: [], skipped: [] } });
    });

    await ctrl.handleOsDrop(['/tmp/file.md'], 0, 0);

    expect(calls[0]).toBe('setDropHighlight');
    expect(calls[1]).toBe('copyFilesToProject');
  });
});

// ---------------------------------------------------------------------------
// clearOsDragHighlight (controller method)
// ---------------------------------------------------------------------------

describe('OsDragInController.clearOsDragHighlight', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  test('calls setDropHighlight(null)', () => {
    const tree = makeFileTree();
    const ctrl = createOsDragInController(tree as any);
    ctrl.clearOsDragHighlight();
    expect(tree.setDropHighlight).toHaveBeenCalledWith(null);
  });

  test('calls springLoadIfHoverIs(null) to cancel spring-load', () => {
    const tree = makeFileTree();
    const ctrl = createOsDragInController(tree as any);
    ctrl.clearOsDragHighlight();
    expect(tree.springLoadIfHoverIs).toHaveBeenCalledWith(null);
  });
});

// ---------------------------------------------------------------------------
// File-row → parent-folder resolution (via getFolderAtPosition returning
// the parent folder's EntryId when hovering a file row in the file tree).
// os-drag-in trusts what getFolderAtPosition returns — these tests verify
// that the controller uses the returned id correctly, not null-fallback.
// ---------------------------------------------------------------------------

describe('handleOsDragOver with parent-folder id from file-row hit', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  test('calls setDropHighlight with the parent folder id when getFolderAtPosition returns parent (file-row hover)', () => {
    // Simulate hovering a file row inside 'docs' → getFolderAtPosition returns the
    // 'docs' folder EntryId (parent), not null. The controller must highlight
    // the parent folder, not skip the highlight.
    const parentFolderId = 10 as EntryId;
    const parentFolderPath = '/projects/test-site/docs';
    const tree = makeFileTree({ folderAtPosition: parentFolderId, pathOfResult: parentFolderPath });
    const ctrl = createOsDragInController(tree as any);

    ctrl.handleOsDragOver(100, 60);

    expect(tree.getFolderAtPosition).toHaveBeenCalledWith(100, 60);
    expect(tree.setDropHighlight).toHaveBeenCalledWith(parentFolderId);
    expect(tree.springLoadIfHoverIs).toHaveBeenCalledWith(parentFolderPath);
  });

  test('calls springLoadIfHoverIs with parent folder path when file-row resolves to parent', () => {
    const parentFolderId = 3 as EntryId;
    const parentFolderPath = '/projects/test-site/posts';
    const tree = makeFileTree({ folderAtPosition: parentFolderId, pathOfResult: parentFolderPath });
    const ctrl = createOsDragInController(tree as any);

    ctrl.handleOsDragOver(50, 80);

    expect(tree.springLoadIfHoverIs).toHaveBeenCalledWith(parentFolderPath);
  });
});

describe('handleOsDrop with parent-folder id from file-row hit', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  test('calls copyFilesToProject with parent folder path when hovering a file row (not project root)', async () => {
    // Hovering a file row inside 'docs' answers the 'docs' folder EntryId from
    // getFolderAtPosition, and the controller must copy into that parent
    // folder, not fall back to the project root.
    const parentFolderId = 10 as EntryId;
    const parentFolderAbsPath = '/projects/test-site/docs';
    mockCopyFilesToProject.mockResolvedValue({ status: 'ok', data: { copied: [], skipped: [] } });

    const tree = makeFileTree({
      folderAtPosition: parentFolderId,
      pathOfResult: parentFolderAbsPath,
      projectRoot: '/projects/test-site',
    });
    const ctrl = createOsDragInController(tree as any);
    await ctrl.handleOsDrop(['/tmp/image.png'], 100, 60);

    // Must use the parent folder path, NOT the project root
    expect(mockCopyFilesToProject).toHaveBeenCalledWith(
      ['/tmp/image.png'],
      parentFolderAbsPath,
    );
    expect(mockCopyFilesToProject).not.toHaveBeenCalledWith(
      expect.anything(),
      '/projects/test-site',
    );
  });

  test('for root-level file hover: getFolderAtPosition returns root EntryId → drops to project root path', async () => {
    // When hovering 'readme.md' (root-level file), getFolderAtPosition resolves
    // to the project root EntryId, and pathOf returns the project root path.
    // The result should be dropping to the project root — same as no-folder fallback
    // but arrived at via the parent-resolution path rather than null.
    const rootFolderId = 1 as EntryId;
    const projectRoot = '/projects/test-site';
    mockCopyFilesToProject.mockResolvedValue({ status: 'ok', data: { copied: [], skipped: [] } });

    const tree = makeFileTree({
      folderAtPosition: rootFolderId,
      pathOfResult: projectRoot,
      projectRoot,
    });
    const ctrl = createOsDragInController(tree as any);
    await ctrl.handleOsDrop(['/tmp/photo.jpg'], 50, 200);

    expect(mockCopyFilesToProject).toHaveBeenCalledWith(
      ['/tmp/photo.jpg'],
      projectRoot,
    );
  });
});

// ---------------------------------------------------------------------------
// Registry relative-path migration: `pathOf` returns a project-root-RELATIVE
// path (per entry-registry.ts).
// The `copyFilesToProject` Rust command resolves its target against
// std::path (process CWD), so the folder-drop case MUST convert to an absolute
// path at the FS boundary. The bug was that it passed the bare relative name
// ("Writings"), which Rust rejected as "Target is not a directory".
// ---------------------------------------------------------------------------

describe('handleOsDrop converts a relative folder path to absolute (registry migration)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  test('folder drop: relative pathOf → absolute targetDir passed to copyFilesToProject', async () => {
    const folderId = 5 as EntryId;
    mockCopyFilesToProject.mockResolvedValue({ status: 'ok', data: { copied: [], skipped: [] } });

    // pathOf returns a project-root-RELATIVE path (the real registry contract),
    // NOT an absolute one — this is the shape the migration left behind.
    const tree = makeFileTree({
      folderAtPosition: folderId,
      pathOfResult: 'Writings',
      projectRoot: '/projects/test-site',
    });
    const ctrl = createOsDragInController(tree as any);
    await ctrl.handleOsDrop(['/tmp/a.md'], 100, 60);

    // Must be the ABSOLUTE path, not the bare relative name.
    expect(mockCopyFilesToProject).toHaveBeenCalledWith(
      ['/tmp/a.md'],
      '/projects/test-site/Writings',
    );
    expect(mockCopyFilesToProject).not.toHaveBeenCalledWith(
      expect.anything(),
      'Writings',
    );
  });

  test('nested relative folder path is joined onto the project root', async () => {
    const folderId = 8 as EntryId;
    mockCopyFilesToProject.mockResolvedValue({ status: 'ok', data: { copied: [], skipped: [] } });
    const tree = makeFileTree({
      folderAtPosition: folderId,
      pathOfResult: 'notes/2026',
      projectRoot: '/projects/test-site',
    });
    const ctrl = createOsDragInController(tree as any);
    await ctrl.handleOsDrop(['/tmp/x.png'], 40, 90);

    expect(mockCopyFilesToProject).toHaveBeenCalledWith(
      ['/tmp/x.png'],
      '/projects/test-site/notes/2026',
    );
  });

  test('root-level relative path (empty string) resolves to the project root', async () => {
    // Hovering a root-level file → getFolderAtPosition returns the root
    // EntryId → pathOf returns PROJECT_ROOT_REL (''). Must resolve to root.
    const rootId = 1 as EntryId;
    mockCopyFilesToProject.mockResolvedValue({ status: 'ok', data: { copied: [], skipped: [] } });
    const tree = makeFileTree({
      folderAtPosition: rootId,
      pathOfResult: '',
      projectRoot: '/projects/test-site',
    });
    const ctrl = createOsDragInController(tree as any);
    await ctrl.handleOsDrop(['/tmp/readme.md'], 20, 20);

    // toAbsolute('', root) → `${root}/`. Rust's is_dir accepts the trailing slash.
    expect(mockCopyFilesToProject).toHaveBeenCalledWith(
      ['/tmp/readme.md'],
      '/projects/test-site/',
    );
  });
});

// ---------------------------------------------------------------------------
// Legacy free-function exports (backward-compat)
// ---------------------------------------------------------------------------

describe('legacy free-function exports', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  test('handleOsDragOver(fileTree, x, y) delegates to a controller', () => {
    const folderId = 99 as EntryId;
    const tree = makeFileTree({ folderAtPosition: folderId, pathOfResult: '/root/pages' });
    handleOsDragOver(tree as any, 100, 50);
    expect(tree.getFolderAtPosition).toHaveBeenCalledWith(100, 50);
    // First call with a folder hit should call setDropHighlight with that id
    expect(tree.setDropHighlight).toHaveBeenCalledWith(folderId);
  });

  test('handleOsDrop(fileTree, paths, x, y) delegates to a controller', async () => {
    mockCopyFilesToProject.mockResolvedValue({ status: 'ok', data: { copied: [], skipped: [] } });
    const tree = makeFileTree({ folderAtPosition: null, projectRoot: '/root' });
    await handleOsDrop(tree as any, ['/tmp/file.md'], 0, 0);
    expect(mockCopyFilesToProject).toHaveBeenCalledWith(['/tmp/file.md'], '/root');
  });

  test('clearOsDragHighlight(fileTree) delegates to a controller', () => {
    const tree = makeFileTree();
    clearOsDragHighlight(tree as any);
    expect(tree.setDropHighlight).toHaveBeenCalledWith(null);
    expect(tree.springLoadIfHoverIs).toHaveBeenCalledWith(null);
  });
});

// ---------------------------------------------------------------------------
// importIntoVault — the backend reports what it did NOT land
// ---------------------------------------------------------------------------

describe('importIntoVault reports files that did not land', () => {
  const ROOT = '/projects/test-site';

  beforeEach(() => {
    vi.clearAllMocks();
    mockResolveAttachmentDir.mockResolvedValue({ status: 'ok', data: 'posts' });
  });

  function copyReturns(copied: unknown[], skipped: unknown[]) {
    mockCopyFilesToProject.mockResolvedValue({ status: 'ok', data: { copied, skipped } });
  }

  const landedPhoto = {
    original_name: 'photo.jpg', final_name: 'photo.jpg',
    final_path: `${ROOT}/posts/photo.jpg`, relative_path: 'posts/photo.jpg',
  };

  test('an ok result that landed nothing is a failure, not a silent success', async () => {
    copyReturns([], [{ original_name: '可愛！.jpg', reason: 'failed' }]);

    const landed = await importIntoVault(['/Users/me/Desktop/可愛！.jpg'], 'posts/hello.md', ROOT);

    expect(landed).toBeNull();
    expect(mockShowToast).toHaveBeenCalledTimes(1);
    expect(mockShowToast.mock.calls[0][0]).toMatchObject({ severity: 'Blocking' });
    expect(mockShowToast.mock.calls[0][0].what).toContain('可愛！.jpg');
  });

  test('a cloud-evicted source says it is still downloading, not that it failed', async () => {
    copyReturns([], [{ original_name: '可愛！.jpg', reason: 'still_in_cloud' }]);

    const landed = await importIntoVault(['/Users/me/Desktop/可愛！.jpg'], 'posts/hello.md', ROOT);

    expect(landed).toBeNull();
    const advisory = mockShowToast.mock.calls[0][0];
    // Not Blocking: moss has asked the provider for the bytes, so the file is
    // coming — the user's move is to try again, not to go looking for a
    // broken file.
    expect(advisory.severity).toBe('NeedsAction');
    expect(advisory.what).toContain('可愛！.jpg');
    expect(advisory.what.toLowerCase()).toContain('downloading');
  });

  test('a partial import lands the good file AND names the one that stalled', async () => {
    copyReturns([landedPhoto], [{ original_name: 'big.jpg', reason: 'still_in_cloud' }]);

    const landed = await importIntoVault(
      ['/Users/me/Desktop/photo.jpg', '/Users/me/Desktop/big.jpg'], 'posts/hello.md', ROOT,
    );

    expect(landed).toEqual([{ dir: 'posts', finalName: 'photo.jpg' }]);
    expect(mockShowToast).toHaveBeenCalledTimes(1);
    expect(mockShowToast.mock.calls[0][0].what).toContain('big.jpg');
  });

  test('a clean import says nothing', async () => {
    copyReturns([landedPhoto], []);

    const landed = await importIntoVault(['/Users/me/Desktop/photo.jpg'], 'posts/hello.md', ROOT);

    expect(landed).toEqual([{ dir: 'posts', finalName: 'photo.jpg' }]);
    expect(mockShowToast).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// pickMediaForBar — the insert bar writes a Media embed as the path from the
// page, so it needs where each file landed, not just its name.
// ---------------------------------------------------------------------------

describe('pickMediaForBar returns project-relative paths', () => {
  const ROOT = '/projects/test-site';
  const ctx = { currentFilePath: () => 'posts/hello.md', projectRoot: () => ROOT };

  beforeEach(() => {
    vi.clearAllMocks();
    mockResolveAttachmentDir.mockResolvedValue({ status: 'ok', data: 'assets' });
  });

  test('a pick from outside the folder is copied in: the attachment folder for an image, the page folder for anything else', async () => {
    mockOpenAssetDialog.mockResolvedValue(['/Users/me/a b.jpg', '/Users/me/clip.mp4']);
    mockCopyFilesToProject
      .mockResolvedValueOnce({ status: 'ok', data: { copied: [{ original_name: 'a b.jpg', final_name: 'a b-2.jpg', final_path: '', relative_path: '' }], skipped: [] } })
      .mockResolvedValueOnce({ status: 'ok', data: { copied: [{ original_name: 'clip.mp4', final_name: 'clip.mp4', final_path: '', relative_path: '' }], skipped: [] } });

    expect(await pickMediaForBar('media', ctx)).toEqual(['assets/a b-2.jpg', 'posts/clip.mp4']);
  });

  test('a pick inside a folder whose name a macOS dialog decomposes is still embedded by its relative path', async () => {
    const cafe = { currentFilePath: () => 'posts/hello.md', projectRoot: () => '/Users/me/Caf\u00e9' };
    mockOpenAssetDialog.mockResolvedValue(['/Users/me/Cafe\u0301/assets/a.jpg']);
    expect(await pickMediaForBar('media', cafe)).toEqual(['assets/a.jpg']);
    expect(mockCopyFilesToProject).not.toHaveBeenCalled();
  });

  test('a pick already inside the folder is embedded where it is, as an uploading host lands it', async () => {
    mockOpenAssetDialog.mockResolvedValue([`${ROOT}/assets/a.jpg`, `${ROOT}/posts/clip.mp4`]);

    expect(await pickMediaForBar('media', ctx)).toEqual(['assets/a.jpg', 'posts/clip.mp4']);
    expect(mockCopyFilesToProject).not.toHaveBeenCalled();
  });
});
