import { describe, it, expect, vi, afterEach } from 'vitest';
import { openInputPopover, closeActiveInputPopover } from '../input-popover';

const flush = () => new Promise((r) => setTimeout(r, 0));

function input(): HTMLInputElement {
  const el = document.querySelector<HTMLInputElement>('.input-popover input.pop-input');
  if (!el) throw new Error('popover input not mounted');
  return el;
}

function press(el: HTMLElement, key: string): void {
  el.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }));
}

afterEach(() => {
  closeActiveInputPopover();
  document.body.innerHTML = '';
});

describe('openInputPopover', () => {
  it('Enter submits the trimmed value after the popover has closed', async () => {
    const onSubmit = vi.fn(() => {
      expect(document.querySelector('.input-popover')).toBeNull();
    });
    openInputPopover({ label: 'Name', at: { x: 10, y: 10 }, onSubmit });
    input().value = '  Blog post  ';
    press(input(), 'Enter');
    expect(onSubmit).toHaveBeenCalledWith('Blog post');
  });

  it('an empty value never submits', () => {
    const onSubmit = vi.fn();
    openInputPopover({ label: 'Name', at: { x: 10, y: 10 }, onSubmit });
    press(input(), 'Enter');
    expect(onSubmit).not.toHaveBeenCalled();
    expect(document.querySelector('.input-popover')).not.toBeNull();
  });

  it('a validate() message blocks the submit and shows under the field', () => {
    const onSubmit = vi.fn();
    openInputPopover({
      label: 'URL',
      at: { x: 10, y: 10 },
      validate: (v) => (v.startsWith('http') ? null : 'not a url'),
      onSubmit,
    });
    input().value = 'nope';
    press(input(), 'Enter');
    expect(onSubmit).not.toHaveBeenCalled();
    expect(document.querySelector('.input-popover')?.textContent).toContain('not a url');
    input().value = 'http://x';
    press(input(), 'Enter');
    expect(onSubmit).toHaveBeenCalledWith('http://x');
  });

  it('Escape closes without submitting; opening another closes the first', async () => {
    const onSubmit = vi.fn();
    openInputPopover({ label: 'A', at: { x: 0, y: 0 }, onSubmit });
    press(input(), 'Escape');
    expect(document.querySelector('.input-popover')).toBeNull();
    expect(onSubmit).not.toHaveBeenCalled();

    openInputPopover({ label: 'B', at: { x: 0, y: 0 }, onSubmit });
    openInputPopover({ label: 'C', at: { x: 0, y: 0 }, onSubmit });
    expect(document.querySelectorAll('.input-popover')).toHaveLength(1);
    expect(document.querySelector('.input-popover')?.textContent).toContain('C');
  });

  it('an initial value is prefilled and selected once focus lands', async () => {
    openInputPopover({ label: 'Rename', at: { x: 0, y: 0 }, initial: 'old', onSubmit: vi.fn() });
    expect(input().value).toBe('old');
    await flush();
    expect(document.activeElement).toBe(input());
    expect(input().selectionStart).toBe(0);
    expect(input().selectionEnd).toBe(3);
  });
});
