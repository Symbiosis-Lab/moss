// The notices about another writer's text the person's text won over: kept in
// Versions, with Undo, or not saved when it could not be kept.

import { t } from "../../i18n/index";
import { baseName } from '../path-domain';
import { removeToastById, showToast } from "../../shared/ui/toast/toast-manager";
import { deepEqual } from '../document-history';
import { changesBetween, outsideWins, type DocumentText } from '../external-merge';
import type { ChangeSet } from '@codemirror/state';

/** How long the notice stays. */
export const KEPT_NOTICE_MS = 6000;
const TOAST_ID = 'editor-outside-edit-kept';

export interface KeptOutsideEditHost {
  currentEntryId(): number | null;
  editor(): { getContent(): string; applyOwnEdit(changes: ChangeSet): void } | null;
  store(): { getValues(): Record<string, unknown>; setField(key: string, value: unknown): void; removeField(key: string): void } | null;
}

/**
 * `saved`: the document as the overlapping merge left it; `theirs`: the same
 * with the other writer winning every overlap.
 */
interface Kept { entry: number; saved: DocumentText; theirs: DocumentText }

const notKeptId = (id: number | null) => `editor-not-kept:${id}`;

export function createKeptOutsideEdit(host: KeptOutsideEditHost) {
  let last: Kept | null = null;
  /** The open document's not-kept notice, so a delete can withdraw it after the registry forgot the id. */
  let notKept: string | null = null;

  function undo(kept: Kept): void {
    removeToastById(TOAST_ID);
    const editor = host.editor();
    const store = host.store();
    if (!editor || host.currentEntryId() !== kept.entry) return;
    const now = { body: editor.getContent(), frontmatter: store?.getValues() ?? {} };
    const target = outsideWins(kept.saved, kept.theirs, now);
    editor.applyOwnEdit(changesBetween(now.body, target.body));
    for (const key of new Set([...Object.keys(now.frontmatter), ...Object.keys(target.frontmatter)])) {
      if (deepEqual(now.frontmatter[key], target.frontmatter[key])) continue;
      if (key in target.frontmatter) store?.setField(key, target.frontmatter[key]);
      else store?.removeField(key);
    }
    if (last === kept) last = null;
  }

  return {
    /** An overlapping merge set the other writer's lines aside in the open document. */
    remember(saved: DocumentText, theirs: DocumentText): void {
      const entry = host.currentEntryId();
      last = entry === null ? null : { entry, saved, theirs };
    },
    /** A save of `path` could not keep the other writer's text, so wrote nothing. */
    noteNotKept(path: string): void {
      notKept = notKeptId(host.currentEntryId());
      showToast({ what: t('editor.changed_outside_unsaved', { name: baseName(path) }), severity: 'NeedsAction' }, {
        id: notKept, persistent: true, dismissible: false,
      });
    },
    withdrawNotKept(): void {
      if (notKept) removeToastById(notKept);
      const id = host.currentEntryId();
      if (id !== null) removeToastById(notKeptId(id));
      notKept = null;
    },
    /** The open document is being left; its notice stays with its entry id. */
    leaving(): void {
      notKept = null;
    },
    /** A save of `path` kept the other writer's text in Versions. */
    announceKept(path: string): void {
      const kept = last !== null && last.entry === host.currentEntryId() ? last : null;
      showToast({ what: t('editor.outside_edit_kept', { name: baseName(path) }), severity: 'ShippedDegraded' }, {
        id: TOAST_ID,
        duration: KEPT_NOTICE_MS,
        actions: kept ? [{ label: t('editor.undo'), onClick: () => undo(kept) }] : undefined,
      });
    },
  };
}
