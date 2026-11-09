// packages/moss-ui/src/ui/editor/preview-note.ts
//
// The one line above the editor saying why the preview did not move.
// PreviewFollower is its sole writer: it is the only thing that knows the
// prospect AND the resolve outcome, so a second writer would race it — which
// is exactly how a `.moss/theme` image once got told it was "not a page".

import { t } from "../i18n/index";
import type { PreviewNoteReason } from './preview-prospect';

const PREVIEW_NOTE_KEYS = {
  'site-wide': 'editor.preview_note_site_wide',
  'not-a-page': 'editor.preview_note_not_a_page',
} as const;

/** Writing `data-i18n` as well as the text keeps the line correct across a
 *  live locale switch, which re-reads that attribute. */
export function setPreviewNote(reason: PreviewNoteReason | null): void {
  const el = document.getElementById('preview-note');
  if (!el) return;
  if (reason) {
    const key = PREVIEW_NOTE_KEYS[reason];
    el.dataset.i18n = key;
    el.textContent = t(key);
  }
  el.hidden = reason === null;
}
