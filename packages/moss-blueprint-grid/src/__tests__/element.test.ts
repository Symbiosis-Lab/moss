// <blueprint-grid>: the element starts the grid on its own canvas, follows the
// pointer anywhere in the window, and takes its colour from the page.
import { describe, test, expect, vi, afterEach } from 'vitest';
import '../element.js';
import { stubCanvas } from './stub-canvas.js';

describe('<blueprint-grid>', () => {
  let fx: ReturnType<typeof stubCanvas>;
  afterEach(() => fx.restore());

  function mount(attrs: Record<string, string> = {}) {
    const el = document.createElement('blueprint-grid');
    for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, v);
    document.body.append(el);
    return el;
  }

  test('draws on a canvas in its shadow root, hidden from assistive technology', () => {
    fx = stubCanvas({ reduceMotion: true });
    const el = mount();
    expect(el.shadowRoot?.querySelector('canvas')).not.toBeNull();
    expect(el.getAttribute('aria-hidden')).toBe('true');
  });

  test('listens for the pointer and touches on the window, so it answers from behind the content', () => {
    fx = stubCanvas({ reduceMotion: true });
    const add = vi.spyOn(window, 'addEventListener');
    mount();
    expect(add.mock.calls.map((c) => c[0])).toEqual(expect.arrayContaining(['pointermove', 'touchmove']));
  });

  test('takes its colour from the color attribute', () => {
    fx = stubCanvas({ reduceMotion: true });
    mount({ color: 'rgb(9, 8, 7)' });
    expect(fx.ctx.strokeStyle).toBe('rgb(9, 8, 7)');
  });

  test('without the attribute, follows --blueprint-grid-color as the page changes it', () => {
    fx = stubCanvas();
    const el = mount();
    el.style.setProperty('--blueprint-grid-color', 'rgb(4, 5, 6)');
    fx.step();
    expect(fx.ctx.strokeStyle).toBe('rgb(4, 5, 6)');
    el.style.setProperty('--blueprint-grid-color', 'rgb(7, 7, 7)');
    fx.step();
    expect(fx.ctx.strokeStyle).toBe('rgb(7, 7, 7)');
  });

  test('reads max-alpha', () => {
    fx = stubCanvas({ reduceMotion: true });
    vi.spyOn(Math, 'random').mockReturnValue(0.5);
    mount({ 'max-alpha': '0.05' });
    expect(Math.max(...fx.alphas)).toBeLessThanOrEqual(0.05);
  });

  test('stops when removed', () => {
    fx = stubCanvas({ reduceMotion: true });
    const el = mount();
    const remove = vi.spyOn(window, 'removeEventListener');
    el.remove();
    expect(remove.mock.calls.map((c) => c[0])).toEqual(expect.arrayContaining(['pointermove', 'touchmove']));
  });
});
