/**
 * DeleteConfirmModal — the one surface a file-tree delete goes through.
 *
 * It opens in the click's frame with the question ("Move X to Trash?") and
 * runs the project-wide reference scan BEHIND it; the scan resolves into the
 * modal rather than in front of it.
 *
 * Its state is the scan. `hits` is null while the scan runs and the resolved
 * list afterwards, and everything on screen derives from it:
 *
 *   null  → the question, a "checking" status line, Cancel / Move to Trash
 *   []    → the question, status "nothing else links here", same buttons
 *   [...] → the disclosure: "Deleting this breaks N links", the capped list,
 *           Cancel / Trash anyway / Remove links and trash
 *
 * Pressing Move to Trash while the scan is still running is `runAction` over
 * the scan promise: the button paints busy in the click's frame, a second
 * press is refused by runAction's guard, and the success handler trashes only
 * when the list it is handed is empty — a non-empty list has become the
 * disclosure, and the handler does nothing.
 *
 * Renders with the shared `.moss-modal-title` / `.moss-modal-body` /
 * `.moss-modal-actions` classes inside a `.moss-stack`, styled by
 * `styles/shared/modal.css`, which both webviews import.
 */

import { OutcomeModal } from "../shared/ui/outcome-modal";
import type { FileReferenceHit } from '@symbiosis-lab/moss-ui';
import { runAction } from "../shared/ui/run-action";
import { t } from "../i18n/index";

const MAX_LIST_ITEMS = 5;

let titleIdCounter = 0;

/** The question the modal asks while the scan runs. */
export interface DeleteQuestion {
  /** "Move "X" to Trash?" / "Move N items to Trash?" */
  title: string;
  /** "It contains N items." — folders only. */
  detail?: string;
}

export interface DeleteConfirmOptions {
  question: DeleteQuestion;
  /**
   * The reference scan. MUST NOT reject: the caller owns the failure policy
   * (TreeOps.scanReferences resolves a failed scan to "no hits" with a
   * warning), and a second catch here would duplicate it. A caller that
   * already holds the list passes `Promise.resolve(hits)`; the continuation
   * then runs before the task ends, so the question is never painted.
   */
  hits: Promise<FileReferenceHit[]>;
  /**
   * Trash without touching other files. May reject or throw: `run()` then
   * rejects with it, so the caller's `runAction` reports the failure — the
   * modal has no reporting surface of its own and adds none.
   */
  onTrash: () => Promise<void>;
  /** Remove every reference, then trash. Same failure contract. */
  onCleanAndTrash: () => Promise<void>;
}

export class DeleteConfirmModal extends OutcomeModal {
  private hits: FileReferenceHit[] | null = null;
  private root!: HTMLElement;
  private statusText!: HTMLElement;
  /** The trash callback's promise once a trash button has been pressed. */
  private outcome: Promise<void> = Promise.resolve();
  private readonly decided: Promise<void>;
  private readonly titleId: string;

  constructor(private readonly opts: DeleteConfirmOptions) {
    let decide!: () => void;
    const decided = new Promise<void>(resolve => { decide = resolve; });
    // OutcomeModal's boolean is a loose fit for three exits, but the modal
    // carries the exits out itself, so `onDone` only has to mean "decided".
    super(() => decide());
    this.decided = decided;
    this.titleId = `moss-delete-confirm-title-${titleIdCounter++}`;
    this.container.classList.add('moss-modal--confirm', 'moss-modal--delete-confirm');
    this.container.setAttribute('aria-labelledby', this.titleId);
  }

  /**
   * Open, and resolve once the decision has been carried out or abandoned.
   * BaseModal refuses a second instance of the class silently (it focuses the
   * live one), so a still-closed modal after `open()` resolves at once.
   */
  run(): Promise<void> {
    this.open();
    if (!this.isOpen()) return Promise.resolve();
    void this.opts.hits.then(hits => this.settleScan(hits));
    return this.decided.then(() => this.outcome);
  }

  renderContent(): HTMLElement {
    const root = document.createElement('div');
    root.className = 'moss-delete-confirm-modal moss-stack';
    this.renderQuestion(root);
    this.root = root;
    return root;
  }

  private settleScan(hits: FileReferenceHit[]): void {
    if (!this.isOpen()) return;
    this.hits = hits;
    if (hits.length > 0) {
      this.renderDisclosure(this.root, hits);
      // The morph replaced the focused button; keep focus inside the trap on
      // the safe default (Cancel is first).
      this.focusFirstElement();
      return;
    }
    this.statusText.parentElement!.classList.replace('is-running', 'is-succeeded');
    this.statusText.textContent = t('tree.delete_no_links');
  }

  /** Close, then carry the decision out; `run()` settles after the callback. */
  private act(cb: () => Promise<void>): void {
    this.outcome = cb();
    this.finish(true);
  }

  private renderQuestion(root: HTMLElement): void {
    root.appendChild(this.title(this.opts.question.title));

    if (this.opts.question.detail) {
      const p = document.createElement('p');
      p.className = 'moss-modal-body';
      p.textContent = this.opts.question.detail;
      root.appendChild(p);
    }

    // A fixed slot: the text changes when the scan lands, the geometry does not.
    const status = document.createElement('p');
    status.className = 'moss-inline-status is-running';
    status.setAttribute('aria-live', 'polite');
    const glyph = document.createElement('span');
    glyph.className = 'moss-inline-status__glyph';
    this.statusText = document.createElement('span');
    this.statusText.textContent = t('tree.delete_checking_links');
    status.append(glyph, this.statusText);
    root.appendChild(status);

    // Recoverable (OS Trash) and deliberately chosen — the destructive-red
    // rule (design-vocabulary.md "Modals", quoting HIG Alerts' Empty Trash
    // example) reserves red for a consequence the user did NOT choose; the
    // primary/default treatment is correct here.
    const trashBtn = this.button('trash', 'moss-btn-primary', t('tree.delete_trash'));
    trashBtn.addEventListener('click', () => {
      if (this.hits !== null) {
        this.act(this.opts.onTrash);
        return;
      }
      void runAction({
        trigger: trashBtn,
        busyLabel: t('tree.delete_checking_links'),
        run: () => this.opts.hits,
        // Closed (the container left the document) or morphed into the
        // disclosure (the question's DOM was replaced): either way the
        // button is gone, and so is anything to restore.
        stale: () => !trashBtn.isConnected,
        onOk: hits => {
          if (hits.length === 0) this.act(this.opts.onTrash);
        },
      });
    });

    root.appendChild(this.actions(this.cancelButton(), trashBtn));
  }

  private renderDisclosure(root: HTMLElement, hits: FileReferenceHit[]): void {
    root.innerHTML = '';
    const n = hits.length;
    root.appendChild(this.title(
      n === 1 ? t('tree.delete_breaks_one') : t('tree.delete_breaks_n', { n }),
    ));

    const p = document.createElement('p');
    p.className = 'moss-modal-body moss-text--muted';
    p.textContent = t('tree.delete_ref_note');
    root.appendChild(p);

    // Capped index of the inbound links about to break. Each row is two
    // lines: the referencing FILE (full path on the portal tooltip, per the
    // data-tooltip convention) over the link TEXT.
    const ul = document.createElement('ul');
    ul.className = 'moss-modal-ref-list';
    for (const hit of hits.slice(0, MAX_LIST_ITEMS)) {
      const li = document.createElement('li');
      li.className = 'moss-modal-ref-item';
      const file = document.createElement('span');
      file.className = 'moss-modal-ref-file';
      file.setAttribute('data-tooltip', hit.referencing_file);
      file.textContent = hit.referencing_file.split('/').pop() || hit.referencing_file;
      const text = document.createElement('code');
      text.className = 'moss-modal-ref-text';
      text.textContent = hit.ref_text;
      li.append(file, text);
      ul.appendChild(li);
    }
    if (n > MAX_LIST_ITEMS) {
      const li = document.createElement('li');
      li.className = 'moss-modal-ref-item moss-modal-ref-more';
      li.textContent = t('tree.delete_ref_more', { n: n - MAX_LIST_ITEMS });
      ul.appendChild(li);
    }
    root.appendChild(ul);

    // Exactly ONE emphasized (danger) action, the recommended one, last; the
    // quieter escape hatches are secondary. "Trash anyway" sits in the slot
    // "Move to Trash" occupied a moment ago, so a press that lands during the
    // morph does what the user meant.
    const trashAnyway = this.button('trash-anyway', 'moss-btn-secondary', t('tree.delete_trash_anyway'));
    trashAnyway.addEventListener('click', () => this.act(this.opts.onTrash));
    // Stays red: `clean_references_and_delete` (moss-build's command table) rewrites every
    // REFERENCING file's content in place before trashing the target, with
    // no OS Trash and no undo_id for that rewrite — unlike the trash-only
    // buttons above, this half of the action has no recovery path at all.
    const clean = this.button('clean-and-trash', 'moss-btn-danger', t('tree.delete_clean_and_trash'));
    clean.addEventListener('click', () => this.act(this.opts.onCleanAndTrash));
    root.appendChild(this.actions(this.cancelButton(), trashAnyway, clean));
  }

  private title(text: string): HTMLElement {
    const h = document.createElement('h2');
    h.className = 'moss-modal-title';
    h.id = this.titleId;
    h.textContent = text;
    return h;
  }

  /** `data-action` is the stable hook for tests, decoupled from the colour class. */
  private button(action: string, tone: string, label: string): HTMLButtonElement {
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.className = `moss-btn ${tone}`;
    btn.dataset.action = action;
    btn.textContent = label;
    return btn;
  }

  private cancelButton(): HTMLButtonElement {
    const btn = this.button('cancel', 'moss-btn-secondary', t('tree.delete_cancel'));
    btn.addEventListener('click', () => this.close());
    return btn;
  }

  private actions(...buttons: HTMLElement[]): HTMLElement {
    const row = document.createElement('div');
    row.className = 'moss-modal-actions';
    row.append(...buttons);
    return row;
  }
}
