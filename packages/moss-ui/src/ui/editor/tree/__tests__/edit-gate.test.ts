/**
 * The file tree's admission gate — the lattice and the phantom grace window.
 *
 * These are the two rules that used to be written out by hand at each call
 * site: "which parked job wins" (four assignments across three methods) and
 * "is this session real" (two copies of the same branch). Proving them here
 * is why the tree's own lifecycle suite no longer has to.
 *
 * DOM-free by construction — the gate's only view of the DOM is the injected
 * `hasLiveInput` predicate. Runs in the node lane (vitest.config.js).
 */
import { describe, test, expect, vi } from 'vitest';
import { EditGate, EDIT_PHANTOM_GRACE_MS, EDIT_WATCHDOG_MS, type EditSession } from '../edit-gate';
import type { EntryId } from '../../entry-registry';

const RENAME: EditSession = { kind: 'rename', path: 'index.md' };
const CREATE: EditSession = { kind: 'create', folderPath: 'works', type: 'file', name: 'untitled.md', tempId: 1 as EntryId };

/** A gate over a hand-cranked clock and a hand-cranked timer queue, so both
 *  windows are exercised without waiting and the PUSH half (watchdog) can be
 *  starved independently of the PULL half — which is the whole reason there
 *  are two of them. */
function makeGate(opts: { hasLiveInput?: boolean } = {}) {
  let clock = 1000;
  let live = opts.hasLiveInput ?? true;
  const timers: Array<{ fn: () => void; at: number; handle: number }> = [];
  let nextHandle = 1;
  const discard = vi.fn();
  const reap = vi.fn();
  const gate = new EditGate({
    hasLiveInput: () => live,
    now: () => clock,
    discard,
    reap,
    trace: () => {},
    setTimer: (fn, ms) => {
      const handle = nextHandle++;
      timers.push({ fn, at: clock + ms, handle });
      return handle;
    },
    clearTimer: (h) => {
      const i = timers.findIndex((t) => t.handle === h);
      if (i >= 0) timers.splice(i, 1);
    },
  });
  return {
    gate,
    discard,
    reap,
    /** Advance the clock WITHOUT running timers — the throttled-webview case
     *  the pull-side grace window exists for. */
    starve: (ms: number) => { clock += ms; },
    /** Advance the clock and fire everything due, oldest first. */
    advance: (ms: number) => {
      clock += ms;
      for (const t of timers.filter((t) => t.at <= clock).sort((a, b) => a.at - b.at)) {
        if (timers.includes(t)) { timers.splice(timers.indexOf(t), 1); t.fn(); }
      }
    },
    pendingTimers: () => timers.length,
    setLiveInput: (v: boolean) => { live = v; },
  };
}

describe('EditGate — derivations', () => {
  test('the booleans callers used to store are getters over the one session', () => {
    const { gate } = makeGate();
    expect(gate.isEditing).toBe(false);
    expect(gate.renamingPath).toBeNull();
    expect(gate.creating).toBeNull();

    gate.begin(RENAME);
    expect(gate.isEditing).toBe(true);
    expect(gate.renamingPath).toBe('index.md');
    expect(gate.creating).toBeNull(); // a rename is not a create

    gate.begin(CREATE);
    expect(gate.renamingPath).toBeNull(); // ...and a create is not a rename
    expect(gate.creating).toEqual(CREATE);
  });

  test('endIfCurrent is object identity, not value equality', () => {
    const { gate } = makeGate();
    gate.begin(RENAME);
    // A second rename of the SAME path is a different session: ending it
    // would pull the live input out from under the gesture that owns it.
    expect(gate.endIfCurrent({ kind: 'rename', path: 'index.md' })).toBe(false);
    expect(gate.isEditing).toBe(true);
    expect(gate.endIfCurrent(RENAME)).toBe(true);
    expect(gate.isEditing).toBe(false);
  });
});

describe('EditGate — the deferred lattice', () => {
  test('a refresh parked behind a render wins; the reverse does not downgrade it', () => {
    const { gate } = makeGate();
    gate.begin(RENAME);
    expect(gate.admit('render')).toBe(false);
    expect(gate.admit('refresh')).toBe(false);
    expect(gate.admit('render')).toBe(false); // must not knock it back down
    gate.end();
    expect(gate.takeDeferred()).toBe('refresh');
    expect(gate.takeDeferred()).toBe('none'); // draining is destructive
  });

  test('admitting work clears a parked job of equal or lower rank only', () => {
    const { gate } = makeGate();
    gate.begin(RENAME);
    gate.admit('refresh');
    gate.end();
    // A bare render does not satisfy a parked refresh — the refresh re-fetches
    // the tree, the render only repaints what is already indexed.
    expect(gate.admit('render')).toBe(true);
    expect(gate.takeDeferred()).toBe('refresh');

    gate.begin(RENAME);
    gate.admit('render');
    gate.end();
    // A refresh does satisfy a parked render: it renders on the way out.
    expect(gate.admit('refresh')).toBe(true);
    expect(gate.takeDeferred()).toBe('none');
  });
});

describe('EditGate — the phantom self-heal', () => {
  test('a session with a live input is never phantom, however old', () => {
    const { gate, discard, starve } = makeGate({ hasLiveInput: true });
    gate.begin(CREATE);
    starve(EDIT_PHANTOM_GRACE_MS * 100);
    expect(gate.admit('render')).toBe(false);
    expect(gate.isEditing).toBe(true);
    expect(discard).not.toHaveBeenCalled();
  });

  test('a fresh session inside its mount gap is parked, not reaped', () => {
    // The race the grace window exists for: startCreate opens the session,
    // then mounts its <input> from a deferred setTimeout an idle WKWebView
    // can starve. A watcher render landing in that gap must not cancel it.
    const { gate, discard } = makeGate({ hasLiveInput: false });
    gate.begin(CREATE);
    expect(gate.admit('render')).toBe(false);
    expect(gate.isEditing).toBe(true);
    expect(discard).not.toHaveBeenCalled();
  });

  test('an inputless session past the grace window is discarded and the work admitted', () => {
    const { gate, discard, starve } = makeGate({ hasLiveInput: false });
    gate.begin(CREATE);
    starve(EDIT_PHANTOM_GRACE_MS + 1);
    expect(gate.admit('render')).toBe(true);
    expect(gate.isEditing).toBe(false);
    expect(discard).toHaveBeenCalledExactlyOnceWith(CREATE);
  });

  test('healing does not resurrect a job parked before the leak', () => {
    const { gate, setLiveInput, starve } = makeGate({ hasLiveInput: true });
    gate.begin(CREATE);
    gate.admit('refresh'); // parked while the input was live
    setLiveInput(false);
    starve(EDIT_PHANTOM_GRACE_MS + 1);
    // The heal admits this render, but a render does not satisfy the parked
    // refresh — the watcher event that raised it still needs a re-fetch.
    expect(gate.admit('render')).toBe(true);
    expect(gate.takeDeferred()).toBe('refresh');
  });
});

describe('EditGate — the watchdog', () => {
  test('a session whose input never mounts is reaped at the window', () => {
    const { gate, reap, advance } = makeGate({ hasLiveInput: false });
    gate.begin(CREATE);
    advance(EDIT_WATCHDOG_MS);
    expect(gate.isEditing).toBe(false);
    // reap, not discard: nothing else is about to repaint, so the tree's
    // recovery has to be driven from here.
    expect(reap).toHaveBeenCalledExactlyOnceWith(CREATE);
  });

  test('a live input the user is typing into is never reaped', () => {
    const { gate, reap, advance } = makeGate({ hasLiveInput: true });
    gate.begin(CREATE);
    advance(EDIT_WATCHDOG_MS * 10);
    expect(gate.isEditing).toBe(true);
    expect(reap).not.toHaveBeenCalled();
  });

  test('it covers a RENAME too — the tree armed it only for creates', () => {
    const { gate, reap, advance } = makeGate({ hasLiveInput: false });
    gate.begin(RENAME);
    advance(EDIT_WATCHDOG_MS);
    expect(reap).toHaveBeenCalledExactlyOnceWith(RENAME);
  });

  test('ending a session disarms its watchdog, and a new session re-arms', () => {
    const { gate, reap, advance, pendingTimers } = makeGate({ hasLiveInput: false });
    gate.begin(CREATE);
    gate.end();
    expect(pendingTimers()).toBe(0);
    advance(EDIT_WATCHDOG_MS * 2);
    expect(reap).not.toHaveBeenCalled();

    gate.begin(RENAME);
    advance(EDIT_WATCHDOG_MS);
    expect(reap).toHaveBeenCalledExactlyOnceWith(RENAME);
  });

  test('a starved watchdog leaves the pull half to heal it', () => {
    // The webview stopped delivering timers. Only admit() can run, and it
    // must not need the timer to notice the leak.
    const { gate, discard, reap, starve } = makeGate({ hasLiveInput: false });
    gate.begin(CREATE);
    starve(EDIT_PHANTOM_GRACE_MS + 1);
    expect(reap).not.toHaveBeenCalled();
    expect(gate.admit('render')).toBe(true);
    expect(discard).toHaveBeenCalledExactlyOnceWith(CREATE);
  });
});

describe('EditGate — canStartGesture', () => {
  test('an open session refuses a new gesture', () => {
    const { gate } = makeGate();
    expect(gate.canStartGesture).toBe(true);
    gate.begin(RENAME);
    expect(gate.canStartGesture).toBe(false);
    gate.end();
    expect(gate.canStartGesture).toBe(true);
  });

  test('a rename IPC refuses a new gesture but does NOT close the gate', async () => {
    // The asymmetry this replaced: startRename checked the in-flight rename
    // and startCreate did not, so a create could start mid-rename. And the
    // gate must stay OPEN — parking the optimistic repaint behind the
    // vault-wide link rewrite is the wait the predicted name exists to remove.
    const { gate } = makeGate();
    let insideCanStart: boolean | null = null;
    let insideAdmit: boolean | null = null;
    await gate.duringRenameCommit(async () => {
      insideCanStart = gate.canStartGesture;
      insideAdmit = gate.admit('render');
    });
    expect(insideCanStart).toBe(false);
    expect(insideAdmit).toBe(true);
    expect(gate.canStartGesture).toBe(true);
  });

  test('a rejected rename IPC still releases the gesture lock', async () => {
    const { gate } = makeGate();
    await expect(gate.duringRenameCommit(() => Promise.reject(new Error('offline'))))
      .rejects.toThrow('offline');
    expect(gate.canStartGesture).toBe(true);
  });
});

describe('EditGate — reset', () => {
  test('unmount drops session, watchdog and parked work without discarding', () => {
    const { gate, discard, reap, advance, pendingTimers } = makeGate({ hasLiveInput: false });
    gate.begin(CREATE);
    gate.admit('refresh');
    gate.reset();
    expect(gate.isEditing).toBe(false);
    expect(gate.takeDeferred()).toBe('none');
    expect(pendingTimers()).toBe(0);
    advance(EDIT_WATCHDOG_MS * 2);
    // unmount tears the whole tree down; there is no bookkeeping left to undo
    // and no recovery to drive.
    expect(discard).not.toHaveBeenCalled();
    expect(reap).not.toHaveBeenCalled();
  });
});
