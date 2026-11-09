/**
 * Tests for keep-last-good dirIndex, on the
 * refresh cycle in tree/refresh.ts, driven through the mounted controller
 * because the retry path ends in a repaint the controller owns.
 *
 * The refresh contract:
 * "refreshTree() builds into staging maps and swaps atomically only on
 * success. On failure or non-resolution (bounded timeout on the IPC await),
 * keep serving the previous index, log a WARN with the cause, and schedule a
 * bounded retry. The index a gesture reads is never mid-clear."
 *
 * Covered here: a failed listTree leaves dirIndex/registry/overrides/DOM
 * completely untouched; a non-resolving listTree behaves the same once the
 * bounded timeout elapses; a single bounded retry fires and, on success,
 * replaces the index; a second consecutive failure does NOT arm a second
 * retry (bounded to one per failure streak); a subsequent SUCCESSFUL refresh
 * re-arms the bound for any future failure streak.
 *
 * Also covered: two overlapping refreshTree calls resolving
 * out of order. The OLDER payload must not be the one that lands.
 */
import { describe, test, expect, vi, beforeEach, afterEach } from 'vitest';
import { installPointerEventPolyfill, installCSSEscapePolyfill, priv, dir, file, startRenameByKey } from './test-helpers';

installPointerEventPolyfill();
installCSSEscapePolyfill();

const { mockListTree, mockGetProjectPath, mockListen, mockWarn, mockMossListeners } = vi.hoisted(() => ({
  mockListTree: vi.fn(),
  mockGetProjectPath: vi.fn(),
  mockListen: vi.fn(),
  mockWarn: vi.fn().mockResolvedValue(undefined),
  mockMossListeners: [] as Array<(payload: unknown) => void>,
}));

vi.mock('../../../engine', () => ({
  engine: {
    listTree: mockListTree,
    getProjectPath: mockGetProjectPath,
    renameEntry: vi.fn(),
    copyFilesToProject: vi.fn(),
    deleteEntry: vi.fn(),
    revealEntry: vi.fn(),
    duplicateEntry: vi.fn(),
    createFiles: vi.fn(),
    createFolder: vi.fn(),
    resolveUrlForFile: vi.fn().mockResolvedValue({ status: 'ok', data: '/' }),
  },
}));

vi.mock("../../../log", () => ({
  uiEvent: vi.fn(),
  debug: vi.fn().mockResolvedValue(undefined),
  info: vi.fn().mockResolvedValue(undefined),
  warn: mockWarn,
}));

import { host } from "../../../host";
import { FileTree } from '../controller';
import { EntryRegistry } from '../../entry-registry';
import type { TreeRefresh, RefreshState } from '../refresh';

const ROOT = '/Users/me/Sites/blog';

/** The controller's refresh cycle, and the record it keeps private. */
const refresher = (ft: FileTree) => priv<{ refresh: TreeRefresh }>(ft).refresh;
const state = (ft: FileTree) => priv<{ state: RefreshState }>(refresher(ft)).state;

function makeTree(folders: string[], projectRoot = ROOT) {
  return dir(projectRoot, [
      file(`${projectRoot}/index.md`),
      ...folders.map((name) => dir(`${projectRoot}/${name}`, [])),
    ], { name: 'blog' });
}

function fireMossEvent(event: unknown): void {
  Array.from(mockMossListeners).forEach((listener) => listener(event));
}

describe('keep-last-good refreshTree', () => {
  let registry: EntryRegistry;
  let fileTree: FileTree;
  let container: HTMLElement;

  beforeEach(async () => {
    vi.clearAllMocks();
    mockMossListeners.length = 0;
    host.engine.on = mockListen;
    mockListen.mockImplementation(async (_event: string, callback: (payload: unknown) => void) => {
      mockMossListeners.push(callback);
      return () => {
        const index = mockMossListeners.indexOf(callback);
        if (index >= 0) mockMossListeners.splice(index, 1);
      };
    });
    vi.useFakeTimers();
    mockGetProjectPath.mockResolvedValue({ status: 'ok', data: ROOT });
    mockListTree.mockResolvedValue({ status: 'ok', data: makeTree(['posts']) });

    registry = new EntryRegistry();
    fileTree = new FileTree(registry);
    container = document.createElement('div');
    document.body.appendChild(container);

    await fileTree.mount(container, ROOT, false);
  });

  afterEach(() => {
    fileTree.unmount();
    container.remove();
    vi.useRealTimers();
  });

  test('a failed listTree leaves dirIndex and the registry completely untouched', async () => {
    const dirIndexBefore = new Map(priv<{ dirIndex: Map<string, unknown> }>(fileTree).dirIndex);
    const pathsBefore = registry.allIds().map(id => registry.getById(id)!.path).sort();
    expect(dirIndexBefore.size).toBeGreaterThan(0);

    mockListTree.mockResolvedValueOnce({ status: 'error', error: 'boom' });
    const ok = await refresher(fileTree).refreshTree();

    expect(ok).toBe(false);
    expect(priv<{ dirIndex: Map<string, unknown> }>(fileTree).dirIndex).toEqual(dirIndexBefore);
    expect(registry.allIds().map(id => registry.getById(id)!.path).sort()).toEqual(pathsBefore);
    expect(mockWarn).toHaveBeenCalled();
    expect(String(mockWarn.mock.calls[0][0])).toMatch(/listTree failed/);
  });

  test('the special kind moss reports for a file reaches its registry record', async () => {
    mockListTree.mockResolvedValueOnce({
      status: 'ok',
      data: dir(ROOT, [file(`${ROOT}/AGENTS.md`, { special: 'agent-instructions' }), file(`${ROOT}/index.md`)], { name: 'blog' }),
    });

    expect(await refresher(fileTree).refreshTree()).toBe(true);

    expect(registry.recordAt('AGENTS.md')?.special).toBe('agent-instructions');
    expect(registry.recordAt('index.md')?.special).toBeNull();
  });

  test('a non-resolving listTree behaves the same once the bounded timeout elapses', async () => {
    const dirIndexBefore = new Map(priv<{ dirIndex: Map<string, unknown> }>(fileTree).dirIndex);
    // Never resolves within this test.
    mockListTree.mockImplementationOnce(() => new Promise(() => {}));

    const refreshPromise = refresher(fileTree).refreshTree();
    await vi.advanceTimersByTimeAsync(5000);
    const ok = await refreshPromise;

    expect(ok).toBe(false);
    expect(priv<{ dirIndex: Map<string, unknown> }>(fileTree).dirIndex).toEqual(dirIndexBefore);
    expect(mockWarn).toHaveBeenCalled();
    expect(String(mockWarn.mock.calls[0][0])).toMatch(/did not resolve/);
  });

  test('an immediately-rejected listTree logs the rejection cause, not a timeout claim', async () => {
    const dirIndexBefore = new Map(priv<{ dirIndex: Map<string, unknown> }>(fileTree).dirIndex);
    mockListTree.mockRejectedValueOnce(new Error('IPC channel closed'));

    const ok = await refresher(fileTree).refreshTree();

    expect(ok).toBe(false);
    expect(priv<{ dirIndex: Map<string, unknown> }>(fileTree).dirIndex).toEqual(dirIndexBefore);
    expect(mockWarn).toHaveBeenCalled();
    const message = String(mockWarn.mock.calls[0][0]);
    // A rejection is NOT a timeout — the WARN must name the actual cause
    // (a bare catch logs "did not resolve" even when the IPC
    // promise rejected immediately) and must not claim "did not resolve".
    expect(message).toMatch(/rejected: IPC channel closed/);
    expect(message).not.toMatch(/did not resolve/);
  });

  test('a bounded retry fires after a failure and replaces the index on success', async () => {
    mockListTree.mockResolvedValueOnce({ status: 'error', error: 'boom' });
    await refresher(fileTree).refreshTree();

    // Next listTree call (the retry) succeeds with a DIFFERENT tree.
    mockListTree.mockResolvedValue({ status: 'ok', data: makeTree(['posts', 'drafts']) });
    await vi.advanceTimersByTimeAsync(2000);
    // refreshAndRender's internal refreshTree() await needs a microtask flush.
    await vi.waitFor(() => {
      expect(priv<{ isDirectory(p: string): boolean }>(fileTree).isDirectory('drafts')).toBe(true);
    });
  });

  test('a second consecutive failure does not arm a second retry (bounded to one)', async () => {
    mockListTree.mockResolvedValue({ status: 'error', error: 'still down' });
    await refresher(fileTree).refreshTree();
    const timerAfterFirst = state(fileTree).retryTimer;
    expect(timerAfterFirst).not.toBeNull();

    // Let the retry fire; it calls refreshAndRender → refreshTree(), which
    // ALSO fails (still mocked as error) — this must NOT arm a second timer.
    // (mount() itself already made 1 listTree call, +1 for the explicit
    // refreshTree() above, +1 for the retry = 3.)
    await vi.advanceTimersByTimeAsync(2000);
    await vi.waitFor(() => {
      expect(mockListTree).toHaveBeenCalledTimes(3);
    });
    expect(state(fileTree).retryTimer).toBeNull();
  });

  test('An older listTree payload resolving last does not overwrite a newer one', async () => {
    // The watcher burst, rename and drop paths all call refreshTree, so two
    // round trips can be in flight at once and nothing ordered them. Here the
    // FIRST call resolves LAST, carrying the stale listing.
    const refresh = () => refresher(fileTree).refreshTree();
    let releaseStale!: (v: unknown) => void;
    mockListTree.mockImplementationOnce(
      () => new Promise((resolve) => { releaseStale = resolve; }),
    );

    const stale = refresh();                       // in flight, holding old data
    mockListTree.mockResolvedValueOnce({ status: 'ok', data: makeTree(['posts', 'drafts']) });
    expect(await refresh()).toBe(true);            // fresh listing lands first
    expect(priv<{ isDirectory(p: string): boolean }>(fileTree).isDirectory('drafts')).toBe(true);

    releaseStale({ status: 'ok', data: makeTree(['posts']) });
    expect(await stale).toBe(false);                // superseded, payload dropped

    // Without the generation token the stale payload would land last and
    // `drafts` would vanish from the tree until something else refreshed it.
    expect(priv<{ isDirectory(p: string): boolean }>(fileTree).isDirectory('drafts')).toBe(true);
  });

  test('a successful refresh re-arms the retry bound for a future failure streak', async () => {
    mockListTree.mockResolvedValueOnce({ status: 'error', error: 'boom' });
    await refresher(fileTree).refreshTree();
    expect(state(fileTree).retryExhausted).toBe(true);

    mockListTree.mockResolvedValueOnce({ status: 'ok', data: makeTree(['posts']) });
    await refresher(fileTree).refreshTree();
    expect(state(fileTree).retryExhausted).toBe(false);

    // A NEW failure streak can arm its own retry.
    mockListTree.mockResolvedValueOnce({ status: 'error', error: 'boom again' });
    await refresher(fileTree).refreshTree();
    expect(state(fileTree).retryTimer).not.toBeNull();
  });

  test('SourceStructureSettled recovers an exhausted refresh for the mounted folder and path', async () => {
    await vi.waitFor(() => expect(mockMossListeners.length).toBeGreaterThanOrEqual(1));
    mockListTree.mockResolvedValue({ status: 'error', error: 'storage pending' });
    await refresher(fileTree).refreshAndRender();
    await vi.advanceTimersByTimeAsync(2000);
    await vi.waitFor(() => expect(mockListTree).toHaveBeenCalledTimes(3));
    expect(state(fileTree).retryExhausted).toBe(true);

    mockListTree.mockResolvedValue({ status: 'ok', data: makeTree(['posts', 'drafts']) });
    fireMossEvent({ kind: 'BuildComplete', payload: { videos_converted: 0, total_time_ms: '100' } });
    fireMossEvent({
      kind: 'SourceStructureSettled',
      payload: { folder_path: `${ROOT}/other`, path: `${ROOT}/other/posts`, outcome: 'Ready' },
    });
    fireMossEvent({
      kind: 'SourceStructureSettled',
      payload: { folder_path: ROOT, path: `${ROOT}-sibling/drafts`, outcome: 'Ready' },
    });
    expect(mockListTree).toHaveBeenCalledTimes(3);

    fireMossEvent({
      kind: 'SourceStructureSettled',
      payload: { folder_path: `${ROOT}/`, path: `${ROOT}/posts/drafts`, outcome: 'Failed' },
    });

    await vi.waitFor(() => {
      expect(priv<{ isDirectory(path: string): boolean }>(fileTree).isDirectory('drafts')).toBe(true);
    });
    expect(state(fileTree).retryExhausted).toBe(false);
    expect(mockListTree).toHaveBeenCalledTimes(4);
  });

  test('folder identity accepts canonical Unicode spelling and lists a descendant path', async () => {
    const root = `${ROOT}/café`;
    const decomposedRoot = root.normalize('NFD');
    fileTree.unmount();
    mockListTree.mockResolvedValue({ status: 'ok', data: makeTree(['posts'], root) });
    await fileTree.mount(container, root, false);
    await vi.waitFor(() => expect(mockMossListeners.length).toBeGreaterThanOrEqual(1));

    mockListTree.mockResolvedValueOnce({ status: 'error', error: 'directory pending' });
    await refresher(fileTree).refreshAndRender();
    expect(state(fileTree).retryExhausted).toBe(true);
    mockListTree.mockResolvedValue({ status: 'ok', data: makeTree(['posts', 'drafts'], root) });
    fireMossEvent({
      kind: 'SourceStructureSettled',
      payload: { folder_path: decomposedRoot, path: `${decomposedRoot}/posts/drafts`, outcome: 'Ready' },
    });

    await vi.waitFor(() => {
      expect(priv<{ isDirectory(path: string): boolean }>(fileTree).isDirectory('drafts')).toBe(true);
    });
  });

  test('settlement does not refresh a healthy tree and is detached on unmount', async () => {
    await vi.waitFor(() => expect(mockMossListeners.length).toBeGreaterThanOrEqual(1));
    const healthyCalls = mockListTree.mock.calls.length;
    fireMossEvent({
      kind: 'SourceStructureSettled',
      payload: { folder_path: ROOT, path: `${ROOT}/posts/drafts`, outcome: 'Ready' },
    });
    await Promise.resolve();
    expect(mockListTree).toHaveBeenCalledTimes(healthyCalls);

    fileTree.unmount();
    const nextRoot = `${ROOT}-next`;
    mockListTree.mockResolvedValue({ status: 'ok', data: makeTree(['posts'], nextRoot) });
    await fileTree.mount(container, nextRoot, false);
    await vi.waitFor(() => expect(mockMossListeners.length).toBeGreaterThanOrEqual(1));
    const nextRootCalls = mockListTree.mock.calls.length;
    fireMossEvent({
      kind: 'SourceStructureSettled',
      payload: { folder_path: ROOT, path: `${ROOT}/posts/drafts`, outcome: 'Ready' },
    });
    expect(mockListTree).toHaveBeenCalledTimes(nextRootCalls);

    fileTree.unmount();
    fireMossEvent({
      kind: 'SourceStructureSettled',
      payload: { folder_path: nextRoot, path: `${nextRoot}/posts/drafts`, outcome: 'Ready' },
    });
    await Promise.resolve();
    expect(mockListTree).toHaveBeenCalledTimes(nextRootCalls);
  });

  test('settlement recovery waits behind an inline edit and flushes when it closes', async () => {
    await vi.waitFor(() => expect(mockMossListeners.length).toBeGreaterThanOrEqual(1));
    mockListTree.mockResolvedValue({ status: 'error', error: 'storage pending' });
    await refresher(fileTree).refreshAndRender();
    await vi.advanceTimersByTimeAsync(2000);
    await vi.waitFor(() => expect(mockListTree).toHaveBeenCalledTimes(3));

    fileTree.highlight(`${ROOT}/index.md`);
    const breadcrumb = Array.from(container.querySelectorAll<HTMLElement>('.seg.bc'))
      .find((element) => element.dataset.path === 'index.md');
    expect(breadcrumb).toBeTruthy();
    startRenameByKey(breadcrumb!);
    expect(container.querySelector('.moss-tree-rename-input')).not.toBeNull();

    const callsBeforeEvent = mockListTree.mock.calls.length;
    mockListTree.mockResolvedValue({ status: 'ok', data: makeTree(['posts', 'drafts']) });
    fireMossEvent({
      kind: 'SourceStructureSettled',
      payload: { folder_path: ROOT, path: `${ROOT}/posts/drafts`, outcome: 'Ready' },
    });
    expect(mockListTree).toHaveBeenCalledTimes(callsBeforeEvent);

    const input = container.querySelector('.moss-tree-rename-input') as HTMLInputElement;
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await vi.waitFor(() => {
      expect(priv<{ isDirectory(path: string): boolean }>(fileTree).isDirectory('drafts')).toBe(true);
    });
    expect(mockListTree).toHaveBeenCalledTimes(callsBeforeEvent + 1);
  });

  test('a subscription that resolves after unmount is immediately unsubscribed', async () => {
    let resolveListen!: (unlisten: () => void) => void;
    const nativeUnlisten = vi.fn();
    mockListen.mockImplementationOnce(async (_event: string, callback: (payload: unknown) => void) => {
      mockMossListeners.push(callback);
      return new Promise((resolve) => { resolveListen = resolve; });
    });

    await fileTree.mount(container, ROOT, false);
    await vi.waitFor(() => expect(resolveListen).toBeTypeOf('function'));
    const callsBeforeUnmount = mockListTree.mock.calls.length;
    fileTree.unmount();
    mockMossListeners[0]?.({ kind: 'FileChanged', payload: { changed_output_files: null, deleted_paths: null, moved_output_paths: null, modified_paths: null } });
    expect(mockListTree).toHaveBeenCalledTimes(callsBeforeUnmount);
    resolveListen(() => {
      nativeUnlisten();
      mockMossListeners.length = 0;
    });

    await vi.waitFor(() => expect(nativeUnlisten).toHaveBeenCalledOnce());
    expect(mockMossListeners).toHaveLength(0);
  });
});
