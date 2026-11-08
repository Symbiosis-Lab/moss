/**
 * SpecialFileModal — says what a special file is before the editor opens it.
 *
 * Cancel (or Escape, or the backdrop) answers no; Open answers yes, carrying
 * the "don't show this again for this kind" checkbox.
 */

import { OutcomeModal } from "../../shared/ui/outcome-modal";
import { checkboxRow } from "../../shared/ui/toggle";
import { t } from "../../i18n/index";
import type { SpecialFileAnswer, SpecialFileKind } from './special-file';

let titleIdCounter = 0;

export class SpecialFileModal extends OutcomeModal {
  private checkbox!: HTMLInputElement;
  private openButton!: HTMLButtonElement;
  private readonly titleId = `moss-special-file-title-${titleIdCounter++}`;

  /** Open answers yes with the checkbox; Cancel, Escape and the backdrop answer no. */
  constructor(private readonly kind: SpecialFileKind, answer: (a: SpecialFileAnswer) => void) {
    super((open) => answer({ open, dontAskAgain: open && this.checkbox.checked }));
    this.container.classList.add('moss-modal--confirm', 'moss-modal--special-file');
    this.container.setAttribute('aria-labelledby', this.titleId);
  }

  renderContent(): HTMLElement {
    const root = document.createElement('div');
    root.className = 'moss-special-file-modal moss-stack';

    const title = document.createElement('h2');
    title.className = 'moss-modal-title';
    title.id = this.titleId;
    title.textContent = t(`special_file.${this.kind}.title`);

    const body = document.createElement('p');
    body.className = 'moss-modal-body';
    // No params: the welcome text carries literal {{site_name}} placeholders.
    body.textContent = t(`special_file.${this.kind}.body`);

    const { row: check, input } = checkboxRow(t(`special_file.${this.kind}.skip`));
    this.checkbox = input;

    const cancel = this.button('cancel', 'moss-btn-secondary', t('dialog.cancel'));
    cancel.addEventListener('click', () => this.close());
    const open = this.button('open', 'moss-btn-primary', t('reference.open'));
    open.addEventListener('click', () => this.finish(true));
    this.openButton = open;
    const actions = document.createElement('div');
    actions.className = 'moss-modal-actions';
    actions.append(cancel, open);

    root.append(title, body, check, actions);
    return root;
  }

  /** Open, not the checkbox that renders first: Enter opens the file, and muting takes a deliberate click. */
  protected initialFocus(): HTMLElement | null {
    return this.openButton;
  }

  private button(action: string, tone: string, label: string): HTMLButtonElement {
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.className = `moss-btn ${tone}`;
    btn.dataset.action = action;
    btn.textContent = label;
    return btn;
  }
}

/** Show the dialog and resolve with the answer. */
export function askAboutSpecialFile(kind: SpecialFileKind): Promise<SpecialFileAnswer> {
  return new Promise((resolve) => {
    const modal = new SpecialFileModal(kind, resolve);
    modal.open();
    // BaseModal refuses a second instance of the class: resolve as a no.
    if (!modal.isOpen()) resolve({ open: false, dontAskAgain: false });
  });
}
