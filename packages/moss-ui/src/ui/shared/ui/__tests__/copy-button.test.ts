/**
 * Unit tests for the canonical copy button factory (shared/ui/copy-button.ts).
 *
 * Tests behavior, not implementation details:
 * - DOM structure: button element, icons, optional label
 * - Click writes to clipboard
 * - Copied state: class, label text, aria-label
 * - Revert after 2000ms (fake timers)
 * - Rapid double-click re-arms the timer
 * - Icon-only mode (label='')
 * - onCopied callback fires
 * - Clipboard failure stays idle
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { createCopyButton } from '../copy-button';
import { initTooltips, destroyTooltips } from '../../../components/tooltip';

// Mock the i18n module so tests don't depend on locale state
vi.mock('../../../i18n', () => ({
  t: (key: string) => {
    const strings: Record<string, string> = {
      'publish.copy': 'Copy',
      'publish.copied': 'Copied',
    };
    return strings[key] ?? key;
  },
}));

// Clipboard mock — resolved by default; tests that need rejection override per-test
const clipboardWriteText = vi.fn().mockResolvedValue(undefined);

beforeEach(() => {
  vi.useFakeTimers();
  document.body.innerHTML = '';

  // Install a writable clipboard mock on the global navigator
  Object.defineProperty(navigator, 'clipboard', {
    value: { writeText: clipboardWriteText },
    writable: true,
    configurable: true,
  });
  clipboardWriteText.mockResolvedValue(undefined);
});

afterEach(() => {
  vi.runAllTimers();
  vi.useRealTimers();
  clipboardWriteText.mockReset();
});

describe('createCopyButton', () => {
  // 1. renders button
  it('renders a <button type="button" class="moss-copy-btn">', () => {
    const btn = createCopyButton({ value: 'hello' });
    expect(btn.tagName).toBe('BUTTON');
    expect(btn.type).toBe('button');
    expect(btn.classList.contains('moss-copy-btn')).toBe(true);
  });

  // 2. copy icon present
  it('contains .moss-copy-btn__icon--copy in the DOM', () => {
    const btn = createCopyButton({ value: 'hello' });
    expect(btn.querySelector('.moss-copy-btn__icon--copy')).not.toBeNull();
  });

  // 3. check icon present
  it('contains .moss-copy-btn__icon--check in the DOM (hidden via CSS)', () => {
    const btn = createCopyButton({ value: 'hello' });
    expect(btn.querySelector('.moss-copy-btn__icon--check')).not.toBeNull();
  });

  // 4. label rendered with default text
  it('renders .moss-copy-btn__label with default t("publish.copy") when label is omitted', () => {
    const btn = createCopyButton({ value: 'hello' });
    const label = btn.querySelector('.moss-copy-btn__label');
    expect(label).not.toBeNull();
    expect(label?.textContent).toBe('Copy');
  });

  // 5. click calls clipboard.writeText
  it('calls navigator.clipboard.writeText with opts.value on click', async () => {
    const btn = createCopyButton({ value: 'https://example.com' });
    btn.click();
    expect(clipboardWriteText).toHaveBeenCalledWith('https://example.com');
  });

  // 6. copied state: button has moss-copy-btn--copied class after click resolves
  it('adds moss-copy-btn--copied class after clipboard write succeeds', async () => {
    const btn = createCopyButton({ value: 'test' });
    btn.click();
    // Flush the resolved promise microtask
    await Promise.resolve();
    expect(btn.classList.contains('moss-copy-btn--copied')).toBe(true);
  });

  // 7. label changes to copied
  it('changes label text to t("publish.copied") after click', async () => {
    const btn = createCopyButton({ value: 'test' });
    btn.click();
    await Promise.resolve();
    const label = btn.querySelector('.moss-copy-btn__label');
    expect(label?.textContent).toBe('Copied');
  });

  // 8. aria-label changes
  it('sets aria-label to t("publish.copied") after click', async () => {
    const btn = createCopyButton({ value: 'test' });
    btn.click();
    await Promise.resolve();
    expect(btn.getAttribute('aria-label')).toBe('Copied');
  });

  // 9. reverts after 2000ms
  it('reverts to initial state after 2000ms', async () => {
    const btn = createCopyButton({ value: 'test' });
    btn.click();
    await Promise.resolve();
    expect(btn.classList.contains('moss-copy-btn--copied')).toBe(true);

    vi.advanceTimersByTime(2000);
    expect(btn.classList.contains('moss-copy-btn--copied')).toBe(false);
    expect(btn.querySelector('.moss-copy-btn__label')?.textContent).toBe('Copy');
    expect(btn.getAttribute('aria-label')).toBe('Copy');
  });

  // 10. rapid double-click re-arms once
  it('rapid double-click re-arms timer so state persists for 2000ms from last click', async () => {
    const btn = createCopyButton({ value: 'test' });

    // First click
    btn.click();
    await Promise.resolve();

    // 1000ms later: second click (re-arms)
    vi.advanceTimersByTime(1000);
    btn.click();
    await Promise.resolve();

    // 1000ms after second click -- still in copied state (only 1000ms of 2000ms have passed)
    vi.advanceTimersByTime(1000);
    expect(btn.classList.contains('moss-copy-btn--copied')).toBe(true);

    // 1000ms more -- now 2000ms after second click, should have reverted
    vi.advanceTimersByTime(1000);
    expect(btn.classList.contains('moss-copy-btn--copied')).toBe(false);
  });

  // 11. label='' no label span
  it('does not render .moss-copy-btn__label when label is empty string', () => {
    const btn = createCopyButton({ value: 'x', label: '', ariaLabel: 'Copy' });
    expect(btn.querySelector('.moss-copy-btn__label')).toBeNull();
  });

  // 12. onCopied fires
  it('calls opts.onCopied after successful clipboard write', async () => {
    const onCopied = vi.fn();
    const btn = createCopyButton({ value: 'test', onCopied });
    btn.click();
    await Promise.resolve();
    expect(onCopied).toHaveBeenCalledTimes(1);
  });

  // 13. clipboard rejection stays idle
  it('does not enter copied state when clipboard.writeText rejects', async () => {
    clipboardWriteText.mockRejectedValueOnce(new Error('Permission denied'));
    const btn = createCopyButton({ value: 'test' });
    btn.click();
    // Flush both the rejection and any resulting microtasks
    await Promise.resolve();
    await Promise.resolve();
    expect(btn.classList.contains('moss-copy-btn--copied')).toBe(false);
  });
});

/**
 * In icon-only mode the tooltip IS the acknowledgement — there is no label to
 * swap, so `data-tooltip` becomes "Copied" and the portal's live-refresh
 * observer repaints the pill the user is still hovering.
 *
 * This is why the tooltip system has no global "a press hides the tooltip"
 * rule, standard though that is elsewhere: it would delete this ack outright,
 * and until this test existed nothing in the suite would have gone red.
 */
describe('copy button — the icon-only ack is delivered through the tooltip', () => {
  afterEach(() => destroyTooltips());

  it('repaints the hovered tooltip as "Copied" without the pointer moving', async () => {
    initTooltips();
    const btn = createCopyButton({ value: 'https://example.com', label: '' });
    document.body.appendChild(btn);

    btn.dispatchEvent(new MouseEvent('mouseover', { bubbles: true }));
    const pill = document.querySelector('.moss-tooltip')!;
    expect(pill.classList.contains('visible')).toBe(true);
    expect(pill.textContent).toBe('Copy');

    // A real press, not `.click()`: the pointer event has to be in the
    // sequence or this test cannot see a press-dismissal at all — which is the
    // only thing it exists to catch.
    btn.dispatchEvent(new Event('pointerdown', { bubbles: true }));
    btn.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));
    btn.click();
    await vi.advanceTimersByTimeAsync(0);
    // The MutationObserver that repaints in place delivers on a microtask.
    await Promise.resolve();

    expect(btn.getAttribute('data-tooltip')).toBe('Copied');
    expect(pill.classList.contains('visible')).toBe(true);
  });
});
