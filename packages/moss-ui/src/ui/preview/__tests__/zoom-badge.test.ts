/**
 * ZoomBadge — the transient "125%" over the preview corner.
 * Shows on every zoom change, auto-hides after a beat, click resets.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { ZoomBadge } from '../preview-zoom';

describe('ZoomBadge', () => {
  let container: HTMLElement;
  let onReset: ReturnType<typeof vi.fn>;
  let badge: ZoomBadge;

  beforeEach(() => {
    vi.useFakeTimers();
    container = document.createElement('div');
    document.body.appendChild(container);
    onReset = vi.fn();
    badge = new ZoomBadge(container, onReset);
  });

  afterEach(() => {
    badge.destroy();
    container.remove();
    vi.useRealTimers();
  });

  const el = () => container.querySelector('.moss-zoom-badge') as HTMLButtonElement;

  it('starts hidden, shows the percentage on show(), and auto-hides', () => {
    expect(el().classList.contains('visible')).toBe(false);

    badge.show(1.25);
    expect(el().textContent).toBe('125%');
    expect(el().classList.contains('visible')).toBe(true);

    vi.advanceTimersByTime(1300);
    expect(el().classList.contains('visible')).toBe(false);
  });

  it('a second show() restarts the hide clock instead of stacking timers', () => {
    badge.show(1.1);
    vi.advanceTimersByTime(800);
    badge.show(1.25);
    vi.advanceTimersByTime(800); // 1600ms after first show — would have hidden
    expect(el().classList.contains('visible')).toBe(true);
    vi.advanceTimersByTime(500);
    expect(el().classList.contains('visible')).toBe(false);
  });

  it('click fires the reset callback', () => {
    badge.show(1.5);
    el().click();
    expect(onReset).toHaveBeenCalledTimes(1);
  });

  it('tooltip teaches the ⌘0 keyboard twin of the click-to-reset', () => {
    expect(el().getAttribute('data-tooltip')).not.toBeNull();
    expect(el().getAttribute('data-tooltip-kbd')).toBe('⌘0');
  });

  it('destroy removes the element and its pending timer', () => {
    badge.show(1.25);
    badge.destroy();
    expect(container.querySelector('.moss-zoom-badge')).toBeNull();
    expect(() => vi.runAllTimers()).not.toThrow();
  });
});
