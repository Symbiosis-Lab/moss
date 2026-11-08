// Documents whose file left outside moss — renamed or deleted in Finder,
// another editor or a sync client.
//
// Two things must never happen: an autosave re-creating the file under its old
// name beside the renamed one, and unsaved text vanishing with the file.
//
//   gone  — a save of the open document found its file gone (the engine's
//           `gone` outcome). Saves to that path wait until the file event says
//           where it went: a rename re-keys the document and saving resumes.
//   held  — unsaved text whose file is gone, one record per path: the open
//           document's (live in the editor) after its file was deleted, or any
//           document the author left while it was gone (a snapshot). Each has
//           its own toast offering to save it back or discard it. A rename seen
//           later re-points the offer at the new name; nothing is written
//           without the author's say.

import { t } from "../../i18n/index";
import { baseName } from '../path-domain';
import { removeToastById, showToast } from "../../shared/ui/toast/toast-manager";

import type { DocumentText } from '../external-merge';

export type { DocumentText };

export interface OutsideFileHost {
  /** Unsaved text, or a save still waiting out its delay. */
  hasUnsavedText(): boolean;
  cancelPendingSave(): void;
  /** The open document's text as it stands. */
  liveText(): DocumentText;
  /** Create the page again; refuses a name something else has taken. */
  createFile(relPath: string): Promise<{ ok: true } | { ok: false; error: string }>;
  /** Write `text` over the page at `relPath`; with `keep`, keep what is there in history first. */
  writeText(relPath: string, text: DocumentText, keep: boolean): Promise<boolean>;
  /** Open `relPath` the way any open does. */
  reopen(relPath: string): Promise<void>;
  markClean(): void;
  showFolderOf(relPath: string): Promise<void>;
}

interface Held {
  /** Null while the document is open in the editor: its text is live there. */
  text: DocumentText | null;
  /** A file exists at the path to write into: one this offer created, or the
   *  file the original was renamed to. */
  fileExists: boolean;
  /** The name the file had when the text was last saved, once a rename moved it. */
  renamedFrom: string | null;
}

export interface OutsideFile {
  /** A save of the open document found its file gone at `relPath`. */
  noteGone(relPath: string): void;
  /** Whether saves to `relPath` must wait. */
  isGoneAt(relPath: string): boolean;
  /** Renames from a file event. Returns true when the open document moved off
   *  the gone path, so saving should resume at its new one. */
  followRenames(pairs: ReadonlyArray<readonly [string, string]>): boolean;
  /** The open document's file was deleted outside moss. Returns whether the
   *  document stays open (it held unsaved text). */
  deletedOutside(relPath: string): boolean;
  /** The author is leaving the open document: a gone or held one keeps its text. */
  leaving(): void;
  /** Whether the open document is one whose file is gone. */
  holdsDocument(): boolean;
}

const toastId = (path: string) => `editor-held-document:${path}`;

export function createOutsideFile(host: OutsideFileHost): OutsideFile {
  let gone: string | null = null;
  const held = new Map<string, Held>();

  function offer(path: string): void {
    const entry = held.get(path);
    if (!entry) return;
    const name = baseName(path);
    const what = entry.renamedFrom === null
      ? t('editor.deleted_outside_unsaved', { name })
      : t('editor.renamed_outside_unsaved', { old: baseName(entry.renamedFrom), name });
    showToast({ what, severity: 'NeedsAction' }, {
      id: toastId(path),
      persistent: true,
      dismissible: false,
      actions: [
        { label: entry.renamedFrom === null ? t('editor.save_it_back') : t('editor.save_into', { name }), variant: 'primary', onClick: () => void saveBack(path) },
        { label: t('editor.discard'), variant: 'secondary', onClick: () => void discard(path) },
      ],
    });
  }

  function hold(path: string, text: DocumentText | null): void {
    held.set(path, { text, fileExists: false, renamedFrom: null });
    offer(path);
  }

  function release(path: string): void {
    held.delete(path);
    removeToastById(toastId(path));
  }

  async function saveBack(path: string): Promise<void> {
    const entry = held.get(path);
    if (!entry) return;
    const text = entry.text ?? host.liveText();
    if (!entry.fileExists) {
      const created = await host.createFile(path);
      if (!created.ok) {
        showToast({ what: t('editor.save_back_failed', { name: baseName(path), error: created.error }), severity: 'NeedsAction' });
        return;
      }
      // Ours now: a retry after a failed write must not trip over it.
      entry.fileExists = true;
    }
    // The page the file was renamed to holds another writer's text, kept before it is replaced.
    if (!await host.writeText(path, text, entry.renamedFrom !== null)) {
      showToast({ what: t('editor.save_back_failed', { name: baseName(path), error: t('editor.save_back_write_failed') }), severity: 'NeedsAction' });
      return;
    }
    release(path);
    if (entry.text === null) { host.markClean(); await host.reopen(path); }
  }

  async function discard(path: string): Promise<void> {
    const entry = held.get(path);
    if (!entry) return;
    release(path);
    if (entry.text === null) { host.markClean(); await host.showFolderOf(path); }
  }

  return {
    noteGone(relPath) { gone = relPath; },
    isGoneAt(relPath) { return gone === relPath || held.has(relPath); },
    followRenames(pairs) {
      let resumed = false;
      for (const [from, to] of pairs) {
        if (gone === from) { gone = null; resumed = true; }
        const entry = held.get(from);
        if (entry && entry.text !== null) {
          // The author had moved on: the offer follows the file, and writing
          // over the renamed file stays the author's choice.
          release(from);
          held.set(to, { ...entry, fileExists: true, renamedFrom: entry.renamedFrom ?? from });
          offer(to);
        }
      }
      return resumed;
    },
    deletedOutside(relPath) {
      if (!host.hasUnsavedText() && gone !== relPath) {
        showToast({ what: t('editor.deleted_outside', { name: baseName(relPath) }), severity: 'ShippedDegraded' });
        return false;
      }
      gone = null;
      host.cancelPendingSave();
      hold(relPath, null);
      return true;
    },
    leaving() {
      if (gone !== null) { hold(gone, host.liveText()); gone = null; }
      for (const entry of held.values()) if (entry.text === null) entry.text = host.liveText();
    },
    holdsDocument() { return [...held.values()].some((e) => e.text === null); },
  };
}
