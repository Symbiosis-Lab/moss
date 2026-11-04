// The grid's physics, line reveal and canvas loop.

import { describe, test, expect, vi, afterEach } from 'vitest';
import {
  createParticles,
  updatePhysics,
  startBlueprintGrid,
  pointerPoint,
  type Particle,
  type BlueprintGridConfig,
} from '../index.js';
import { stubCanvas } from './stub-canvas.js';

const DEFAULT_CONFIG: BlueprintGridConfig = {
  particleCount: 12,
  speed: 0.0025,
  repulsion: 0.0000002,
  attraction: 0.006,
  dampening: 0.985,
  gridSpacing: 40,
  sigmaPerp: 0.018,
  sigmaParallel: 0.18,
  segmentLength: 8,
  minLineWidth: 0.1,
  maxLineWidth: 0.4,
};

describe('createParticles', () => {
  test('creates the requested number of particles', () => {
    const particles = createParticles(10, DEFAULT_CONFIG.speed);
    expect(particles).toHaveLength(10);
  });

  test('all particles are within [0, 1] bounds', () => {
    const particles = createParticles(50, DEFAULT_CONFIG.speed);
    for (const p of particles) {
      expect(p.x).toBeGreaterThanOrEqual(0);
      expect(p.x).toBeLessThanOrEqual(1);
      expect(p.y).toBeGreaterThanOrEqual(0);
      expect(p.y).toBeLessThanOrEqual(1);
    }
  });

  test('particles have small initial velocities bounded by speed param', () => {
    const particles = createParticles(50, DEFAULT_CONFIG.speed);
    for (const p of particles) {
      expect(Math.abs(p.vx)).toBeLessThanOrEqual(DEFAULT_CONFIG.speed);
      expect(Math.abs(p.vy)).toBeLessThanOrEqual(DEFAULT_CONFIG.speed);
    }
  });
});

describe('updatePhysics', () => {
  test('applies velocity to position', () => {
    const particles: Particle[] = [
      { x: 0.5, y: 0.5, vx: 0.01, vy: -0.01 },
    ];
    updatePhysics(particles, 0.5, 0.5, DEFAULT_CONFIG);
    expect(particles[0].x).toBeGreaterThan(0.5);
    expect(particles[0].y).toBeLessThan(0.5);
  });

  test('dampens velocity each frame', () => {
    const particles: Particle[] = [
      { x: 0.5, y: 0.5, vx: 0.005, vy: 0.005 },
    ];
    updatePhysics(particles, 0.5, 0.5, DEFAULT_CONFIG);
    expect(Math.abs(particles[0].vx)).toBeLessThan(0.005);
    expect(Math.abs(particles[0].vy)).toBeLessThan(0.005);
  });

  test('repels nearby particles', () => {
    const particles: Particle[] = [
      { x: 0.50, y: 0.5, vx: 0, vy: 0 },
      { x: 0.51, y: 0.5, vx: 0, vy: 0 },
    ];
    const config = { ...DEFAULT_CONFIG, attraction: 0 };
    updatePhysics(particles, 0.5, 0.5, config);
    expect(particles[0].vx).toBeLessThan(0);
    expect(particles[1].vx).toBeGreaterThan(0);
  });

  test('attracts particles toward mouse', () => {
    const particles: Particle[] = [
      { x: 0.2, y: 0.2, vx: 0, vy: 0 },
    ];
    updatePhysics(particles, 0.8, 0.8, DEFAULT_CONFIG);
    expect(particles[0].vx).toBeGreaterThan(0);
    expect(particles[0].vy).toBeGreaterThan(0);
  });

  test('wraps particles that go out of bounds', () => {
    const particles: Particle[] = [
      { x: 1.05, y: -0.05, vx: 0.001, vy: -0.001 },
    ];
    updatePhysics(particles, 0.5, 0.5, DEFAULT_CONFIG);
    expect(particles[0].x).toBeGreaterThanOrEqual(0);
    expect(particles[0].x).toBeLessThanOrEqual(1);
    expect(particles[0].y).toBeGreaterThanOrEqual(0);
    expect(particles[0].y).toBeLessThanOrEqual(1);
  });

  test('caps velocity to max speed', () => {
    const particles: Particle[] = [
      { x: 0.5, y: 0.5, vx: 0.1, vy: 0.1 },
    ];
    updatePhysics(particles, 0.5, 0.5, DEFAULT_CONFIG);
    const speed = Math.sqrt(particles[0].vx ** 2 + particles[0].vy ** 2);
    expect(speed).toBeLessThanOrEqual(DEFAULT_CONFIG.speed * 1.01);
  });
});

describe('pointerPoint', () => {
  test('reads the first touch of a touch event', () => {
    const e = new Event('touchmove');
    Object.defineProperty(e, 'touches', { value: [{ clientX: 30, clientY: 40 }] });
    expect(pointerPoint(e)).toEqual({ clientX: 30, clientY: 40 });
  });

  test('reads a pointer or mouse event itself', () => {
    const e = new MouseEvent('pointermove', { clientX: 5, clientY: 6 });
    expect(pointerPoint(e)).toMatchObject({ clientX: 5, clientY: 6 });
  });

  test('is null for a touch event with no touches left', () => {
    const e = new Event('touchmove');
    Object.defineProperty(e, 'touches', { value: [] });
    expect(pointerPoint(e)).toBeNull();
  });
});

describe('startBlueprintGrid', () => {
  let fx: ReturnType<typeof stubCanvas>;
  afterEach(() => fx.restore());

  function mount(options: Parameters<typeof startBlueprintGrid>[1] = {}) {
    return startBlueprintGrid(document.createElement('div'), { canvas: document.createElement('canvas'), ...options });
  }

  test('with reduced motion requested, paints one still frame and does not loop', () => {
    fx = stubCanvas({ reduceMotion: true });
    const onFrame = vi.fn();
    const cleanup = mount({ onFrame });
    expect(fx.raf).not.toHaveBeenCalled();
    expect(onFrame).toHaveBeenCalledTimes(1);
    expect(fx.calls).toContainEqual(['clearRect', 0, 0, 400, 300]);
    expect(() => cleanup()).not.toThrow();
  });

  test('without reduced motion, the animation loop runs', () => {
    fx = stubCanvas();
    mount();
    expect(fx.raf).toHaveBeenCalled();
  });

  test('with reduced motion, a theme change repaints the still frame in the new colour', async () => {
    fx = stubCanvas({ reduceMotion: true });
    mount();
    expect(fx.ctx.strokeStyle).toBe('rgb(20,60,130)');
    document.documentElement.setAttribute('data-theme', 'dark');
    await Promise.resolve();
    expect(fx.ctx.strokeStyle).toBe('rgb(90,155,255)');
  });

  test('draws in the colour and peak opacity it is given', () => {
    fx = stubCanvas({ reduceMotion: true });
    // Every particle at the centre, which sits on a vertical line of a 400px-wide grid.
    vi.spyOn(Math, 'random').mockReturnValue(0.5);
    mount({ color: () => 'rgb(1, 2, 3)', maxAlpha: 0.1 });
    expect(fx.ctx.strokeStyle).toBe('rgb(1, 2, 3)');
    expect(fx.alphas.length).toBeGreaterThan(0);
    expect(Math.max(...fx.alphas)).toBeLessThanOrEqual(0.1);
    expect(Math.max(...fx.alphas)).toBeGreaterThan(0.05);
  });

  test('segments are 8px long on both axes, on a tall screen too', () => {
    fx = stubCanvas({ width: 300, height: 600, reduceMotion: true });
    vi.spyOn(Math, 'random').mockReturnValue(0.5);
    mount();
    const lengths = { h: [] as number[], v: [] as number[] };
    for (let i = 0; i < fx.calls.length - 1; i++) {
      const [m, x0, y0] = fx.calls[i], [n, x1, y1] = fx.calls[i + 1];
      if (m !== 'moveTo' || n !== 'lineTo') continue;
      if (y0 === y1) lengths.h.push(x1 - x0); else lengths.v.push(y1 - y0);
    }
    expect(lengths.h.length).toBeGreaterThan(0);
    expect(lengths.v.length).toBeGreaterThan(0);
    expect(Math.max(...lengths.h, ...lengths.v)).toBeLessThanOrEqual(8 + 1e-9);
  });

  test('follows a finger: listens for touches on the pointer target, passively, and stops on cleanup', () => {
    fx = stubCanvas();
    const target = new EventTarget();
    const add = vi.spyOn(target, 'addEventListener');
    const remove = vi.spyOn(target, 'removeEventListener');
    const cleanup = mount({ pointerTarget: target });

    const added = add.mock.calls.map((c) => c[0]);
    expect(added).toEqual(expect.arrayContaining(['pointermove', 'touchstart', 'touchmove']));
    expect(add.mock.calls.every((c) => (c[2] as AddEventListenerOptions)?.passive === true)).toBe(true);
    cleanup();
    expect(remove.mock.calls.map((c) => c[0])).toEqual(expect.arrayContaining(added));
  });
});
