/**
 * The Browse button on the asset-completion tooltip.
 * A short header strip above the completion list, carrying the same button the
 * chip bar's cover field carries. The list below is untouched.
 *
 * This is the one place moss decorates someone else's DOM: the completion
 * tooltip belongs to @codemirror/autocomplete and has no header API. The strip
 * is therefore attached by a view plugin that watches for
 * `.cm-tooltip-autocomplete` to appear, and removes itself the moment the
 * caret stops asking for a file. Two consequences worth stating out loud:
 *
 *  - **Nothing in the strip may take focus.** Focusing anything inside the
 *    tooltip blurs the editor, and CM6 closes the completion under the user.
 *    Hence `tabindex="-1"`, and the button's own mousedown `preventDefault`.
 *  - **The keyboard route is a keymap entry, not Tab.** For the same reason:
 *    there is no tab stop to reach.
 */

import { type EditorState, type Extension } from '@codemirror/state';
import { EditorView, ViewPlugin, keymap, type ViewUpdate } from '@codemirror/view';
import { closeCompletion } from '@codemirror/autocomplete';
import type { CompletionPhase, ExtKind } from '@symbiosis-lab/moss-syntax';
import { browseButton, canBrowse, type BrowseContext } from './browse-button';
import {
  applyCompletion,
  asksForFile,
  assetKindsFor,
  parseCompletionContext,
} from '../cm-completion';
import type { WikilinkCompletion } from '@symbiosis-lab/moss-ui';

/** What the editor knows; asked at pick time, never cached. */
export interface AssetBrowseOptions {
  pagePath(): string | null;
  projectRoot(): string | null;
  /** Refresh the file tree after a file is imported into the vault. */
  afterImport?(): Promise<void>;
}

const STRIP_CLASS = 'cm-asset-browse-strip';

/** Where a picked file would land: the phase to re-check, the kinds the dialog offers. */
export interface PickSite {
  ctx: CompletionPhase;
  /** `null` means every embeddable asset (an `![[` embed). */
  kinds: ExtKind[] | null;
  from: number;
  to: number;
}

/**
 * The pick site at `pos`, or `null` when the caret is not asking for a file
 * (prose, a `[[` link, a `per-line=` attr, a block body that takes no assets).
 *
 * Re-parses the LIVE document rather than remembering what the completion
 * was built from: the dialog is modal and slow by human standards, and a
 * save, an external edit, or the author's own last keystroke can move the
 * token in between. `parseCompletionContext` and `assetKindsFor` are the
 * functions the completion source uses, so the button and the dropdown can
 * never disagree about what the caret is in.
 */
export function pickSiteAt(state: EditorState, pos: number): PickSite | null {
  const line = state.doc.lineAt(pos);
  const ctx = parseCompletionContext(state.doc, line.number, pos - line.from);
  if (!ctx || !asksForFile(ctx)) return null;
  return { ctx, kinds: assetKindsFor(ctx), from: line.from + ctx.from, to: pos };
}

/**
 * Land a picked file through the SAME accept path an accepted completion
 * takes — including closing an `![[`'s brackets. The dispatch carries no
 * `userEvent: 'input.type'`: this is not typing, and labelling it so would
 * re-trigger the very completion the pick just answered.
 */
function landPick(view: EditorView, item: WikilinkCompletion): void {
  // The view can be gone: a CLI-routed folder switch remounts the editor,
  // and dispatching into a destroyed view throws.
  if (!view.dom.isConnected) return;
  // Re-read the caret's site — the document may have moved while the dialog
  // was up. If it is no longer asking for a file, DROP the pick: the captured
  // range was computed against a document that no longer exists, and
  // dispatching it would splice the reference into whatever now occupies
  // those offsets.
  const now = pickSiteAt(view.state, view.state.selection.main.head);
  if (!now) return;
  applyCompletion(view, now.from, now.to, item, now.ctx);
  view.focus();
}

/**
 * Open the dialog for whatever the caret is currently asking for, and land the
 * result. Shared by the strip's button and the keymap entry; returns false
 * when the caret is not in an asset position, so the chord falls through.
 */
function browseHere(view: EditorView, opts: AssetBrowseOptions): boolean {
  const site = pickSiteAt(view.state, view.state.selection.main.head);
  if (!site || !canBrowse()) return false;
  const btn = browseButton({
    variant: 'strip',
    ctx: contextFor(view, opts, site.kinds),
    onOpen: () => closeCompletion(view),
    onPicked: (item) => landPick(view, item),
  });
  btn.click();
  return true;
}

function contextFor(
  view: EditorView,
  opts: AssetBrowseOptions,
  kinds: string[] | null,
): BrowseContext {
  return {
    pagePath: () => opts.pagePath() ?? '',
    projectRoot: () => opts.projectRoot(),
    kinds: () => kinds,
    afterImport: opts.afterImport,
  };
}

/**
 * The strip itself, plus the keyboard route. Add to the editor's extensions
 * after `autocompletion()`.
 */
export function assetBrowseExtension(opts: AssetBrowseOptions): Extension {
  const plugin = ViewPlugin.fromClass(
    class {
      private strip: HTMLElement | null = null;

      /**
       * CM6 inserts the completion tooltip as a DIRECT child of `view.dom`
       * (moss configures no `tooltips({parent})`), and it does so during its
       * own DOM update — which may land after this plugin's `update` has run.
       * So the observer is what actually catches the tooltip appearing;
       * `update` covers the case where it is already there and the caret moved
       * out of an asset position. `childList` without `subtree` is deliberate:
       * the content DOM mutates on every keystroke and is none of our business.
       */
      private readonly observer = new MutationObserver(() => this.sync());

      constructor(readonly view: EditorView) {
        this.observer.observe(view.dom, { childList: true });
        this.sync();
      }
      update(u: ViewUpdate): void { if (u.docChanged || u.selectionSet || u.transactions.length) this.sync(); }
      destroy(): void { this.observer.disconnect(); this.remove(); }

      private sync(): void {
        const tooltip = this.view.dom.querySelector<HTMLElement>('.cm-tooltip-autocomplete');
        const site = tooltip
          ? pickSiteAt(this.view.state, this.view.state.selection.main.head)
          : null;
        if (!tooltip || !site || !canBrowse()) { this.remove(); return; }
        if (this.strip?.parentElement === tooltip) return;
        this.remove();
        const strip = document.createElement('div');
        strip.className = STRIP_CLASS;
        strip.tabIndex = -1;
        strip.appendChild(browseButton({
          variant: 'strip',
          ctx: contextFor(this.view, opts, site.kinds),
          onOpen: () => closeCompletion(this.view),
          onPicked: (item) => landPick(this.view, item),
        }));
        tooltip.insertBefore(strip, tooltip.firstChild);
        this.strip = strip;
      }

      private remove(): void {
        this.strip?.remove();
        this.strip = null;
      }
    },
  );

  return [
    plugin,
    // `Mod-o` — "open a file", and free in moss's editor keymap. Returns false
    // outside an asset position so it never swallows the chord.
    keymap.of([{ key: 'Mod-o', run: (view) => browseHere(view, opts) }]),
  ];
}
