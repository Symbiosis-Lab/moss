/**
 * Pointing at one thing in the preview.
 *
 * `revealSource(selector)` is the single primitive: scroll the element into
 * view, run the one-shot wash over it. `revealFooter()` is that primitive
 * aimed at the page footer — the answer to opening `footer.md`, which becomes
 * no page and so cannot be followed — and the shell aims the same primitive at
 * `[data-source-fm="<field>"]` when the user opens a frontmatter chip.
 *
 * The load-bearing behaviors here:
 *   - the `animation` property is CLEARED after the run, so the next idiomorph
 *     morph of the footer cannot replay it;
 *   - `armFooterReveal()` is consumed EXACTLY ONCE. A reveal answers a click;
 *     re-asserting it on every rebuild would yank the preview to the footer
 *     while the user is typing.
 */
import { describe, test, expect, beforeEach, afterEach, vi } from 'vitest';

vi.mock("../../log", () => ({
  uiEvent: vi.fn(),
  info: vi.fn(), warn: vi.fn(), error: vi.fn(), debug: vi.fn(),
}));
vi.mock('../../engine', () => ({
  engine: {
    resolvePageSource: vi.fn().mockResolvedValue({
      status: 'ok',
      data: { source_path: null, is_dir: false, is_article: false },
    }),
    listPages: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  },
}));
vi.mock('../../context/event-bus', () => ({
  eventBus: { emit: vi.fn() },
  Events: { PREVIEW_URL_CHANGED: 'preview-url-changed' },
}));

import { NavigationManager } from '../navigation-manager';
import { host } from "../../host";

// No native swipe stream: the indicator falls back to the bridge's wheel heuristic.
host.swipePhases = async () => false;

const FOOTER = 'body > footer[data-source-none]';

function mkManager(): { nav: NavigationManager; rpc: ReturnType<typeof vi.fn> } {
  document.body.innerHTML = `
    <button id="btn-nav-back"></button>
    <button id="btn-nav-forward"></button>
    <span id="moss-url-text">/</span>
    <div class="moss-device-toggle"></div>
    <div class="moss-preview-container"></div>
  `;
  const iframe = document.createElement('iframe');
  // Navigation URLs resolve against the iframe's current origin.
  iframe.src = 'http://localhost:1234/';
  document.body.appendChild(iframe);
  const nav = new NavigationManager(iframe);
  const rpc = vi.fn().mockResolvedValue(undefined);
  // @ts-ignore — private rpc, same access pattern as navigation-manager.test.ts
  nav.rpc.call = rpc;
  return { nav, rpc };
}

/** Drive a bridge navigation echo the way the real iframe does. */
function echoNavigation(url: string): void {
  window.dispatchEvent(
    new MessageEvent('message', { data: { type: 'moss-navigation', url, title: '' } }),
  );
}

describe('NavigationManager.revealSource', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
    document.body.innerHTML = '';
  });

  test('scrolls the footer into view, then washes it', async () => {
    const { nav, rpc } = mkManager();

    await nav.revealFooter();

    expect(rpc).toHaveBeenNthCalledWith(1, 'element.scrollIntoView', FOOTER);
    expect(rpc).toHaveBeenNthCalledWith(
      2,
      'element.setStyle',
      FOOTER,
      'animation',
      expect.stringContaining('moss-slot-reveal'),
    );
  });

  test('clears the animation property once the wash is over', async () => {
    const { nav, rpc } = mkManager();

    await nav.revealFooter();
    rpc.mockClear();
    await vi.runAllTimersAsync();

    expect(rpc).toHaveBeenCalledWith('element.setStyle', FOOTER, 'animation', '');
  });

  test('a page with no footer is a no-op, not a stuck style', async () => {
    const { nav, rpc } = mkManager();
    rpc.mockRejectedValue(new Error('Element not found'));

    await expect(nav.revealFooter()).resolves.toBeUndefined();

    rpc.mockClear();
    await vi.runAllTimersAsync();
    // Nothing was ever set, so nothing is scheduled to be cleared.
    expect(rpc).not.toHaveBeenCalled();
  });

  test('no iframe means no RPC and no throw', async () => {
    const nav = new NavigationManager(null);
    await expect(nav.revealFooter()).resolves.toBeUndefined();
  });

  test('aims at any selector, so a frontmatter field gets the footer treatment', async () => {
    const { nav, rpc } = mkManager();
    const sel = '[data-source-fm="byline"]';

    await nav.revealSource(sel);

    expect(rpc).toHaveBeenNthCalledWith(1, 'element.scrollIntoView', sel);
    expect(rpc).toHaveBeenNthCalledWith(
      2,
      'element.setStyle',
      sel,
      'animation',
      expect.stringContaining('moss-slot-reveal'),
    );
  });
});

// The hover strength of the same primitive: chip hover → class-based flash on
// EVERY matching element, never a scroll (a hover is a glance; the element may
// be off-viewport and must stay there).
describe('NavigationManager.flashSource', () => {
  afterEach(() => {
    document.body.innerHTML = '';
  });

  test('adds the flash class to all matches and never scrolls', async () => {
    const { nav, rpc } = mkManager();
    const sel = '[data-source-fm="date"]';

    await nav.flashSource(sel);

    expect(rpc).toHaveBeenCalledTimes(1);
    expect(rpc).toHaveBeenCalledWith('element.classAll', sel, 'moss-fm-flash', true);
  });

  test('clearFlashSource removes the class (hover-out)', async () => {
    const { nav, rpc } = mkManager();
    const sel = '[data-source-fm="date"]';

    await nav.clearFlashSource(sel);

    expect(rpc).toHaveBeenCalledWith('element.classAll', sel, 'moss-fm-flash', false);
  });

  test('no bridge means no throw', async () => {
    const nav = new NavigationManager(null);
    await expect(nav.flashSource('[data-source-fm="x"]')).resolves.toBeUndefined();
    await expect(nav.clearFlashSource('[data-source-fm="x"]')).resolves.toBeUndefined();
  });
});

describe('NavigationManager.armFooterReveal', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
    document.body.innerHTML = '';
  });

  test('fires on the next navigation echo', async () => {
    const { nav, rpc } = mkManager();

    nav.armFooterReveal('/zh-hans/');
    echoNavigation('http://localhost:1234/zh-hans/');
    await vi.advanceTimersByTimeAsync(0);

    expect(rpc).toHaveBeenCalledWith('element.scrollIntoView', FOOTER);
  });

  test('is consumed once — a later rebuild echo does not re-reveal', async () => {
    const { nav, rpc } = mkManager();

    nav.armFooterReveal('/zh-hans/');
    echoNavigation('http://localhost:1234/zh-hans/');
    await vi.advanceTimersByTimeAsync(0);
    rpc.mockClear();

    // The rebuild that the footer edit itself triggers.
    echoNavigation('http://localhost:1234/zh-hans/');
    await vi.advanceTimersByTimeAsync(0);

    expect(rpc).not.toHaveBeenCalledWith('element.scrollIntoView', FOOTER);
  });

  test('a no-op navigation reveals immediately instead of leaking the arm', async () => {
    const { nav, rpc } = mkManager();
    // @ts-ignore — private iframe; navigation resolves against its current src.
    const iframe = nav.iframe as HTMLIFrameElement;
    nav.navigateToPage('/zh-hans/');
    const landedOn = iframe.src;

    nav.armFooterReveal('/zh-hans/');
    // Same URL: `iframe.src` never changes, so no bridge echo will arrive.
    // Without settling the arm here it would fire on whatever the user clicked
    // next, minutes later.
    nav.navigateToPage('/zh-hans/');
    expect(iframe.src).toBe(landedOn);
    await vi.advanceTimersByTimeAsync(0);

    expect(rpc).toHaveBeenCalledWith('element.scrollIntoView', FOOTER);

    rpc.mockClear();
    echoNavigation('http://localhost:1234/about/');
    await vi.advanceTimersByTimeAsync(0);
    expect(rpc).not.toHaveBeenCalledWith('element.scrollIntoView', FOOTER);
  });

  test('a superseded navigation does not steal the reveal', async () => {
    const { nav, rpc } = mkManager();

    // Open footer.md → arm for the tree home; then click an article before the
    // home has loaded. Exactly one echo arrives, and it is the article's.
    nav.armFooterReveal('/');
    echoNavigation('http://localhost:1234/posts/hello/');
    await vi.advanceTimersByTimeAsync(0);

    // Flashing the article's footer would be an answer to a question the user
    // never asked.
    expect(rpc).not.toHaveBeenCalledWith('element.scrollIntoView', FOOTER);
  });

  test('an arm whose navigation never echoes expires instead of waiting forever', async () => {
    const { nav, rpc } = mkManager();

    nav.armFooterReveal('/zh-hans/');
    // Nothing echoes for the armed URL; the user browses elsewhere later.
    await vi.advanceTimersByTimeAsync(5000);
    echoNavigation('http://localhost:1234/about/');
    await vi.advanceTimersByTimeAsync(0);
    expect(rpc).not.toHaveBeenCalledWith('element.scrollIntoView', FOOTER);

    // And the retired arm cannot fire even if its own URL shows up much later.
    echoNavigation('http://localhost:1234/zh-hans/');
    await vi.advanceTimersByTimeAsync(0);
    expect(rpc).not.toHaveBeenCalledWith('element.scrollIntoView', FOOTER);
  });

  test('an unarmed navigation never reveals', async () => {
    const { nav, rpc } = mkManager();

    echoNavigation('http://localhost:1234/about/');
    await vi.advanceTimersByTimeAsync(0);

    expect(rpc).not.toHaveBeenCalledWith('element.scrollIntoView', FOOTER);
  });
});
