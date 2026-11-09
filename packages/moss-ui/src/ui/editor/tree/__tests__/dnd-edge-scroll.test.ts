/**
 * Unit tests for the edge-scroll logic (updateEdgeScroll / stopEdgeScroll /
 * scheduleEdgeScroll) in tree/dnd.ts's DropFeedback, driven through FileTree.
 *
 * Strategy: jsdom has no layout engine so getBoundingClientRect returns zeroes
 * by default. We mock it to return a known rect for the navBody element, then
 * invoke updateEdgeScroll with a Y value inside/outside the EDGE_ZONE and
 * verify behaviour via scrollTop mutations and timer counts.
 *
 * DropFeedback.scheduleEdgeScroll performs ONE immediate scroll step, then sets a timer for
 * the next step. So after one call to updateEdgeScroll in the edge zone:
 *   - scrollTop changes immediately (first step)
 *   - advancing by SCROLL_INTERVAL causes the second step
 *
 * mount() may set its own timers (spineInitTimer etc.), so we capture the
 * timer baseline after mount and before any edge-scroll calls.
 *
 * We use vi.useFakeTimers() so we can advance time without real I/O.
 */

import { describe, test, expect, vi, beforeEach, afterEach } from 'vitest';
import { installPointerEventPolyfill, dir, file } from './test-helpers';

installPointerEventPolyfill();

// ---------------------------------------------------------------------------
// Minimal mocks required to import FileTree without DOM/tauri side-effects
// ---------------------------------------------------------------------------
const { mockListTree, mockGetProjectPath } = vi.hoisted(() => ({
  mockListTree: vi.fn(),
  mockGetProjectPath: vi.fn(),
}));

vi.mock('../../../engine', () => ({
  engine: {
    listTree: mockListTree,
    getProjectPath: mockGetProjectPath,
    renameEntry: vi.fn(),
    renameEntryWithRefs: vi.fn(),
    copyFilesToProject: vi.fn(),
    deleteEntry: vi.fn(),
    revealEntry: vi.fn(),
    duplicateEntry: vi.fn(),
    createFiles: vi.fn(),
    createFolder: vi.fn(),
    moveFile: vi.fn(),
    moveEntries: vi.fn(),
    resolveUrlForFile: vi.fn().mockResolvedValue({ status: 'ok', data: '/' }),
  },
}));


if (typeof globalThis.CSS === 'undefined') {
  (globalThis as unknown as { CSS: { escape(s: string): string } }).CSS = {
    escape: (s: string) => s.replace(/(["\\])/g, '\\$1'),
  };
}

import { FileTree } from '../controller';
import { EntryRegistry } from '../../entry-registry';

// ---------------------------------------------------------------------------
// Test constants that mirror the private statics in tree/dnd.ts
// ---------------------------------------------------------------------------
const EDGE_ZONE = 48;    // tree/dnd.ts EDGE_ZONE
const SCROLL_STEP = 8;   // tree/dnd.ts SCROLL_STEP
const SCROLL_INTERVAL = 16; // tree/dnd.ts SCROLL_INTERVAL

const ROOT = '/Users/me/Sites/blog';

// navBody rect used in all tests: top=100, bottom=700 (height=600)
const NAV_RECT = { top: 100, bottom: 700, left: 0, right: 300, width: 300, height: 600 };

function makeTree(files: string[]) {
  return dir(ROOT, files.map((name) => file(`${ROOT}/${name}`)), { name: 'blog' });
}

// ---------------------------------------------------------------------------
// Shared setup
// ---------------------------------------------------------------------------

let registry: EntryRegistry;
let fileTree: FileTree;
let container: HTMLElement;
let navBody: HTMLElement;
/** Timer count right after mount, before any edge-scroll calls. */
let baselineTimerCount: number;

async function mountTree() {
  vi.clearAllMocks();
  mockGetProjectPath.mockResolvedValue({ status: 'ok', data: ROOT });
  mockListTree.mockResolvedValue({ status: 'ok', data: makeTree(['index.md', 'about.md']) });

  registry = new EntryRegistry();
  fileTree = new FileTree(registry);
  container = document.createElement('div');
  document.body.appendChild(container);
  await fileTree.mount(container, ROOT, false);

  // Grab the navBody that mount() created inside container.
  navBody = container.querySelector('.nav-body') as HTMLElement;
  expect(navBody).not.toBeNull();

  // Give navBody a known layout so edge-zone math works.
  vi.spyOn(navBody, 'getBoundingClientRect').mockReturnValue(NAV_RECT as DOMRect);

  // Capture baseline after mount so we can detect edge-scroll timers relative
  // to whatever mount() itself schedules (spineInitTimer etc.).
  baselineTimerCount = vi.getTimerCount();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('edge-scroll: updateEdgeScroll direction and scheduling', () => {
  beforeEach(async () => {
    vi.useFakeTimers();
    await mountTree();
  });

  afterEach(() => {
    fileTree.unmount();
    container.remove();
    vi.useRealTimers();
  });

  // ── Edge zones keep their direction ────────────────────────────────────────────────

  test('updateEdgeScroll in the top EDGE_ZONE schedules a scroll tick (direction preserved, not zeroed)', () => {
    // Y just inside the TOP edge zone: clientY < rect.top + EDGE_ZONE
    const clientY = NAV_RECT.top + EDGE_ZONE - 1; // 147

    fileTree.updateEdgeScroll(clientY);

    // A timer must have been scheduled — if the direction were zeroed, stopEdgeScroll()
    // inside updateEdgeScroll would have zeroed the direction, causing
    // scheduleEdgeScroll's guard to bail, and no extra setTimeout would fire.
    expect(vi.getTimerCount()).toBeGreaterThan(baselineTimerCount);
  });

  test('updateEdgeScroll in the bottom EDGE_ZONE schedules a scroll tick', () => {
    // Y just inside the BOTTOM edge zone: clientY > rect.bottom - EDGE_ZONE
    const clientY = NAV_RECT.bottom - EDGE_ZONE + 1; // 653

    fileTree.updateEdgeScroll(clientY);

    expect(vi.getTimerCount()).toBeGreaterThan(baselineTimerCount);
  });

  test('updateEdgeScroll outside the edge zone does NOT add extra scroll timers', () => {
    // Y in the middle of the tree — no edge zone
    const clientY = (NAV_RECT.top + NAV_RECT.bottom) / 2; // 400

    fileTree.updateEdgeScroll(clientY);

    // No new timers added beyond the mount baseline
    expect(vi.getTimerCount()).toBe(baselineTimerCount);
  });

  // ── Immediate first scroll step ─────────────────────────────────────────
  // scheduleEdgeScroll scrolls immediately (first step) then sets a timer for
  // subsequent steps.

  test('top zone: updateEdgeScroll immediately decreases scrollTop by SCROLL_STEP', () => {
    navBody.scrollTop = 200;
    const clientY = NAV_RECT.top + EDGE_ZONE - 1; // top zone → dir = -1

    fileTree.updateEdgeScroll(clientY);

    // dir = -1 → first scroll step happens immediately inside scheduleEdgeScroll
    expect(navBody.scrollTop).toBe(200 - SCROLL_STEP);
  });

  test('bottom zone: updateEdgeScroll immediately increases scrollTop by SCROLL_STEP', () => {
    navBody.scrollTop = 200;
    const clientY = NAV_RECT.bottom - EDGE_ZONE + 1; // bottom zone → dir = 1

    fileTree.updateEdgeScroll(clientY);

    // dir = 1 → first scroll step happens immediately
    expect(navBody.scrollTop).toBe(200 + SCROLL_STEP);
  });

  test('advancing timer by SCROLL_INTERVAL causes a SECOND scroll step in the bottom zone', () => {
    navBody.scrollTop = 200;
    const clientY = NAV_RECT.bottom - EDGE_ZONE + 1;

    fileTree.updateEdgeScroll(clientY);
    // After immediate first step: scrollTop = 208
    expect(navBody.scrollTop).toBe(200 + SCROLL_STEP);

    vi.advanceTimersByTime(SCROLL_INTERVAL);
    // After timer fires (second step): scrollTop = 216
    expect(navBody.scrollTop).toBe(200 + SCROLL_STEP * 2);
  });

  test('scroll repeats on successive timer ticks', () => {
    navBody.scrollTop = 1000;
    const clientY = NAV_RECT.bottom - EDGE_ZONE + 1; // bottom zone → dir = 1

    fileTree.updateEdgeScroll(clientY);
    // Immediate first step: 1000 + 8 = 1008
    // 3 more timer ticks = 3 more steps: 1008 + 24 = 1032
    vi.advanceTimersByTime(SCROLL_INTERVAL * 3);
    expect(navBody.scrollTop).toBe(1000 + SCROLL_STEP * 4);
  });

  // ── stopEdgeScroll ──────────────────────────────────────────────────────

  test('stopEdgeScroll cancels the timer and prevents further scrolling', () => {
    navBody.scrollTop = 200;
    const clientY = NAV_RECT.bottom - EDGE_ZONE + 1;

    fileTree.updateEdgeScroll(clientY);
    // First step scrolled immediately: scrollTop = 208
    const scrollAfterFirst = navBody.scrollTop;

    fileTree.stopEdgeScroll();

    // After stopping, advancing time should NOT add more scroll steps.
    vi.advanceTimersByTime(SCROLL_INTERVAL * 5);
    expect(navBody.scrollTop).toBe(scrollAfterFirst);

    // No new timers beyond what the advancing itself would leave behind —
    // the edge-scroll timer must have been cancelled. We verify behavior
    // (no additional scrolling) rather than counting timers here, because
    // vi.advanceTimersByTime() also fires/removes baseline mount timers.
  });

  test('stopEdgeScroll is idempotent (no throw when called twice)', () => {
    const clientY = NAV_RECT.bottom - EDGE_ZONE + 1;
    fileTree.updateEdgeScroll(clientY);
    expect(() => {
      fileTree.stopEdgeScroll();
      fileTree.stopEdgeScroll();
    }).not.toThrow();
  });

  // ── Moving from one edge zone to another ────────────────────────────────

  test('moving from top zone to bottom zone: direction changes, scroll continues upward', () => {
    navBody.scrollTop = 500;
    // Start in top zone (dir = -1) → immediate step: 500 - 8 = 492
    fileTree.updateEdgeScroll(NAV_RECT.top + EDGE_ZONE - 1);
    expect(navBody.scrollTop).toBe(500 - SCROLL_STEP);

    // Move to bottom zone (dir = 1) → should cancel previous timer,
    // reset direction, start fresh → immediate step on current scrollTop
    const beforeSwitch = navBody.scrollTop;
    fileTree.updateEdgeScroll(NAV_RECT.bottom - EDGE_ZONE + 1);
    expect(navBody.scrollTop).toBe(beforeSwitch + SCROLL_STEP);

    // Advance one timer tick — should see one more step in the +1 (down) direction
    const beforeTick = navBody.scrollTop;
    vi.advanceTimersByTime(SCROLL_INTERVAL);
    expect(navBody.scrollTop).toBe(beforeTick + SCROLL_STEP);
  });

  // ── Calling updateEdgeScroll twice with same position is idempotent ─────

  test('calling updateEdgeScroll twice with same zone does not double-schedule', () => {
    const clientY = NAV_RECT.bottom - EDGE_ZONE + 1;
    navBody.scrollTop = 0;

    fileTree.updateEdgeScroll(clientY); // dir 0→1, schedules timer, immediate scroll
    const countAfterFirst = vi.getTimerCount();
    const scrollAfterFirst = navBody.scrollTop;

    fileTree.updateEdgeScroll(clientY); // same dir, no change → no re-schedule
    expect(vi.getTimerCount()).toBe(countAfterFirst); // same count
    expect(navBody.scrollTop).toBe(scrollAfterFirst); // no extra immediate scroll
  });

  // ── Moving OUT of edge zone stops scroll ────────────────────────────────

  test('moving pointer to the middle zone (non-edge) stops the scroll', () => {
    navBody.scrollTop = 200;
    const clientY = NAV_RECT.bottom - EDGE_ZONE + 1;
    fileTree.updateEdgeScroll(clientY);
    const timerAfterStart = vi.getTimerCount();
    expect(timerAfterStart).toBeGreaterThan(baselineTimerCount);

    // Move to middle → dir becomes 0 → stopEdgeScroll() called internally
    fileTree.updateEdgeScroll((NAV_RECT.top + NAV_RECT.bottom) / 2);

    // Edge-scroll timer must be gone; only baseline timers remain
    expect(vi.getTimerCount()).toBe(baselineTimerCount);
  });
});

// ── Visibility/public API ──────────────────────────────────────────────────

describe('edge-scroll: public API is accessible from editor-main.ts', () => {
  beforeEach(async () => {
    vi.useFakeTimers();
    await mountTree();
  });

  afterEach(() => {
    fileTree.unmount();
    container.remove();
    vi.useRealTimers();
  });

  test('updateEdgeScroll is a public method on FileTree', () => {
    expect(typeof fileTree.updateEdgeScroll).toBe('function');
  });

  test('stopEdgeScroll is a public method on FileTree', () => {
    expect(typeof fileTree.stopEdgeScroll).toBe('function');
  });

  test('calling updateEdgeScroll from "editor-main" side (via fileTree ref) works correctly', () => {
    navBody.scrollTop = 300;

    // Simulate what editor-main does after receiving a Tauri 'over' event
    // with logicalY in the bottom edge zone:
    const logicalY = NAV_RECT.bottom - EDGE_ZONE + 1;
    fileTree.updateEdgeScroll(logicalY);
    expect(vi.getTimerCount()).toBeGreaterThan(baselineTimerCount);
    expect(navBody.scrollTop).toBe(300 + SCROLL_STEP); // immediate first step

    // Simulate editor-main calling stopEdgeScroll on Tauri 'drop' or 'leave':
    fileTree.stopEdgeScroll();
    expect(vi.getTimerCount()).toBe(baselineTimerCount);

    // No further scrolling after stop
    vi.advanceTimersByTime(SCROLL_INTERVAL * 10);
    expect(navBody.scrollTop).toBe(300 + SCROLL_STEP);
  });
});
