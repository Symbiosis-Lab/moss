// packages/moss-ui/src/ui/editor/session/no-content-pane.ts
//
// The surfaces the editor shows when the CM slot holds no document, and the
// one thing one of them has to DO.
//
// They look alike — a title, a sentence, sometimes a button — but they are not
// interchangeable, and a download shown as a missing page is what happens when they get treated as if
// they were. `generated` and `takeover` are claims about the SITE ("moss
// makes this page", "this page has no file yet; here is the one that would
// replace it"); `pending` claims only that moss has no answer yet, and its
// `download` reason, a claim about the NETWORK, is the only one that resolves
// itself. A user shown
// the wrong one has no way to tell which kind of claim they are reading: they
// were told a home page they could see in Finder did not exist.
//
// Keeping the copy here rather than in editor-main's surface switch is what
// makes that distinction reviewable in one screen. What does NOT belong here is
// `pending`'s side effect: a pane knows when it MOUNTS, and the shell needs to
// know when the pane is UP — which is not the same event, because a retry that
// lands back on a pane already on screen mounts nothing at all. That
// announcement lives in editor-main's surface switch, which sees both.

import { t } from "../../i18n/index";
import { mountPaneState } from '../pane-state';
import type { EditorSurface, Takeover, TakeoverKind } from './node-class';

/** The surfaces that render as a pane-state (title + sentence + optional CTA). */
export type NoContentPane = Extract<
  EditorSurface,
  { kind: 'takeover' } | { kind: 'pending' } | { kind: 'generated' }
>;

/**
 * Whether the shell should be watching the cloud stream for this editor.
 *
 * The editor webview never sees `icloud-sync` — the shell owns that listener
 * (see `preview/cloud/cloud-sync-store.ts`, "one listener, one state"). So the
 * editor says what it is showing and the shell re-offers the file on arrival,
 * extending the self-heal it already runs for the stale-cache race. Without
 * this the wrong surface outlives the condition that caused it, which is the
 * half the user felt longest: it stayed wrong until they
 * navigated again.
 *
 * Payload: `{ pending, sourcePath }`. Both directions ride ONE event on
 * purpose — a separate "stop watching" channel could overtake an arm emitted
 * just before it and leave the shell watching a pane that is gone, which is
 * the stale-arm bug wearing the fix's clothes. One channel, last message wins.
 * `sourcePath` names the file when the editor knows it; see
 * `EditorSurface.pending`.
 *
 * The emit itself is editor-main's.
 */
export const SOURCE_PENDING_EVENT = 'editor:source-pending';

/** The payload of {@link SOURCE_PENDING_EVENT}. */
export interface SourcePendingPayload {
  /** Is a still-downloading pane on screen right now? */
  pending: boolean;
  /** The file it is waiting for, when the editor knows it. */
  sourcePath: string | null;
}

/** The sentence and the button, per kind. The title is always the display name. */
const TAKEOVER_COPY: Record<TakeoverKind, { desc: Parameters<typeof t>[0]; cta: Parameters<typeof t>[0] }> = {
  'root-home': { desc: 'editor.takeover_root_desc', cta: 'editor.takeover_root_cta' },
  'folder-home': { desc: 'editor.takeover_folder_desc', cta: 'editor.takeover_folder_cta' },
  'term-page': { desc: 'editor.takeover_term_desc', cta: 'editor.takeover_term_cta' },
  'lang-home': { desc: 'editor.takeover_lang_desc', cta: 'editor.takeover_lang_cta' },
};

/**
 * What the term-page copy calls the field: the chip label the property bar
 * already shows for it (`Author`, `Editor`, `評審`). A site can declare any
 * field into any namespace, so the wording is interpolated rather than
 * written per namespace — which is why `author-page` and `tag-page` stopped
 * being kinds of their own.
 */
function takeoverParams(takeover: Takeover): Record<string, string> {
  const field = takeover.field;
  return {
    name: takeover.display,
    label: field === null ? '' : t(`chip.${field}.label` as Parameters<typeof t>[0]),
  };
}

export function mountNoContentPane(
  host: HTMLElement,
  surface: NoContentPane,
  actions: { takeOver: (takeover: Takeover) => void },
): void {
  switch (surface.kind) {
    case 'takeover': {
      const copy = TAKEOVER_COPY[surface.takeover.kind];
      const params = takeoverParams(surface.takeover);
      mountPaneState(host, {
        title: surface.takeover.display,
        description: t(copy.desc, params),
        cta: { label: t(copy.cta, params), onClick: () => actions.takeOver(surface.takeover) },
      });
      return;
    }
    case 'pending':
      mountPaneState(host, {
        title: t(surface.reason === 'download' ? 'editor.pane_pending_title' : 'editor.pane_loading_title'),
        description: t(surface.reason === 'download' ? 'editor.pane_pending_desc' : 'editor.pane_loading_desc'),
      });
      // No CTA on purpose. The waiting screen carries none either, for the same
      // reason: there is nothing the user can do about a download in flight,
      // and a button that only re-asks would make the wait feel like their
      // fault.
      return;
    case 'generated':
      mountPaneState(host, {
        title: t('editor.pane_generated_title'),
        description: t('editor.pane_generated_desc'),
      });
      return;
  }
}
