// A canvas, observers and animation frames under the tests' control: jsdom
// ships none of them working.
import { vi } from 'vitest';

export function stubCanvas({ width = 400, height = 300, reduceMotion = false } = {}) {
  const calls: Array<[string, ...number[]]> = [];
  const alphas: number[] = [];
  const ctx: Record<string, unknown> = {};
  for (const m of ['clearRect', 'beginPath', 'moveTo', 'lineTo', 'stroke', 'setTransform']) {
    ctx[m] = vi.fn((...a: number[]) => { calls.push([m, ...a]); });
  }
  Object.defineProperty(ctx, 'globalAlpha', { set: (v: number) => { alphas.push(v); }, get: () => alphas.at(-1) });

  const originalGetContext = HTMLCanvasElement.prototype.getContext;
  const originalMatchMedia = window.matchMedia;
  HTMLCanvasElement.prototype.getContext = vi.fn(() => ctx) as unknown as typeof HTMLCanvasElement.prototype.getContext;
  window.matchMedia = vi.fn(() => ({ matches: reduceMotion }) as unknown as MediaQueryList);
  vi.stubGlobal('ResizeObserver', class {
    constructor(private cb: ResizeObserverCallback) {}
    observe(target: Element) {
      this.cb([{ contentRect: { width, height }, target } as unknown as ResizeObserverEntry], this as unknown as ResizeObserver);
    }
    disconnect() {}
  });
  let next: FrameRequestCallback | null = null;
  const raf = vi.fn((cb: FrameRequestCallback) => { next = cb; return 1; });
  vi.stubGlobal('requestAnimationFrame', raf);
  vi.stubGlobal('cancelAnimationFrame', vi.fn());

  return {
    ctx,
    calls,
    alphas,
    raf,
    /** Runs the pending animation frame. */
    step() { next?.(0); },
    restore() {
      HTMLCanvasElement.prototype.getContext = originalGetContext;
      window.matchMedia = originalMatchMedia;
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
      document.body.innerHTML = '';
      document.documentElement.removeAttribute('data-theme');
    },
  };
}
