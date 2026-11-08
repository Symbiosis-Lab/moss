import { describe, test, expect, vi } from 'vitest';
import { EditorFileEvents } from '../editor-file-events';
import { EntryRegistry } from '../entry-registry';
import { TreeRefresh, type CommandResult, type ListedNode, type RefreshHost } from '../tree/refresh';
import { initialView } from '../tree/model';

const { sourceStructureHandlers } = vi.hoisted(() => ({
  sourceStructureHandlers: [] as Array<(event: { payload: any }) => void | Promise<void>>,
}));

vi.mock("../../utils/moss-events", () => ({
  onMossEvent: vi.fn(async (kind: string, handler: (event: { payload: any }) => void | Promise<void>) => {
    if (kind === 'SourceStructureSettled') sourceStructureHandlers.push(handler);
    return () => {
      if (kind === 'SourceStructureSettled') {
        const index = sourceStructureHandlers.indexOf(handler);
        if (index >= 0) sourceStructureHandlers.splice(index, 1);
      }
    };
  }),
}));

function mockBus() {
  const cbs: Record<string, Array<(m: any) => void | Promise<void>>> = {};
  const onMossEvent = vi.fn(async (kind: string, cb: (m: any) => void | Promise<void>) => {
    (cbs[kind] ??= []).push(cb);
    return () => { cbs[kind] = (cbs[kind] ?? []).filter((c) => c !== cb); };
  });
  // fire returns a promise so callers can await full async dispatch
  const fire = (kind: string, payload: any) =>
    Promise.all((cbs[kind] ?? []).map((c) => c({ payload })));
  return { onMossEvent, fire, cbs };
}

function makeTreeRefresh(listTree: RefreshHost['commands']['listTree']) {
  const registry = new EntryRegistry();
  const dirIndex = new Map();
  const render = vi.fn();
  const host: RefreshHost = {
    projectRoot: () => '/project',
    commands: { listTree },
    registry: () => registry,
    dirIndex,
    update: (fn) => { fn(initialView()); },
    reconcileSelection: vi.fn(),
    gate: { admit: () => true },
    render,
    leaveInternal: vi.fn(),
    flushPendingWork: vi.fn(),
    refreshDerivations: async () => {},
  };
  return { tree: new TreeRefresh(host), dirIndex, render };
}

describe('EditorFileEvents', () => {
  test('subscribes to FileChanged + AssetReady exactly once', async () => {
    const bus = mockBus();
    const c = new EditorFileEvents(bus.onMossEvent as any);
    await c.start();
    expect(bus.cbs['FileChanged']?.length).toBe(1);
    expect(bus.cbs['AssetReady']?.length).toBe(1);
  });

  test('dispatches FileChanged to registered handlers in priority order', async () => {
    const bus = mockBus();
    const c = new EditorFileEvents(bus.onMossEvent as any);
    await c.start();
    const order: string[] = [];
    c.onFileChanged(() => { order.push('low'); }, 100);
    c.onFileChanged(() => { order.push('high'); }, 0); // lower number = earlier
    await bus.fire('FileChanged', { modified_paths: [] });
    expect(order).toEqual(['high', 'low']);
  });

  test('unregister stops delivery; stop() tears down the bus subscriptions', async () => {
    const bus = mockBus();
    const c = new EditorFileEvents(bus.onMossEvent as any);
    await c.start();
    const handler = vi.fn();
    const off = c.onAssetReady(handler);
    await bus.fire('AssetReady', { path: 'a.jpg', asset_type: 'image' });
    expect(handler).toHaveBeenCalledTimes(1);
    off();
    await bus.fire('AssetReady', { path: 'a.jpg', asset_type: 'image' });
    expect(handler).toHaveBeenCalledTimes(1);
    await c.stop();
    expect(bus.cbs['FileChanged']?.length).toBe(0);
  });

  test('a throwing handler does not block other handlers', async () => {
    const bus = mockBus();
    const c = new EditorFileEvents(bus.onMossEvent as any);
    await c.start();
    const good = vi.fn();
    c.onFileChanged(() => { throw new Error('boom'); }, 0);
    c.onFileChanged(good, 10);
    // FileChanged dispatch is async and catches internally — just await it
    await expect(bus.fire('FileChanged', { modified_paths: [] })).resolves.not.toThrow();
    expect(good).toHaveBeenCalledTimes(1);
  });

  test('FileChanged handlers complete in priority order (sequential await)', async () => {
    const bus = mockBus();
    const c = new EditorFileEvents(bus.onMossEvent as any);
    await c.start();
    const order: string[] = [];
    c.onFileChanged(async () => { await new Promise(r => setTimeout(r, 5)); order.push('first(0)'); }, 0);
    c.onFileChanged(async () => { order.push('second(10)'); }, 10);
    // fire the captured callback and await its completion
    await bus.cbs['FileChanged'][0]({ payload: { modified_paths: [] } });
    expect(order).toEqual(['first(0)', 'second(10)']);
  });

  test('mounted tree refresh completes before the next FileChanged consumer', async () => {
    const bus = mockBus();
    const events = new EditorFileEvents(bus.onMossEvent as any);
    await events.start();

    let finishListing!: (result: CommandResult<ListedNode>) => void;
    const listing = new Promise<CommandResult<ListedNode>>((resolve) => {
      finishListing = resolve;
    });
    const { tree } = makeTreeRefresh(() => listing);
    const mounted = new AbortController();
    await tree.startWatching(mounted.signal, events);

    let observerRan = false;
    events.onFileChanged(() => { observerRan = true; }, 100);

    const dispatch = bus.cbs.FileChanged[0]({ payload: { modified_paths: [] } });
    await Promise.resolve();
    expect(observerRan).toBe(false);

    finishListing({
      status: 'ok',
      data: {
        path: '/project', is_dir: true, file_id: null, is_home: false, hidden: 0,
        children: [{ path: '/project/a.md', is_dir: false, file_id: null, is_home: false, hidden: 0, children: null }],
      },
    });
    await dispatch;
    expect(observerRan).toBe(true);
    mounted.abort();
    await events.stop();
  });

  test('unmount discards a late SourceStructureSettled listing without retrying or rendering', async () => {
    const listingResult: ListedNode = {
      path: '/project', is_dir: true, file_id: null, is_home: false, hidden: 0,
      children: [{ path: '/project/a.md', is_dir: false, file_id: null, is_home: false, hidden: 0, children: null }],
    };
    let resolveListing!: (result: CommandResult<ListedNode>) => void;
    let rejectListing!: (error: Error) => void;
    const pendingListing = new Promise<CommandResult<ListedNode>>((resolve, reject) => {
      resolveListing = resolve;
      rejectListing = reject;
    });
    const listTree = vi.fn()
      .mockResolvedValueOnce({ status: 'error', error: 'initial offline' })
      .mockImplementationOnce(() => pendingListing);
    const { tree, dirIndex, render } = makeTreeRefresh(listTree);
    const mounted = new AbortController();
    sourceStructureHandlers.length = 0;
    await tree.startWatching(mounted.signal, undefined);
    await tree.refreshTree(); // exhaust the first attempt so SourceStructureSettled is accepted

    sourceStructureHandlers[0]({ payload: { folder_path: '/project', path: '/project/a.md' } });
    expect(listTree).toHaveBeenCalledTimes(2);
    mounted.abort();
    resolveListing({ status: 'ok', data: listingResult });
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(dirIndex.size).toBe(0);
    expect(render).not.toHaveBeenCalled();

    // A late failure must not arm the bounded retry after dispose either.
    sourceStructureHandlers.length = 0;
    let rejectLateFailure!: (error: Error) => void;
    const lateFailure = new Promise<CommandResult<ListedNode>>((_resolve, reject) => {
      rejectLateFailure = reject;
    });
    const failedListTree = vi.fn()
      .mockResolvedValueOnce({ status: 'error', error: 'initial offline' })
      .mockImplementationOnce(() => lateFailure)
      .mockResolvedValue({ status: 'ok', data: listingResult });
    const failedTree = makeTreeRefresh(failedListTree);
    const failedMount = new AbortController();
    await failedTree.tree.startWatching(failedMount.signal, undefined);
    await failedTree.tree.refreshTree(); // enable the structural recovery callback
    sourceStructureHandlers[0]({ payload: { folder_path: '/project', path: '/project/a.md' } });
    expect(failedListTree).toHaveBeenCalledTimes(2);
    failedMount.abort();
    rejectLateFailure(new Error('offline'));
    await new Promise((resolve) => setTimeout(resolve, 0));
    vi.useFakeTimers();
    try {
      await vi.advanceTimersByTimeAsync(2_500);
      expect(failedListTree).toHaveBeenCalledTimes(2);
      expect(failedTree.dirIndex.size).toBe(0);
      expect(failedTree.render).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  test('start() is idempotent — no duplicate subscriptions', async () => {
    const bus = mockBus();
    const c = new EditorFileEvents(bus.onMossEvent as any);
    await c.start();
    await c.start();
    expect(bus.cbs['FileChanged']?.length).toBe(1);
    expect(bus.cbs['AssetReady']?.length).toBe(1);
    expect(bus.cbs['SourceAssetChanged']?.length).toBe(1);
    expect(bus.cbs['RawFileCreated']?.length).toBe(1);
  });

  // ── SourceAssetChanged ────────────────────────────────────────────────────

  test('subscribes to SourceAssetChanged exactly once', async () => {
    const bus = mockBus();
    const c = new EditorFileEvents(bus.onMossEvent as any);
    await c.start();
    expect(bus.cbs['SourceAssetChanged']?.length).toBe(1);
  });

  test('dispatches SourceAssetChanged to registered handlers', async () => {
    const bus = mockBus();
    const c = new EditorFileEvents(bus.onMossEvent as any);
    await c.start();
    const handler = vi.fn();
    c.onSourceAssetChanged(handler);
    await bus.fire('SourceAssetChanged', { path: '/图片/摄影/X.jpeg' });
    expect(handler).toHaveBeenCalledTimes(1);
    expect(handler).toHaveBeenCalledWith({ path: '/图片/摄影/X.jpeg' });
  });

  test('SourceAssetChanged unregister stops delivery', async () => {
    const bus = mockBus();
    const c = new EditorFileEvents(bus.onMossEvent as any);
    await c.start();
    const handler = vi.fn();
    const off = c.onSourceAssetChanged(handler);
    await bus.fire('SourceAssetChanged', { path: '/img.png' });
    expect(handler).toHaveBeenCalledTimes(1);
    off();
    await bus.fire('SourceAssetChanged', { path: '/img.png' });
    expect(handler).toHaveBeenCalledTimes(1);
  });

  test('stop() tears down SourceAssetChanged subscription', async () => {
    const bus = mockBus();
    const c = new EditorFileEvents(bus.onMossEvent as any);
    await c.start();
    await c.stop();
    // After stop(), the bus subscription must be removed (unlisten called).
    expect(bus.cbs['SourceAssetChanged']?.length).toBe(0);
  });

  test('SourceAssetChanged with path dispatched via coordinator bumps assetBustQuery', async () => {
    // Integration: simulate the coordinator wiring in cm-reference-resolver.
    // The coordinator's onSourceAssetChanged handler calls bumpAssetBust(e.path).
    // Verify end-to-end that the path from SourceAssetChanged makes the bust token non-empty.
    const { bumpAssetBust, assetBustQuery, __resetAssetBustForTests } =
      await import('../asset-bust');
    __resetAssetBustForTests();

    const bus = mockBus();
    const c = new EditorFileEvents(bus.onMossEvent as any);
    await c.start();

    // Register a handler that calls bumpAssetBust — mirrors what the resolver does.
    c.onSourceAssetChanged((e) => { bumpAssetBust(e.path); });

    await bus.fire('SourceAssetChanged', { path: '/图片/摄影/X.jpeg' });

    const q = assetBustQuery('/图片/摄影/X.jpeg');
    expect(q).not.toBe('');
    expect(q).toMatch(/^\?_t=\d+$/);

    __resetAssetBustForTests();
  });
});
