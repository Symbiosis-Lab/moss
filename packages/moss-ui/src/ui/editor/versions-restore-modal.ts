/**
 * VersionsRestoreModal — the confirm surface `versions-surface.ts` opens
 * before a whole-site OR whole-folder restore (design, "Restore semantics":
 * "A site restore gets the delete-confirm modal shape with disclosed
 * consequences" — folder scope reuses the same shape. A page
 * restore needs no confirmation at all (same section) and never opens this.
 *
 * A `BaseModal` subclass; the
 * constructor-plus-`renderContent()` shape mirrors every other subclass —
 * `first-publish-modal.ts` and `editor/delete-confirm-modal.ts` are the two
 * this one's title/body/actions layout was copied from most directly.
 */

import { BaseModal } from "../shared/ui/base-modal";
import { t, formatNumber } from "../i18n/index";

export interface VersionsRestoreModalOptions {
  /** The version's own moment, already formatted ("Sep 8, 11:20") — the
   *  same string the header above this button shows. */
  when: string;
  /** `changed_since.length` for the version being restored to. */
  pages: number;
  /** Of those, the ones with verb `"added"` — created after this moment,
   *  so a restore sends them to the Trash rather than reverting them. */
  added: number;
  /** The folder's display name for a folder-scope restore, or `null` for the
   *  whole site — picks which title/confirm-label pair this renders. */
  folderName: string | null;
  /** Runs after the modal has closed. */
  onConfirm: () => void;
}

export class VersionsRestoreModal extends BaseModal {
  constructor(private readonly opts: VersionsRestoreModalOptions) {
    super();
    this.container.classList.add('moss-modal--confirm');
  }

  renderContent(): HTMLElement {
    const root = document.createElement('div');
    root.className = 'moss-versions-restore-modal moss-stack';

    const title = document.createElement('h2');
    title.className = 'moss-modal-title';
    title.textContent = this.opts.folderName === null
      ? t('versions.modal_title', { when: this.opts.when })
      : t('versions.modal_title_scope', { name: this.opts.folderName, when: this.opts.when });
    root.appendChild(title);

    const body = document.createElement('p');
    body.className = 'moss-modal-body';
    body.textContent = t('versions.modal_body', {
      pages: formatNumber(this.opts.pages),
      added: formatNumber(this.opts.added),
    });
    root.appendChild(body);

    const actions = document.createElement('div');
    actions.className = 'moss-modal-actions';

    const cancel = document.createElement('button');
    cancel.type = 'button';
    cancel.className = 'moss-btn moss-btn-secondary';
    cancel.dataset.action = 'cancel';
    // Reused across modules (delete-confirm-modal.ts's own Cancel), per C1.
    cancel.textContent = t('tree.delete_cancel');
    cancel.addEventListener('click', () => this.close());
    actions.appendChild(cancel);

    const confirm = document.createElement('button');
    confirm.type = 'button';
    confirm.className = 'moss-btn moss-btn-primary';
    confirm.dataset.action = 'confirm';
    confirm.textContent = this.opts.folderName === null
      ? t('versions.restore_site')
      : t('versions.restore_scope', { name: this.opts.folderName });
    confirm.addEventListener('click', () => {
      this.close();
      this.opts.onConfirm();
    });
    actions.appendChild(confirm);

    root.appendChild(actions);
    return root;
  }
}
