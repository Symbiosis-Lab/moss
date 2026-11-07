/**
 * A toast action's URL comes from whatever raised the toast, and a plugin can
 * raise one. The click handler calls `preventDefault` and routes through
 * `openUrl`, which refuses a non-http(s) scheme — but the `href` it sets is
 * still a live link for a middle-click or the context menu's "Open link",
 * and that route reaches the OS handler without passing `openUrl` at all.
 */
import { describe, it, expect } from 'vitest';
import { buildActionsElement, reconcileInlineLink } from '../toast-dom';

const hrefOf = (el: HTMLElement) =>
  el.querySelector('a')?.getAttribute('href') ?? '';

describe('a toast link', () => {
  it('keeps an http(s) action URL', () => {
    const el = buildActionsElement([{ label: 'Open', url: 'https://example.com/x' }]);
    expect(hrefOf(el)).toBe('https://example.com/x');
  });

  it('refuses to make a non-http scheme clickable', () => {
    for (const url of ['javascript:alert(1)', 'file:///etc/passwd', 'app-of-their-choosing://go']) {
      expect(hrefOf(buildActionsElement([{ label: 'Open', url }]))).toBe('#');
    }
  });

  it('applies the same rule to an inline link', () => {
    const toast = document.createElement('div');
    const message = document.createElement('div');
    message.className = 'moss-toast-message';
    toast.appendChild(message);
    reconcileInlineLink(toast, { label: 'Go', url: 'javascript:alert(1)' });
    expect(hrefOf(message)).toBe('#');
  });
});
