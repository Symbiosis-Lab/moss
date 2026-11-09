/**
 * DeleteConfirmModal — the state table
 * Each state is driven with the scan held open and released by the test. The clock is the
 * scan promise, so this is a DOM-structure question and vitest can see all of
 * it; nothing here claims a cascade or a paint.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { DeleteConfirmModal } from '../delete-confirm-modal';
import type { FileReferenceHit } from '@symbiosis-lab/moss-ui';
import { setLocale } from "../../i18n/index";

function hit(file: string, text: string, line = 1): FileReferenceHit {
  return { referencing_file: file, ref_text: text, line };
}

/** A scan the test settles by hand. */
function heldScan() {
  let resolve!: (hits: FileReferenceHit[]) => void;
  const promise = new Promise<FileReferenceHit[]>(r => { resolve = r; });
  return { promise, resolve };
}

function modal(): HTMLElement {
  const el = document.querySelector<HTMLElement>('.moss-modal--delete-confirm.visible');
  if (!el) throw new Error('delete modal is not open');
  return el;
}

/** Buttons are found by their stable `data-action`, not their colour class. */
function button(action: string): HTMLButtonElement {
  const btn = modal().querySelector<HTMLButtonElement>(`button[data-action="${action}"]`);
  if (!btn) throw new Error(`no button[data-action="${action}"]`);
  return btn;
}

function status(): HTMLElement {
  return modal().querySelector<HTMLElement>('.moss-inline-status')!;
}

async function drain(n = 6): Promise<void> {
  while (n--) await Promise.resolve();
}

function open(overrides: Partial<ConstructorParameters<typeof DeleteConfirmModal>[0]> = {}) {
  const scan = heldScan();
  const onTrash = vi.fn().mockResolvedValue(undefined);
  const onCleanAndTrash = vi.fn().mockResolvedValue(undefined);
  const m = new DeleteConfirmModal({
    question: { title: 'Move "a.md" to Trash?', detail: 'It contains 2 items.' },
    hits: scan.promise,
    onTrash,
    onCleanAndTrash,
    ...overrides,
  });
  const done = m.run();
  return { m, scan, onTrash, onCleanAndTrash, done };
}

describe('DeleteConfirmModal', () => {
  beforeEach(() => {
    setLocale('en');
  });

  afterEach(() => {
    document.querySelectorAll('.moss-modal, .moss-modal-backdrop').forEach(n => n.remove());
    vi.restoreAllMocks();
  });

  it('opens with the question and a running status while the scan is held', () => {
    const { onTrash, onCleanAndTrash } = open();

    expect(modal().querySelector('.moss-modal-title')?.textContent).toBe('Move "a.md" to Trash?');
    expect(modal().querySelector('.moss-modal-body')?.textContent).toBe('It contains 2 items.');
    expect(status().classList).toContain('is-running');
    expect(status().textContent).toContain('Checking');
    expect(button('cancel')).toBeTruthy();
    // Recoverable (OS Trash) and deliberately chosen — HIG Alerts' Empty
    // Trash case, so no destructive-red styling.
    expect(button('trash').classList).toContain('moss-btn-primary');
    expect(modal().getAttribute('aria-labelledby')).toBe(modal().querySelector('.moss-modal-title')!.id);
    expect(onTrash).not.toHaveBeenCalled();
    expect(onCleanAndTrash).not.toHaveBeenCalled();
  });

  it('a press during the scan paints the button busy and trashes once the scan is clean', async () => {
    const { scan, onTrash, done } = open();
    const trash = button('trash');
    trash.click();

    // Painted in the click's frame; nothing issued yet.
    expect(trash.getAttribute('aria-busy')).toBe('true');
    expect(trash.classList).toContain('moss-is-busy');
    expect(trash.textContent).toContain('Checking');
    expect(onTrash).not.toHaveBeenCalled();

    // A second press while busy is refused.
    trash.click();

    scan.resolve([]);
    await drain();
    expect(onTrash).toHaveBeenCalledTimes(1);
    expect(document.querySelector('.moss-modal--delete-confirm.visible')).toBeNull();
    await expect(done).resolves.toBeUndefined();
  });

  it('a press during the scan becomes the disclosure when the scan finds links', async () => {
    const { scan, onTrash, onCleanAndTrash } = open();
    button('trash').click();

    scan.resolve([hit('/proj/b.md', '[[a]]'), hit('/proj/c.md', '[a](a.md)')]);
    await drain();

    const title = modal().querySelector('.moss-modal-title')!;
    expect(title.textContent).toBe('Deleting this breaks 2 links');
    expect(modal().getAttribute('aria-labelledby')).toBe(title.id);
    expect(modal().querySelectorAll('.moss-modal-ref-item').length).toBe(2);
    expect(document.activeElement).toBe(button('cancel'));
    expect(button('trash-anyway').classList).toContain('moss-btn-secondary');
    expect(button('clean-and-trash').classList).toContain('moss-btn-danger');
    expect(onTrash).not.toHaveBeenCalled();
    expect(onCleanAndTrash).not.toHaveBeenCalled();
  });

  it('a clean scan before any press updates the status and leaves the primary live', async () => {
    const { scan, onTrash } = open();
    scan.resolve([]);
    await drain();

    expect(status().classList).toContain('is-succeeded');
    expect(status().textContent).toContain('No other pages link here');
    expect(modal().querySelector('.moss-modal-title')?.textContent).toBe('Move "a.md" to Trash?');

    button('trash').click();
    await drain();
    expect(onTrash).toHaveBeenCalledTimes(1);
    expect(document.querySelector('.moss-modal--delete-confirm.visible')).toBeNull();
  });

  it('Cancel during the scan settles run(), and the scan resolving later fires nothing — even after a press', async () => {
    const { scan, onTrash, onCleanAndTrash, done } = open();
    // Press first, then change your mind: the pending press must not trash
    // once the scan comes back clean. This is what `stale()` guards.
    button('trash').click();
    button('cancel').click();
    await expect(done).resolves.toBeUndefined();

    scan.resolve([]);
    await drain();
    expect(onTrash).not.toHaveBeenCalled();
    expect(onCleanAndTrash).not.toHaveBeenCalled();
    expect(document.querySelector('.moss-modal--delete-confirm')).toBeNull();
  });

  it('the disclosure buttons carry out their exits and run() waits for the callback', async () => {
    const first = open();
    first.scan.resolve([hit('/proj/b.md', '[[a]]')]);
    await drain();
    expect(modal().querySelector('.moss-modal-title')?.textContent).toBe('Deleting this breaks 1 link');
    button('trash-anyway').click();
    await drain();
    expect(first.onTrash).toHaveBeenCalledTimes(1);
    expect(first.onCleanAndTrash).not.toHaveBeenCalled();
    await expect(first.done).resolves.toBeUndefined();

    let release!: () => void;
    const second = open({
      onCleanAndTrash: vi.fn(() => new Promise<void>(r => { release = r; })),
    });
    second.scan.resolve([hit('/proj/b.md', '[[a]]')]);
    await drain();
    button('clean-and-trash').click();
    let settled = false;
    void second.done.then(() => { settled = true; });
    await drain();
    // Closed at once, but run() holds until the command has returned.
    expect(document.querySelector('.moss-modal--delete-confirm.visible')).toBeNull();
    expect(settled).toBe(false);
    release();
    await drain();
    expect(settled).toBe(true);
  });

  it('a failing trash rejects run() so the caller reports it; the modal adds no surface', async () => {
    const { scan, done } = open({ onTrash: vi.fn().mockRejectedValue(new Error('EACCES')) });
    scan.resolve([]);
    await drain();
    button('trash').click();
    await expect(done).rejects.toThrow('EACCES');
    expect(document.querySelector('.moss-modal--delete-confirm.visible')).toBeNull();
  });

  it('a second instance while one is open resolves at once and fires nothing', async () => {
    const first = open();
    const second = open();
    await expect(second.done).resolves.toBeUndefined();

    second.scan.resolve([hit('/proj/b.md', '[[a]]')]);
    await drain();
    // The live modal is still the first one, still asking its question.
    expect(modal().querySelector('.moss-modal-title')?.textContent).toBe('Move "a.md" to Trash?');
    expect(document.querySelectorAll('.moss-modal--delete-confirm').length).toBe(1);
    first.m.close();
  });

  it('renders file names and link text as text, never as markup', async () => {
    const evilPath = '/proj/"onmouseover="alert(1)<script>evil.md';
    const { scan } = open();
    scan.resolve([hit(evilPath, '<img src=x onerror=alert(1)>')]);
    await drain();

    const item = modal().querySelector('.moss-modal-ref-item')!;
    expect(item.querySelector('script')).toBeNull();
    expect(item.querySelector('img')).toBeNull();
    const file = item.querySelector('.moss-modal-ref-file')!;
    expect(file.textContent).toContain('<script>');
    expect(file.getAttribute('data-tooltip')).toBe(evilPath);
    expect(file.hasAttribute('onmouseover')).toBe(false);
    expect(item.querySelector('.moss-modal-ref-text')!.textContent).toContain('<img');
  });

  it('caps the list at 5 and shows a "+N more" row', async () => {
    const { scan } = open();
    scan.resolve(Array.from({ length: 8 }, (_, i) => hit(`/proj/f${i}.md`, 'note')));
    await drain();

    expect(modal().querySelectorAll('.moss-modal-ref-item').length).toBe(6);
    expect(modal().querySelector('.moss-modal-ref-more')?.textContent).toContain('3');
  });
});
