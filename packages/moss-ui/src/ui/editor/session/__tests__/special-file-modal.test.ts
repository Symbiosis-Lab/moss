/**
 * SpecialFileModal — one dialog, five wordings, Cancel and Open, one checkbox.
 * DOM structure and answers only; nothing here claims a cascade or a paint.
 */

import { describe, it, expect, afterEach } from 'vitest';
import { askAboutSpecialFile } from '../special-file-modal';
import type { SpecialFileKind } from '../special-file';
import { setLocale, t } from "../../../i18n/index";

const KINDS: SpecialFileKind[] = ['agent-instructions', 'welcome-email', 'footer', 'not-found', 'page-settings'];

function modal(): HTMLElement {
  const el = document.querySelector<HTMLElement>('.moss-modal--special-file.visible');
  if (!el) throw new Error('special-file modal is not open');
  return el;
}
const button = (action: string) => modal().querySelector<HTMLButtonElement>(`button[data-action="${action}"]`)!;
const checkbox = () => modal().querySelector<HTMLInputElement>('input[type="checkbox"]')!;

afterEach(() => {
  document.body.innerHTML = '';
  setLocale('en');
});

describe('SpecialFileModal', () => {
  it.each(KINDS)('shows the title, body and checkbox wording for %s', (kind) => {
    void askAboutSpecialFile(kind);
    expect(modal().querySelector('.moss-modal-title')!.textContent).toBe(t(`special_file.${kind}.title`));
    expect(modal().querySelector('.moss-modal-body')!.textContent).toBe(t(`special_file.${kind}.body`));
    expect(modal().querySelector('.moss-modal-check')!.textContent).toBe(t(`special_file.${kind}.skip`));
    expect(modal().getAttribute('aria-labelledby')).toBe(modal().querySelector('.moss-modal-title')!.id);
    expect(checkbox().checked).toBe(false);
  });

  it('focuses Open, not the checkbox, so Enter opens and Space cannot mute by accident', async () => {
    void askAboutSpecialFile('footer');
    await new Promise((r) => requestAnimationFrame(() => r(null)));
    expect(document.activeElement).toBe(button('open'));
  });

  it('shows the welcome email placeholders as written, in every language', () => {
    for (const locale of ['en', 'de', 'zh-hans', 'zh-hant'] as const) {
      setLocale(locale);
      void askAboutSpecialFile('welcome-email');
      const text = modal().querySelector('.moss-modal-body')!.textContent!;
      expect(text).toContain('{{site_name}}');
      expect(text).toContain('{{site_url}}');
      expect(text).toContain('{{email}}');
      document.body.innerHTML = '';
    }
  });

  it('Open answers yes and carries the checkbox', async () => {
    const answer = askAboutSpecialFile('footer');
    checkbox().click();
    button('open').click();
    expect(await answer).toEqual({ open: true, dontAskAgain: true });
    expect(document.querySelector('.moss-modal--special-file')).toBeNull();
  });

  it('Open without the box answers yes, not muted', async () => {
    const answer = askAboutSpecialFile('footer');
    button('open').click();
    expect(await answer).toEqual({ open: true, dontAskAgain: false });
  });

  it('Cancel, Escape and the backdrop answer no, even with the box ticked', async () => {
    for (const dismiss of [
      () => button('cancel').click(),
      () => document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' })),
      () => document.querySelector<HTMLElement>('.moss-modal-backdrop')!.click(),
    ]) {
      const answer = askAboutSpecialFile('not-found');
      checkbox().click();
      dismiss();
      expect(await answer).toEqual({ open: false, dontAskAgain: false });
    }
  });

  it('a second dialog while one is up answers no instead of stacking', async () => {
    void askAboutSpecialFile('footer');
    expect(await askAboutSpecialFile('footer')).toEqual({ open: false, dontAskAgain: false });
    expect(document.querySelectorAll('.moss-modal--special-file').length).toBe(1);
  });
});
