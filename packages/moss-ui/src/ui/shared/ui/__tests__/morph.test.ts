/**
 * Unit tests for the height-morph primitive (ui/morph.ts).
 *
 * jsdom has no layout engine, so `getBoundingClientRect()` is stubbed to feed
 * the before/after heights. What is actually under test is the STYLE
 * BOOKKEEPING: a leaked inline `height` would freeze the modal at one phase's
 * size forever, which is the regression that will really happen.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { morphHeight, prefersReducedMotion, crossfadeText } from '../morph';

/** Queue of heights returned by successive getBoundingClientRect() calls. */
function stubHeights(el: HTMLElement, heights: number[]): void {
  let i = 0;
  el.getBoundingClientRect = () => {
    const height = heights[Math.min(i++, heights.length - 1)];
    return { height, width: 0, top: 0, left: 0, right: 0, bottom: height, x: 0, y: 0, toJSON() {} } as DOMRect;
  };
}

function setReducedMotion(matches: boolean): void {
  vi.stubGlobal(
    'matchMedia',
    vi.fn().mockReturnValue({ matches, media: '', addEventListener() {}, removeEventListener() {} }),
  );
}

let el: HTMLElement;

beforeEach(() => {
  vi.useFakeTimers();
  setReducedMotion(false);
  document.body.innerHTML = '';
  el = document.createElement('div');
  document.body.appendChild(el);
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  document.body.innerHTML = '';
});

describe('morphHeight', () => {
  it('runs mutate() exactly once and its DOM changes survive the morph', async () => {
    stubHeights(el, [100, 160]);
    const mutate = vi.fn(() => {
      el.appendChild(document.createElement('span'));
    });

    const done = morphHeight(el, mutate);
    await vi.advanceTimersByTimeAsync(500);
    await done;

    expect(mutate).toHaveBeenCalledTimes(1);
    expect(el.querySelectorAll('span')).toHaveLength(1);
  });

  it('writes the target height during the transition and clears it afterwards', async () => {
    stubHeights(el, [100, 160]);

    const done = morphHeight(el, () => {});
    // Mid-flight: the element is pinned to the target height.
    expect(el.style.height).toBe('160px');

    await vi.advanceTimersByTimeAsync(500);
    await done;
    // A leaked inline height would freeze the modal — it must be gone.
    expect(el.style.height).toBe('');
  });

  it('restores overflow after the morph', async () => {
    stubHeights(el, [100, 160]);

    const done = morphHeight(el, () => {});
    expect(el.style.overflow).toBe('hidden');

    await vi.advanceTimersByTimeAsync(500);
    await done;
    expect(el.style.overflow).toBe('');
  });

  it('resolves a height-only transitionend on itself, ignoring bubbled ones', async () => {
    stubHeights(el, [100, 160]);
    const child = document.createElement('div');
    el.appendChild(child);

    const done = morphHeight(el, () => {});

    // A child's opacity transition (revealed row) bubbles through the modal.
    child.dispatchEvent(new Event('transitionend', { bubbles: true }));
    expect(el.style.height).toBe('160px'); // still in flight

    const ev = new Event('transitionend', { bubbles: true }) as TransitionEvent;
    Object.defineProperty(ev, 'propertyName', { value: 'height' });
    el.dispatchEvent(ev);
    await done;
    expect(el.style.height).toBe('');
  });

  it('writes no styles at all under reduced motion', async () => {
    setReducedMotion(true);
    stubHeights(el, [100, 160]);

    await morphHeight(el, () => {});

    expect(prefersReducedMotion()).toBe(true);
    expect(el.getAttribute('style')).toBeNull();
  });

  it('leaves no styles behind when a second morph retargets an in-flight one', async () => {
    stubHeights(el, [100, 160, 160, 220]);

    const first = morphHeight(el, () => {});
    const second = morphHeight(el, () => {});
    expect(el.style.height).toBe('220px');

    await vi.advanceTimersByTimeAsync(500);
    await Promise.all([first, second]);

    expect(el.style.height).toBe('');
    expect(el.style.overflow).toBe('');
  });
});

/** A minimal fake `Element.animate`, shaped enough for `crossfadeText`: it
 *  reads only `.finished` (a promise) and calls `.cancel()` on the fade-out
 *  once the text swaps. Each call is recorded so a test can inspect the
 *  keyframes/options crossfadeText asked for, in order. */
function fakeAnimate(el: HTMLElement): { calls: { keyframes: unknown; options: KeyframeAnimationOptions }[]; cancel: ReturnType<typeof vi.fn>; resolveFadeOut: () => void } {
  const calls: { keyframes: unknown; options: KeyframeAnimationOptions }[] = [];
  const cancel = vi.fn();
  let resolveFadeOut!: () => void;
  const fadeOutFinished = new Promise<void>((resolve) => { resolveFadeOut = resolve; });
  let call = 0;
  (el as unknown as { animate: typeof el.animate }).animate = ((keyframes: unknown, options: KeyframeAnimationOptions) => {
    calls.push({ keyframes, options });
    call += 1;
    return call === 1
      ? { finished: fadeOutFinished, cancel }
      : { finished: Promise.resolve(), cancel: vi.fn() };
  }) as unknown as typeof el.animate;
  return { calls, cancel, resolveFadeOut };
}

describe('crossfadeText', () => {
  it('sets the text synchronously without a real Element.animate (jsdom)', async () => {
    expect(typeof el.animate).not.toBe('function');
    await crossfadeText(el, 'Editor');
    expect(el.textContent).toBe('Editor');
  });

  it('sets the text synchronously under reduced motion even when animate exists', async () => {
    setReducedMotion(true);
    const { calls } = fakeAnimate(el);
    await crossfadeText(el, 'Editor');
    expect(el.textContent).toBe('Editor');
    expect(calls).toHaveLength(0);
  });

  it('fades out, swaps the text, cancels the fade-out, then fades in with no fill', async () => {
    const { calls, cancel, resolveFadeOut } = fakeAnimate(el);

    const done = crossfadeText(el, 'Editor');
    // Still mid fade-out: exactly one animate() call, text unchanged, and
    // the fade-out has not been cancelled yet.
    expect(calls).toHaveLength(1);
    expect(calls[0]!.keyframes).toEqual([{ opacity: 1 }, { opacity: 0 }]);
    expect(calls[0]!.options).toMatchObject({ duration: 140, fill: 'forwards' });
    expect(el.textContent).toBe('');
    expect(cancel).not.toHaveBeenCalled();

    resolveFadeOut();
    await done;

    // Swap, then cancel the fade-out (releasing its pinned opacity:0) before
    // the fade-in starts — and the fade-in carries no `fill`, so the node
    // ends on its own stylesheet opacity rather than a pinned inline one.
    expect(el.textContent).toBe('Editor');
    expect(cancel).toHaveBeenCalledTimes(1);
    expect(calls).toHaveLength(2);
    expect(calls[1]!.keyframes).toEqual([{ opacity: 0 }, { opacity: 1 }]);
    expect(calls[1]!.options).toMatchObject({ duration: 160 });
    expect(calls[1]!.options.fill).toBeUndefined();
  });
});
