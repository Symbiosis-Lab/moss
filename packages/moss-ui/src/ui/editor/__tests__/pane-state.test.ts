// packages/moss-ui/src/ui/editor/__tests__/pane-state.test.ts
import { describe, it, expect, beforeEach, vi } from 'vitest';

// Mock the blueprint-grid module so the lifecycle (start on mount, stop on
// unmount/re-mount) is observable without a real 2D canvas context (jsdom's
// getContext throws). `startBlueprintGrid` returns a spy stop fn.
const gridStop = vi.fn();
const startSpy = vi.fn((_el: HTMLElement) => gridStop);
vi.mock('@symbiosis-lab/moss-blueprint-grid', () => ({
  startBlueprintGrid: (el: HTMLElement) => startSpy(el),
}));

import { mountPaneState, unmountPaneState } from '../pane-state';

describe('pane-state', () => {
  let host: HTMLElement;
  beforeEach(() => {
    host = document.createElement('div');
    // The module-level grid handle persists across tests in a file; flush any
    // leaked handle from a prior test BEFORE resetting the spies so each test's
    // call counts start clean.
    unmountPaneState(host);
    gridStop.mockClear();
    startSpy.mockClear();
  });

  it('renders title and description', () => {
    mountPaneState(host, { title: 'Generated page', description: 'No source file.' });
    expect(host.querySelector('.moss-pane-state__title')!.textContent).toBe('Generated page');
    expect(host.querySelector('.moss-pane-state__desc')!.textContent).toBe('No source file.');
    expect(host.querySelector('.moss-pane-state__cta')).toBeNull();
  });

  it('renders a CTA and fires onClick', () => {
    let clicked = 0;
    mountPaneState(host, { title: 'T', description: 'D', cta: { label: 'Go', onClick: () => { clicked++; } } });
    const btn = host.querySelector('.moss-pane-state__cta') as HTMLButtonElement;
    expect(btn.textContent!.trim()).toBe('Go');
    btn.click();
    expect(clicked).toBe(1);
  });

  it('escapes HTML in title and description', () => {
    mountPaneState(host, { title: '<img>', description: '<b>x</b>' });
    expect(host.querySelector('.moss-pane-state__title')!.innerHTML).toBe('&lt;img&gt;');
    expect(host.querySelector('.moss-pane-state__desc')!.innerHTML).toBe('&lt;b&gt;x&lt;/b&gt;');
  });

  it('mount is idempotent (replaces a prior state)', () => {
    mountPaneState(host, { title: 'A', description: 'D' });
    mountPaneState(host, { title: 'B', description: 'D' });
    expect(host.querySelectorAll('[data-pane-state]').length).toBe(1);
    expect(host.querySelector('.moss-pane-state__title')!.textContent).toBe('B');
  });

  it('unmount removes the state', () => {
    mountPaneState(host, { title: 'A', description: 'D' });
    unmountPaneState(host);
    expect(host.querySelector('[data-pane-state]')).toBeNull();
  });

  it('mounts a blueprint-grid canvas behind the content and starts the animation', () => {
    mountPaneState(host, { title: 'T', description: 'D' });
    const canvas = host.querySelector('#blueprint-canvas');
    expect(canvas).not.toBeNull();
    expect(canvas!.getAttribute('aria-hidden')).toBe('true');
    // The canvas is the FIRST child so it paints behind the content in source
    // order (and via z-index in CSS); the inner content follows it.
    expect(host.querySelector('[data-pane-state]')!.firstElementChild).toBe(canvas);
    expect(startSpy).toHaveBeenCalledTimes(1);
  });

  it('stops the blueprint grid on unmount (no RAF/canvas leak)', () => {
    mountPaneState(host, { title: 'T', description: 'D' });
    unmountPaneState(host);
    expect(gridStop).toHaveBeenCalledTimes(1);
  });

  it('stops the prior grid when re-mounting (no leak across surfaces)', () => {
    mountPaneState(host, { title: 'A', description: 'D' });
    mountPaneState(host, { title: 'B', description: 'D' });
    expect(gridStop).toHaveBeenCalledTimes(1); // prior grid torn down
    expect(startSpy).toHaveBeenCalledTimes(2); // new grid started
  });
});
