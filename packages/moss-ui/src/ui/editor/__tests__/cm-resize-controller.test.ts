import { describe, test, expect, vi } from 'vitest';
import { createResizeController, clampPercent } from '../cm-resize-controller';

function setup(onCommit = vi.fn()) {
  const wrapper = document.createElement('div');
  // jsdom has no layout — stub the start width (300px) the gesture reads.
  (wrapper as any).getBoundingClientRect = () => ({ width: 300, height: 200, left: 0, top: 0, right: 300, bottom: 200, x: 0, y: 0, toJSON() {} });
  const rc = createResizeController({ wrapper, getContentWidth: () => 600, onCommit });
  wrapper.appendChild(rc.handle);
  document.body.appendChild(wrapper);
  return { wrapper, rc, onCommit };
}
const down = (el: HTMLElement, x: number) => el.dispatchEvent(new MouseEvent('pointerdown', { clientX: x, bubbles: true, cancelable: true }));
const move = (x: number) => window.dispatchEvent(new MouseEvent('pointermove', { clientX: x }));
const up = (x: number) => window.dispatchEvent(new MouseEvent('pointerup', { clientX: x }));

describe('clampPercent (drag math)', () => {
  test('rounds (startW + dx)/contentW and clamps to [10,100]', () => {
    expect(clampPercent(300, 60, 600)).toBe(60);
    expect(clampPercent(600, 400, 600)).toBe(100);
    expect(clampPercent(10, -100, 600)).toBe(10);
  });
});

describe('resize controller', () => {
  test('handle has the corner-bracket SVG + aria-label', () => {
    const { rc } = setup();
    expect(rc.handle.classList.contains('cm-lp-image-handle')).toBe(true);
    expect(rc.handle.getAttribute('aria-label')).toBe('Resize image');
    expect(rc.handle.querySelector('svg')).toBeTruthy();
  });

  test('drag shows a live badge and commits the final percent once on release', () => {
    const { wrapper, onCommit } = setup();
    down(wrapper.querySelector('.cm-lp-image-handle')!, 100);
    move(160); // dx=60 → (300+60)/600 = 60%
    expect(wrapper.style.width).toBe('60%');
    expect(wrapper.querySelector('.cm-lp-image-badge')?.textContent).toBe('60%');
    up(160);
    expect(wrapper.querySelector('.cm-lp-image-badge')).toBeNull(); // badge removed
    expect(onCommit).toHaveBeenCalledTimes(1);
    expect(onCommit).toHaveBeenCalledWith('60%');
  });

  test("a drag suspends the wrapper's own max-width, and gives it back", () => {
    // A drag writes style.width directly, so a max-width on the same element
    // silently fights it: pointer and badge keep moving while the picture stops
    // growing, which reads as a stuck handle. The editor's image height cap
    // is such a max-width, and a resize is the one moment the user is
    // deliberately asking to see the size.
    const { wrapper } = setup();
    wrapper.style.maxWidth = '133px';
    down(wrapper.querySelector('.cm-lp-image-handle')!, 100);
    expect(wrapper.style.maxWidth).toBe('none');
    move(160);
    up(160);
    expect(wrapper.style.maxWidth).toBe('133px');
  });

  test('a bare click (no movement) commits nothing', () => {
    const { wrapper, onCommit } = setup();
    down(wrapper.querySelector('.cm-lp-image-handle')!, 100);
    up(100);
    expect(onCommit).not.toHaveBeenCalled();
  });

  test('double bare-click resets (one onCommit(null))', () => {
    const { wrapper, onCommit } = setup();
    const h = wrapper.querySelector('.cm-lp-image-handle') as HTMLElement;
    down(h, 100); up(100);   // first click — no commit
    down(h, 100); up(100);   // second click within window — reset
    expect(onCommit).toHaveBeenCalledTimes(1);
    expect(onCommit).toHaveBeenCalledWith(null);
  });

  test('movement threshold: 2px is a click (no commit), 3px is a drag (commit)', () => {
    const a = setup();
    down(a.wrapper.querySelector('.cm-lp-image-handle')!, 100);
    move(102); // dx=2 (< 3) → not a move
    up(102);
    expect(a.onCommit).not.toHaveBeenCalled();

    const b = setup();
    down(b.wrapper.querySelector('.cm-lp-image-handle')!, 100);
    move(103); // dx=3 (>= 3) → a move
    up(103);
    expect(b.onCommit).toHaveBeenCalledTimes(1);
  });

  test('a drag followed by a quick single click does NOT reset', () => {
    const { wrapper, onCommit } = setup();
    const h = wrapper.querySelector('.cm-lp-image-handle') as HTMLElement;
    down(h, 100); move(160); up(160);   // drag → commit '60%', clears double-click state
    down(h, 100); up(100);              // immediate no-move click → must NOT reset
    expect(onCommit).toHaveBeenCalledTimes(1);
    expect(onCommit).toHaveBeenCalledWith('60%');
    expect(onCommit).not.toHaveBeenCalledWith(null);
  });

  test('window listeners are removed on release (no leak after up)', () => {
    const { wrapper } = setup();
    const h = wrapper.querySelector('.cm-lp-image-handle') as HTMLElement;
    down(h, 100); move(160); up(160);   // wrapper.style.width now '60%'
    move(400);                          // stray move AFTER up — must be ignored
    expect(wrapper.style.width).toBe('60%'); // unchanged → listener was removed
  });
});
