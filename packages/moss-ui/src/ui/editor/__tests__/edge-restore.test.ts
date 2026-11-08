/**
 * Collapsed-pane edge affordance, RIGHT edge — the editor-owned "back to
 * split" accelerator.
 *
 * These tests pin the behaviour jsdom can see: the strip exists exactly while
 * the editor is full-width (it must be gone the instant split resumes, because
 * in split mode those pixels are the shell's draggable divider), the dwell and
 * the scroll suppression gate the reveal, the click emits the same event the
 * layout button emits, and the tab never fades out from under the cursor.
 *
 * Everything painted — nothing at rest, the muted fill, the fade — is a render
 * gate that measures edge restoration; jsdom has no cascade.
 */

import { describe, test, expect, vi, beforeEach, afterEach, type MockInstance } from 'vitest';

type HostModule = typeof import("../../host");
type EdgeModule = typeof import('../edge-restore');

// Set by freshEdge: the host module is re-evaluated with the edge module after each reset.
let host: HostModule['host'];
let mockEmit: MockInstance;

async function freshEdge(): Promise<EdgeModule> {
  vi.resetModules();
  const hostModule: HostModule = await import("../../host");
  host = hostModule.host;
  host.bus = hostModule.inPageBus();
  mockEmit = vi.spyOn(host.bus, 'emit');
  const mod = await import('../edge-restore');
  mod.initEdgeRestore();
  return mod;
}

function setFullWidth(fullWidth: boolean): void {
  host.bus.emit('editor:full-width-changed', { fullWidth });
}

/** A pointermove at (x, y) in the editor document. */
function movePointer(x: number, y: number): void {
  const e = new Event('pointermove') as PointerEvent & { clientX: number; clientY: number };
  Object.defineProperty(e, 'clientX', { value: x });
  Object.defineProperty(e, 'clientY', { value: y });
  document.dispatchEvent(e);
}

const WIDTH = 1000;
/** Inside the 24px zone and clear of the 48px nav band. */
const IN_ZONE = { x: WIDTH - 4, y: 300 };

describe('right-edge restore — lifecycle', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    window.innerWidth = WIDTH;
    document.body.innerHTML = '';
  });

  afterEach(() => {
    vi.useRealTimers();
    document.body.innerHTML = '';
  });

  test('nothing is mounted until the editor goes full-width', async () => {
    const m = await freshEdge();
    expect(m.edgeRestoreElement()).toBeNull();
    setFullWidth(true);
    expect(m.edgeRestoreElement()).not.toBeNull();
  });

  test('the strip is torn down the instant full-width ends', async () => {
    const m = await freshEdge();
    setFullWidth(true);
    setFullWidth(false);
    // In split mode these pixels belong to the shell's divider — a different
    // gesture and a different cursor, so nothing of ours may survive.
    expect(m.edgeRestoreElement()).toBeNull();
    expect(document.querySelector('.moss-edge-restore')).toBeNull();
  });

  test('the pointermove listener is removed on teardown', async () => {
    const m = await freshEdge();
    setFullWidth(true);
    setFullWidth(false);
    expect(() => { movePointer(IN_ZONE.x, IN_ZONE.y); }).not.toThrow();
    expect(m.edgeRestoreElement()).toBeNull();
  });

  test('a repeated full-width event does not mount a second strip', async () => {
    await freshEdge();
    setFullWidth(true);
    setFullWidth(true);
    expect(document.querySelectorAll('.moss-edge-restore').length).toBe(1);
  });
});

describe('right-edge restore — hover, dwell and suppression', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    window.innerWidth = WIDTH;
    document.body.innerHTML = '';
  });

  afterEach(() => {
    vi.useRealTimers();
    document.body.innerHTML = '';
  });

  test('the tab only takes pointer events AFTER the dwell', async () => {
    const m = await freshEdge();
    setFullWidth(true);
    vi.advanceTimersByTime(1300); // let the just-in-time reveal lapse
    movePointer(500, 300);
    movePointer(IN_ZONE.x, IN_ZONE.y);
    const strip = m.edgeRestoreElement()!;
    expect(strip.classList.contains('is-hot')).toBe(true);
    // Not armed yet: an invisible target here would steal a scrollbar grab.
    expect(strip.classList.contains('is-armed')).toBe(false);
    vi.advanceTimersByTime(150);
    expect(strip.classList.contains('is-armed')).toBe(true);
  });

  test('leaving the zone before the dwell elapses reveals nothing', async () => {
    const m = await freshEdge();
    setFullWidth(true);
    vi.advanceTimersByTime(1300);
    movePointer(IN_ZONE.x, IN_ZONE.y);
    vi.advanceTimersByTime(100);
    movePointer(200, 300); // a pass-through, not intent
    vi.advanceTimersByTime(500);
    const strip = m.edgeRestoreElement()!;
    expect(strip.classList.contains('is-hot')).toBe(false);
    expect(strip.classList.contains('is-armed')).toBe(false);
  });

  test('the top nav band is not part of the zone', async () => {
    const m = await freshEdge();
    setFullWidth(true);
    vi.advanceTimersByTime(1300);
    movePointer(IN_ZONE.x, 20); // inside the 48px nav band
    expect(m.edgeRestoreElement()!.classList.contains('is-hot')).toBe(false);
  });

  test('a fast flick to the edge right after a scroll summons nothing', async () => {
    const m = await freshEdge();
    setFullWidth(true);
    vi.advanceTimersByTime(1300);
    document.dispatchEvent(new Event('wheel'));
    movePointer(IN_ZONE.x, IN_ZONE.y);
    const strip = m.edgeRestoreElement()!;
    expect(strip.classList.contains('is-hot')).toBe(false);
    // …and the edge comes back once the suppression window passes.
    vi.advanceTimersByTime(401);
    movePointer(IN_ZONE.x, IN_ZONE.y + 1);
    expect(strip.classList.contains('is-hot')).toBe(true);
  });

  test('scrolling while the tab is up puts it away', async () => {
    const m = await freshEdge();
    setFullWidth(true);
    vi.advanceTimersByTime(1300);
    movePointer(IN_ZONE.x, IN_ZONE.y);
    vi.advanceTimersByTime(200);
    const strip = m.edgeRestoreElement()!;
    expect(strip.classList.contains('is-armed')).toBe(true);
    document.dispatchEvent(new Event('wheel'));
    expect(strip.classList.contains('is-hot')).toBe(false);
    expect(strip.classList.contains('is-armed')).toBe(false);
  });
});

describe('right-edge restore — reveal and click', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    window.innerWidth = WIDTH;
    document.body.innerHTML = '';
  });

  afterEach(() => {
    vi.useRealTimers();
    document.body.innerHTML = '';
  });

  test('collapsing the preview reveals the tab and holds it ~1.2s', async () => {
    const m = await freshEdge();
    setFullWidth(true);
    const strip = m.edgeRestoreElement()!;
    expect(strip.classList.contains('is-revealed')).toBe(true);
    vi.advanceTimersByTime(1199);
    expect(strip.classList.contains('is-revealed')).toBe(true);
    vi.advanceTimersByTime(2);
    expect(strip.classList.contains('is-revealed')).toBe(false);
  });

  test('the tab never fades out from under the cursor — `.is-hot` carries that, not the hold', async () => {
    const m = await freshEdge();
    setFullWidth(true);
    movePointer(IN_ZONE.x, IN_ZONE.y);
    vi.advanceTimersByTime(2000);
    const strip = m.edgeRestoreElement()!;
    // The HOLD is over on schedule — it is a hint with a deadline, and a hold
    // that outlasted its timer whenever `inZone` was true could never end at
    // all once `inZone` was wrong (a pointer that parks elsewhere sends no
    // `pointermove` to correct it).
    expect(strip.classList.contains('is-revealed')).toBe(false);
    // The tab is still painted, because the pointer is genuinely here and
    // `.is-hot` says so. This class is the right edge's `:hover`.
    expect(strip.classList.contains('is-hot')).toBe(true);
    // …and it is self-correcting: one move out of the zone clears it.
    movePointer(200, 300);
    expect(strip.classList.contains('is-hot')).toBe(false);
  });

  test('…including a pointer that was already parked there, and never moves', async () => {
    // The realistic case, and the one an enter-event alone cannot see: a
    // right-snap drag releases on mouseup with the hand at the edge, or ⌘⇧E
    // fires with it resting there. The strip is created UNDER that cursor and
    // no pointer event follows — so the position has to have been remembered
    // from before it existed.
    const m = await freshEdge();
    movePointer(IN_ZONE.x, IN_ZONE.y); // the last move before full-width
    setFullWidth(true);
    const strip = m.edgeRestoreElement()!;
    // Visible AND clickable at once: the hold already shows the tab, so making
    // the user earn a dwell for something on screen would be theatre.
    expect(strip.classList.contains('is-armed')).toBe(true);
    vi.advanceTimersByTime(2000);
    // The seed's job is `.is-hot` / `.is-armed`, which keep the tab painted and
    // clickable under the parked cursor. It does NOT extend the hold: a seeded
    // position is a guess, and a guess must not be able to pin the tab open.
    expect(strip.classList.contains('is-hot')).toBe(true);
    expect(strip.classList.contains('is-revealed')).toBe(false);
    movePointer(200, 300);
    expect(strip.classList.contains('is-hot')).toBe(false);
  });

  test('a pointer that leaves the window releases the tab — no move required', async () => {
    // `.is-hot` / `.is-armed` carry the "don't fade out from under the cursor"
    // promise now, so they must be clearable by something OTHER than a
    // pointermove. Enter the zone and then leave the window: every intermediate
    // position is still inside the 24px band, so the tracker observes no
    // transition and nothing would ever clear it. That is the same shape as the
    // bug this whole change is about — state held on a belief no event corrects.
    const m = await freshEdge();
    setFullWidth(true);
    movePointer(IN_ZONE.x, IN_ZONE.y);
    const strip = m.edgeRestoreElement()!;
    vi.advanceTimersByTime(1300);
    expect(strip.classList.contains('is-hot')).toBe(true);

    document.dispatchEvent(new Event('pointerleave'));
    expect(strip.classList.contains('is-hot')).toBe(false);
    expect(strip.classList.contains('is-armed')).toBe(false);
  });

  test('a window that loses focus releases it too', async () => {
    const m = await freshEdge();
    setFullWidth(true);
    movePointer(IN_ZONE.x, IN_ZONE.y);
    const strip = m.edgeRestoreElement()!;
    vi.advanceTimersByTime(1300);
    window.dispatchEvent(new Event('blur'));
    expect(strip.classList.contains('is-hot')).toBe(false);
    expect(strip.classList.contains('is-armed')).toBe(false);
  });

  test('a pointer arriving late in the hold is never left visible-but-unclickable', async () => {
    // The tab is painted by `.is-revealed` for the whole 1.2s. If the pointer
    // arrives at 1.1s its dwell would not elapse until 1.25s — so dropping
    // `.is-revealed` at 1.2s would take `pointer-events` away from a fully
    // painted target for ~150ms, and a click there would fall through to the
    // editor. The dwell exists to stop an INVISIBLE target stealing a scrollbar
    // grab; this one has been on screen for over a second, so there is nothing
    // left to earn.
    const m = await freshEdge();
    setFullWidth(true);
    const strip = m.edgeRestoreElement()!;
    vi.advanceTimersByTime(1100);
    movePointer(IN_ZONE.x, IN_ZONE.y);
    expect(strip.classList.contains('is-armed')).toBe(false); // dwell pending
    vi.advanceTimersByTime(101); // the hold ends at 1200, before the dwell would
    expect(strip.classList.contains('is-revealed')).toBe(false);
    expect(strip.classList.contains('is-armed')).toBe(true);
  });

  test('a pointer parked away from the edge gets the ordinary 1.2s hold', async () => {
    const m = await freshEdge();
    movePointer(200, 300);
    setFullWidth(true);
    const strip = m.edgeRestoreElement()!;
    expect(strip.classList.contains('is-armed')).toBe(false);
    vi.advanceTimersByTime(1300);
    expect(strip.classList.contains('is-revealed')).toBe(false);
  });

  test('the pressed fill is released by a cancelled press, not left latched', async () => {
    const m = await freshEdge();
    setFullWidth(true);
    const strip = m.edgeRestoreElement()!;
    const tab = strip.querySelector('.moss-edge-restore__tab') as HTMLElement;
    tab.dispatchEvent(new Event('pointerdown'));
    expect(strip.classList.contains('is-pressed')).toBe(true);
    // A system gesture takeover, or a release that landed off the tab. Both
    // used to leave the darker fill stuck until the pointer left the zone.
    tab.dispatchEvent(new Event('pointercancel'));
    expect(strip.classList.contains('is-pressed')).toBe(false);
  });

  test('clicking the tab emits the layout button\'s own event', async () => {
    const m = await freshEdge();
    setFullWidth(true);
    const tab = m.edgeRestoreElement()!.querySelector('.moss-edge-restore__tab') as HTMLElement;
    tab.click();
    // The same event the layout button emits — no new command, no new path.
    expect(mockEmit).toHaveBeenCalledWith('editor:request-split');
  });

  test('the tab is a labelled button, and carries no tooltip on top of itself', async () => {
    const m = await freshEdge();
    setFullWidth(true);
    const tab = m.edgeRestoreElement()!.querySelector('.moss-edge-restore__tab') as HTMLElement;
    expect(tab.tagName).toBe('BUTTON');
    expect(tab.getAttribute('aria-label')).toBe('Back to split view'); // t('toolbar.back_to_split')
    expect(tab.hasAttribute('title')).toBe(false);
    // No portal tooltip either. It has no dwell — it paints on the first
    // `mouseover` — so a bubble would appear before the tab it names had begun
    // to fade in. The shell's left strip drops it for the same reason.
    expect(tab.hasAttribute('data-tooltip')).toBe(false);
  });

  test('mount() emits the shape the render gate measures', async () => {
    // The render gate cannot import mount() (it pulls the Tauri event API), so
    // its right-edge fixture is written out by hand. This is what stops the two
    // from drifting: change the tag, the classes or the nesting here and the
    // gate's copy goes stale — so assert them where the real mount() runs.
    const m = await freshEdge();
    setFullWidth(true);
    const strip = m.edgeRestoreElement()!;
    expect(strip.tagName).toBe('DIV'); // inert: the tracker owns hover, not :hover
    // Base classes only — the just-in-time reveal has already added a state one.
    expect(strip.classList.contains('moss-edge-restore')).toBe(true);
    expect(strip.classList.contains('moss-edge-restore--right')).toBe(true);
    const tab = strip.firstElementChild as HTMLElement;
    expect(tab.tagName).toBe('BUTTON');
    expect(tab.className).toBe('moss-edge-restore__tab');
  });
});
