/**
 * The slow-trash status toast in tree/ops.ts `trash()`, driven through the
 * public `deleteEntries` with a minimal OpsHost — no controller mount needed.
 *
 * Pins the behavior when a trash command the OS
 * holds for seconds used to be dead air the user read as a broken delete. A
 * command outliving the threshold now shows a persistent status toast, cleared
 * when the command settles — success or failure alike.
 *
 * Routed through the single-empty-folder path (trashes without the modal), so
 * the executor is reached without any DOM. The modal phases and the failure
 * toast are pinned elsewhere (delete-confirm-modal.test.ts, menu.test.ts).
 */

import { describe, test, expect, vi, beforeEach, afterEach } from 'vitest';

const { mockShowOrUpdateToast, mockRemoveToastById, mockShowToast } = vi.hoisted(() => ({
  mockShowOrUpdateToast: vi.fn(),
  mockRemoveToastById: vi.fn(),
  mockShowToast: vi.fn(),
}));

vi.mock("../../../shared/ui/toast/toast-manager", () => ({
  showToast: mockShowToast,
  showAck: vi.fn(),
  showOrUpdateToast: mockShowOrUpdateToast,
  removeToastById: mockRemoveToastById,
}));

import { TreeOps, type OpsHost, type OpsCommands } from '../ops';

const PROJECT_ROOT = '/projects/my-site';

type Deferred = { settle: (r: { status: 'ok' | 'error'; data?: null; error?: string }) => void };

/** A deleteEntries mock the test settles by hand. */
function deferredDelete(): { fn: ReturnType<typeof vi.fn>; d: Deferred } {
  const d = {} as Deferred;
  const fn = vi.fn().mockImplementation(
    () => new Promise(res => {
      d.settle = res; // command errors arrive as resolved {status:'error'}
    }),
  );
  return { fn, d };
}

/** An ops instance whose 'drafts' path is an empty folder: trashes modal-free. */
function makeOps(deleteEntries: ReturnType<typeof vi.fn>) {
  const commands = {
    deleteEntries,
    scanReferencesForDelete: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  } as unknown as OpsCommands;
  const host = {
    projectRoot: () => PROJECT_ROOT,
    commands,
    registry: () => ({ recordAt: () => ({ isDir: true }) }),
    children: () => [],
    // admit(): false short-circuits applyDeleteLocally — the purge is the
    // controller's business, not under test here.
    gate: { admit: () => false },
  } as unknown as OpsHost;
  return new TreeOps(host);
}

/** Flush microtasks under fake timers. */
async function flush(): Promise<void> {
  await vi.advanceTimersByTimeAsync(0);
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.clearAllMocks();
  vi.useRealTimers();
});

describe('slow-trash status toast', () => {
  test('a command outliving the threshold shows a persistent toast, cleared on settle', async () => {
    const { fn, d } = deferredDelete();
    const ops = makeOps(fn);

    const done = ops.deleteEntries(['drafts']);
    await flush(); // scan resolves clean, command starts
    expect(fn).toHaveBeenCalledTimes(1);
    expect(mockShowOrUpdateToast).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(2100);
    expect(mockShowOrUpdateToast).toHaveBeenCalledTimes(1);
    const [message, options] = mockShowOrUpdateToast.mock.calls[0];
    expect(message).toContain('drafts');
    expect(options).toMatchObject({ persistent: true });

    d.settle({ status: 'ok', data: null });
    await done;
    expect(mockRemoveToastById).toHaveBeenCalledWith(options.id);
  });

  test('a fast delete never shows the status toast', async () => {
    const fn = vi.fn().mockResolvedValue({ status: 'ok', data: null });
    const ops = makeOps(fn);
    const done = ops.deleteEntries(['drafts']);
    await vi.advanceTimersByTimeAsync(3000);
    await done;
    expect(fn).toHaveBeenCalledTimes(1);
    expect(mockShowOrUpdateToast).not.toHaveBeenCalled();
  });

  test('a failing command still clears the toast', async () => {
    const { fn, d } = deferredDelete();
    const ops = makeOps(fn);

    const done = ops.deleteEntries(['drafts']);
    await vi.advanceTimersByTimeAsync(2100);
    expect(mockShowOrUpdateToast).toHaveBeenCalledTimes(1);

    d.settle({ status: 'error', error: 'disk on fire' });
    await done; // runAction absorbs the failure into its toast
    expect(mockRemoveToastById).toHaveBeenCalled();
  });
});

describe('delete failure toast', () => {
  test('shortens the absolute path the command echoes back to its basename', async () => {
    // Mirrors the shape `delete_entry_inner` reports today (and the single
    // clean sentence it's moving to): the failing command's own error names
    // the exact absolute path this call sent it, and the client's real path
    // was ~120 characters of iCloud noise before this fix.
    const absPath = `${PROJECT_ROOT}/文稿/测试.md`;
    const fn = vi.fn().mockResolvedValue({
      status: 'error',
      error: `Couldn't move '${absPath}' to the Trash. Try again, or delete it in Finder.`,
    });
    const ops = makeOps(fn);

    await ops.deleteEntries(['文稿/测试.md']);

    expect(mockShowToast).toHaveBeenCalledTimes(1);
    const [{ what }] = mockShowToast.mock.calls[0];
    expect(what).toContain('测试.md');
    expect(what).not.toContain(absPath);
  });
});
