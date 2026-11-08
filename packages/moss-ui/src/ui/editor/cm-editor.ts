// CM6 integration — thin wrapper around CodeMirror 6.
// Supports markdown (with custom extensions) and other web languages.

import { EditorView, minimalSetup } from 'codemirror';
import { dropCursor, keymap, placeholder, type ViewUpdate } from '@codemirror/view';
import { search } from '@codemirror/search';
import { html } from '@codemirror/lang-html';
import { javascript } from '@codemirror/lang-javascript';
import { css } from '@codemirror/lang-css';
import { json } from '@codemirror/lang-json';
import { Annotation, ChangeSet, Compartment, EditorState, Extension, Prec, Transaction } from '@codemirror/state';
import { StreamLanguage } from '@codemirror/language';
import { isolateHistory, historyField } from '@codemirror/commands';
import type { EditorReferenceResolution } from '@symbiosis-lab/moss-ui';
import { toml } from '@codemirror/legacy-modes/mode/toml';
import { t } from "../i18n/index";
import { applyStaticStrings } from "../i18n/apply-static-strings";
import { UNDO_KEY, REDO_KEY } from "../utils/kbd-label";
import { installBackgroundClickCaret } from './cm-background-click';
import { shortcodeDivergenceCheck } from './cm-shortcode-scanner';
import {
  shortcodeBlockExtension,
  mossHighlightExtension,
  linkClickableExtension,
} from '@symbiosis-lab/moss-syntax/cm6';
import { createApplyEmbedWidth } from './embed-width';
import { followFilePath, followTarget, linkNavExtension, navigateToLinkTarget, navigateToReference, chordTrackingExtension } from './cm-link-nav';
import { wikilinkCompleteExtension, type WikilinkCompleteOptions } from './cm-wikilink-complete';
import { assetBrowseExtension, type AssetBrowseOptions } from './asset-picker/completion-strip';
import { insertBarExtension, type InsertBarHooks } from './cm-insert-bar';
import { quietChromeExtension } from './editor-quiet-chrome';
import { expandDerivedExtension, expandViaBackend } from './expand-derived/extension';
import { blockEmbedsExtension, type ResolveEmbedFn, type NavigateLinkFn, type CanFollowLinkFn } from './cm-block-embeds';
import type { ResolveImageFn } from './cm-block-embed-widgets';
import { actAtCaretExtension } from './cm-act-at-caret';
import { createReferenceResolver, type FileEventsCoordinator } from './cm-reference-resolver';
import { insertCommentAndEdit } from './cm-comment';
import { detectFileType, type EditorFileType } from './file-types';
import {
  referenceCardExtension, assetCardProvider, footnoteCardProvider,
  externalCardProvider, linkCardProvider, pageLinkCardProvider, cardProviders, cardAugmenters,
  currentArticleCardProvider, isCurrentArticleMapAt, openCardAt, withLinkControls, type CurrentArticleCardData, type CardProvider,
} from './cm-reference-card';
import { problemCardAugmenter, problemsExtension } from './cm-problems';
import { toRelative } from './path-domain';
import { pasteExtension, type PasteOptions } from './cm-paste';
import { assetBustQuery } from './asset-bust';
import { sourceAssetUrl } from './source-asset-url';
import { jsonEditingExtension } from './cm-json';
import { osDropCaret, blockDropCursor } from './cm-os-drop-caret';
import { clearOutsideWashLater, outsideWashOf, revealFlashExtension } from './cm-reveal-flash';
import { dropExtension, type DestinationFor } from './cm-drop';
import { focusPointExtension, pictureAtCaretField, heroWordsExtension, sourceMarkdownExtensions } from './cm';
import { host as uiHost } from "../host";
import { engine } from '../engine';

export { MARKDOWN_EXTENSIONS } from './cm';

/** Resolve a unified-cache reference envelope to a renderable image, or
 *  undefined. Reads the `resolved` ResolvedAsset off the (text, is_embed:true)
 *  envelope — the SINGLE reference cache (cm-reference-resolver). Guards against
 *  envelopes without an image-mime asset (unresolved / non-image kinds) so an
 *  unguarded `.startsWith` can never crash the live-preview ViewPlugin. */
export function resolveCachedImage(
  env: EditorReferenceResolution | undefined,
  bustQuery = '',
): { url: string; width?: number; height?: number } | undefined {
  const a = env?.resolved;
  if (!a || typeof a.mime_type !== 'string' || !a.mime_type.startsWith('image/')) return undefined;
  return {
    url: sourceAssetUrl(a.request_url, bustQuery),
    width: a.width ?? undefined,
    height: a.height ?? undefined,
  };
}

function languageExtension(fileType: Exclude<EditorFileType, 'markdown'>, filePath?: string): Extension {
  switch (fileType) {
    case 'html': return html();
    case 'javascript': {
      const ext = filePath?.split('.').pop()?.toLowerCase();
      return javascript({
        typescript: ext === 'ts' || ext === 'tsx',
        jsx: ext === 'jsx' || ext === 'tsx',
      });
    }
    case 'css': return css();
    case 'json': return [json(), jsonEditingExtension()];
    case 'toml': return StreamLanguage.define(toml);
    case 'plain': return [];
  }
}

// Marks a transaction as originating from outside the user — a file-changed
// reload, a remote sync, etc. The updateListener uses this to skip the
// onChange callback (which drives auto-save); we do not want external content
// updates to schedule a spurious save.
const externalUpdate = Annotation.define<true>();

// `applyEmbedWidth` lives in embed-width.ts as a factory over `setImageWidth`,
// so cm-reference-card.ts's width-preset actions use the SAME function. This
// module builds the one real instance from the engine command, uses it below
// (`onSetWidth`) and hands it to `assetCardProvider` as an option
// (registration block below) — exported too, so existing imports of
// `applyEmbedWidth` from this module keep working.
export const applyEmbedWidth = createApplyEmbedWidth(async (md, width) => {
  const r = await engine.setImageWidth(md, width);
  return r.status === 'ok' ? r.data : md;
});

export interface CmEditorOptions {
  content: string;
  container: HTMLElement;
  onChange: (gestureEnd: boolean) => void; // gestureEnd: an instrument wrote it, ending a gesture rather than a typing burst
  /**
   * Every view update, for the combined document history's reconciliation.
   * Separate from onChange because it must NOT skip a non-doc update: the
   * depth-floor clamp is edge-triggered on the update that lowers the depth,
   * and a listener that filters on docChanged would never see a prune.
   */
  onHistoryUpdate?: (update: ViewUpdate) => void;
  /** A previous `toJSON()` of this file; its text, selection and undo history replace `content`. */
  restore?: unknown;
  /**
   * The caret moved, or an edit moved it. Drives the caret half of
   * editor→preview scroll sync (`editor-main.ts`), which is what makes the
   * preview follow the line being worked on rather than only the scrollbar.
   *
   * An option rather than something the caller appends later: a config
   * appended through `view.dispatch` needs a real `EditorView`, and the boot
   * tests hand `editor-main` a stand-in with no `dispatch`.
   */
  onCaretMove?: () => void;
  /**
   * ⌘Z / ⌘⇧Z pressed with the caret in the body. Return true if handled.
   *
   * Registered unconditionally, with no platform branch, and that is a fix
   * rather than an oversight. moss's Edit menu — which claims the keystroke
   * before WebKit sees it and broadcasts `menu:edit-undo` — is macOS-ONLY
   * (`startup.rs` gates the whole module). On Linux and Windows no menu exists,
   * so without this keymap Ctrl+Z would reach CodeMirror's own history and
   * quietly undo only the body half, diverging from what the cluster's buttons
   * do on the same machine. On macOS the accelerator wins first and this never
   * fires; the history's re-entrancy guard makes a double delivery harmless if
   * some configuration ever sends both.
   */
  onHistoryKey?: (direction: 'undo' | 'redo') => boolean;
  /**
   * Getter for the registry-relative path of the file being edited.
   * Called at resolve time (not captured at construction) so resolution
   * remains correct after a cross-directory move of the open file.
   * Required for link/asset resolution.
   */
  getFromFile?: () => string;
  /**
   * Getter for the absolute project root (vault folder), or `null` when no
   * folder is open. Threaded into the reference card's link/image providers
   * A page-link card's backend call and an image card's
   * Replace… picker both need it, the same accessor shape
   * `editor-main.ts`'s own `projectRoot: () => projectRoot` already uses for
   * the insert bar's media picker.
   */
  getProjectRoot?: () => string | null;
  /** The page's front matter and the way to write fields back through its store ("Expand" reads and updates the inferred title, description and date). */
  getFrontmatter?: () => Record<string, unknown>;
  applyFrontmatter?: (patch: { title?: string; description?: string; date?: string; inferred?: string[] | null }) => void;
  /** The title, description and date the page's embeds would give, for the chip bar to show as suggestions. */
  onDerivedFacts?: (facts: { title?: string; description?: string; date?: string }) => void;
  /** Current document title and location values, read from their existing owners. */
  getCurrentArticle?: () => CurrentArticleCardData;
  /**
   * Persistent bottom toolbar (markdown only). Carries the I/O hooks the
   * bar's picker-backed inserts need (dialog + vault import live in
   * editor-main, per file-first architecture); the comment action is wired
   * here from cm-comment. See cm-insert-bar.ts.
   */
  insertBar?: Omit<InsertBarHooks, 'onComment'>;
  /**
   * Called from `destroy()` BEFORE the underlying `EditorView.destroy()`
   * nukes the scrollDOM. Use this to hand off any DOM children that have
   * been re-parented INTO `view.scrollDOM` (e.g. the editor heading) so
   * they survive the destroy.
   */
  onBeforeDestroy?: () => void;
  /** Wikilink + section heading autocomplete (markdown only). */
  wikilinkComplete?: WikilinkCompleteOptions;
  /** "Browse files…" on the asset-completion tooltip (markdown only). Composed
   *  here, not inside `wikilinkCompleteExtension` — the strip's context module
   *  imports that one, so wiring it there would close an import cycle. */
  assetBrowse?: AssetBrowseOptions;
  /**
   * Persist a pasted clipboard image into the project (markdown only) —
   * see cm-paste.ts. Receives (suggestedName, base64) and returns the stored
   * file's project-relative path, or null on failure.
   */
  pasteImage?: PasteOptions['saveImage'];
  /**
   * A project file's standard-form destination from the open page, for the
   * embeds a drop, paste or Media pick inserts (cm-drop.ts `DestinationFor`).
   */
  destinationFor?: DestinationFor;
  /**
   * Optional EditorFileEvents coordinator. When provided, it is threaded into
   * the reference resolver so all FileChanged/AssetReady subscriptions flow
   * through ONE shared coordinator rather than each subsystem self-subscribing.
   */
  fileEvents?: FileEventsCoordinator;
  /**
   * Resolve file-tree entry IDs to project-relative paths for the internal
   * tree→editor drop (cm-drop.ts). Called with an array of entry
   * IDs from the `application/x-moss-entries` dataTransfer payload. A
   * directory's path ends with `/` so it produces `![[folder/]]`. Omitting
   * this disables the tree→editor drop entirely (no extension is registered).
   */
  resolveEntries?: (entryIds: number[]) => string[];
  /**
   * Additional CM6 extensions appended AFTER all built-in extensions. Use this
   * to inject caller-specific behaviour (e.g. email guard rules) without
   * modifying the shared wrapper. Extensions are applied in order so they can
   * override or augment any built-in facet.
   */
  extraExtensions?: Extension[];
  /**
   * Enable the CriticMarkup comment UI (⌘⇧M keymap, gutter markers, popovers,
   * and highlight decorations). Defaults to `true` when omitted. Pass
   * `comments: false` to disable comment authoring entirely — e.g. in the email
   * composer where CriticMarkup comments are stripped at render time and the UI
   * would be misleading.
   */
  comments?: boolean;
  /**
   * Move focus UP out of the editor into the title above it (Obsidian
   * inline-title parity — the mirror of the heading's ArrowDown-descend into
   * `focusBody`). Invoked on ArrowUp ONLY when the main selection is an empty
   * cursor on line 1. Return `true` when focus actually moved (the ArrowUp is
   * then consumed); return `false` to let CM6's default ArrowUp run (e.g. the
   * title is hidden). Optional — omit for surfaces that have no title above the
   * editor (e.g. the email composer).
   */
  onEscapeTop?: () => boolean;
}

/** Why the editor is frozen; picks the scrim caption. */
export type SealReason = 'publish' | 'commit';

const SEAL_CAPTION: Record<SealReason, string> = {
  publish: 'editor.seal_publishing',
  commit: 'editor.seal_committing',
};

export class CmEditor {
  private view: EditorView;
  private onBeforeDestroy?: () => void;
  private readonly uninstallBackgroundClick: () => void;
  /** Reconfigurable EditorView.editable (read-only without remount). */
  private readonly editableCompartment = new Compartment();

  constructor(options: CmEditorOptions) {
    // Resolve the path once at construction for language/file-type detection.
    // This is construction-time only (choosing a language extension); all
    // resolver call sites use getFromFile() live.
    const fromFile = options.getFromFile?.();
    const fileType = fromFile ? detectFileType(fromFile) : 'markdown';
    const isMarkdown = fileType === 'markdown';
    const hasInsertBar = isMarkdown && !!options.insertBar;

    const extensions: Extension[] = [
      // editable is reconfigured live by setReadOnly().
      this.editableCompartment.of(EditorView.editable.of(true)),
      minimalSetup,
      // Drop-caret feedback while dragging files/text over the editor.
      // Non-markdown files use the built-in dropCursor() (vertical inline caret)
      // since they take no embeds at all. Markdown files use
      // blockDropCursor() (horizontal block-boundary line) registered below,
      // which REPLACES dropCursor() for tree→editor drags.
      ...(isMarkdown ? [] : [dropCursor()]),
      search({ top: false }),
      ...(fileType === 'markdown' ? [] : [languageExtension(fileType, fromFile), mossHighlightExtension()]),
      // Single-click reveal wash from the preview (any file type — a css/js
      // file opened via a page's asset link maps clicks too).
      ...revealFlashExtension(),
      EditorView.lineWrapping,
      ...(options.onHistoryKey
        ? [
            // Above minimalSetup's historyKeymap, which would otherwise take
            // Mod-z first and undo the body alone.
            Prec.highest(
              keymap.of([
                { key: UNDO_KEY, run: () => options.onHistoryKey!('undo') },
                { key: REDO_KEY, run: () => options.onHistoryKey!('redo') },
                { key: 'Mod-y', run: () => options.onHistoryKey!('redo') },
              ]),
            ),
          ]
        : []),
      EditorView.updateListener.of((update) => {
        options.onHistoryUpdate?.(update);
        // Skip external-origin changes (applyExternal): another writer's change
        // is neither an edit to auto-save nor a caret the user moved — the
        // caret maps through it, and that must not send the preview anywhere.
        const external = update.transactions.some(tr => tr.annotation(externalUpdate));
        if (external) return;
        if (update.selectionSet || update.docChanged) options.onCaretMove?.();
        if (update.docChanged) options.onChange(update.transactions.some((tr) => tr.isUserEvent('input.instrument')));
      }),
      // Tell CM6's scrollIntoView about the editor heading we re-parent
      // into `.cm-scroller`. Without this, `view.dispatch({ scrollIntoView: true })`
      // for line 1 thinks the line is fully visible at scrollTop=0 even
      // though the heading is covering it. With this, CM6 reserves the
      // top `headingHeight` px as a margin, so the cursor scrolls below
      // the heading. Returning null when there's no heading keeps the
      // default (no margin).
      EditorView.scrollMargins.of(view => {
        const top = view.contentDOM.offsetTop;
        // Keep the caret clear of the fixed bottom insert bar (36px pill +
        // 16px inset) so typing near the viewport bottom never puts fresh
        // text under it.
        const bottom = hasInsertBar ? 64 : 0;
        if (top <= 0 && bottom <= 0) return null;
        return { top: Math.max(0, top), bottom };
      }),
      // ArrowUp escape into the title (Obsidian inline-title parity — the mirror
      // of the heading's ArrowDown-descend). When the caret is an EMPTY cursor
      // on line 1 and a title is wired, ArrowUp hands off to `onEscapeTop`
      // (which focuses the title) instead of doing nothing at the top of the
      // doc. Registered for ALL file types because the title sits above every
      // loaded document, not just markdown.
      //
      // Precedence: `Prec.high` sits BELOW the slash menu's `Prec.highest`
      // ArrowUp binding (cm-slash-menu.ts) — so an open slash menu still gets
      // ArrowUp to move its selection — but ABOVE `minimalSetup`'s default
      // cursor keymap (`Prec.default`), so we intercept BEFORE `cursorLineUp`
      // runs. A non-empty selection, a caret off line 1, `onEscapeTop`
      // returning false (title hidden), or an in-progress IME composition all
      // fall through (return false) to the default ArrowUp.
      ...(options.onEscapeTop
        ? [Prec.high(keymap.of([{
            key: 'ArrowUp',
            run: (view) => {
              if (view.composing) return false; // never hijack an IME candidate nav
              const { main } = view.state.selection;
              if (!main.empty) return false;
              if (view.state.doc.lineAt(main.head).number !== 1) return false;
              return options.onEscapeTop?.() ?? false;
            },
          }]))]
        : []),
      // Backspace at the very start of the document joins the title: the title
      // and the body are one surface, so the first line has no "line above" to
      // refuse to merge into. Only at true position 0 with an empty cursor — a
      // hung heading/quote's caret sits at its text start (cm-head-fit), so
      // stepBackspace still peels one level first. A blank line 1 loses its
      // break (one undo step); a line with text keeps it, and is never merged
      // into the title, which is frontmatter. Focus moves first so a hidden
      // title (onEscapeTop false) leaves CM6's default Backspace untouched.
      ...(options.onEscapeTop
        ? [Prec.high(keymap.of([{
            key: 'Backspace',
            run: (view) => {
              if (view.composing) return false;
              const { main } = view.state.selection;
              if (!main.empty || main.head !== 0) return false;
              if (!options.onEscapeTop?.()) return false;
              const { doc } = view.state;
              if (doc.length > 0 && doc.line(1).length === 0) {
                view.dispatch({ changes: { from: 0, to: 1 }, userEvent: 'delete.backward', annotations: isolateHistory.of('full') });
              }
              return true;
            },
          }]))]
        : []),
      ...(isMarkdown ? sourceMarkdownExtensions({ comments: options.comments }) : []),
    ];

    // Markdown-only extensions
    if (isMarkdown) {
      // Empty-document ghost line (Notion pattern): the one moment a reader is
      // guaranteed to look at nothing is the moment to advertise the slash
      // menu. CM6 removes it natively on first input. The function form is
      // re-evaluated each time the placeholder is (re)created, so a locale
      // switch stays live without a remount.
      extensions.push(placeholder(() => {
        const el = document.createElement('span');
        el.textContent = t('editor.empty_placeholder');
        return el;
      }));
      // Paste verbs: bare-URL-over-selection → link; clipboard image → save+insert.
      extensions.push(pasteExtension({ saveImage: options.pasteImage, destinationFor: options.destinationFor }));

      // Reference resolver (#3) — the ONE push adapter routing BOTH embeds and
      // links through the shared `classify_reference` kernel. It owns the single
      // (text, is_embed)-keyed cache + lifecycle (100ms batch debounce,
      // FileChanged invalidation). EVERY downstream consumer — image render,
      // problem marks, hover, cmd-click nav — reads THIS cache. There is no
      // second asset/link resolver.
      let resolveImage: ResolveImageFn | undefined;
      let resolveEmbed: ResolveEmbedFn | undefined;
      let navigateLink: NavigateLinkFn | undefined;
      let canFollowLink: CanFollowLinkFn | undefined;
      if (options.getFromFile) {
        const refResolver = createReferenceResolver({
          getFromFile: options.getFromFile,
          fileEvents: options.fileEvents,
        });
        extensions.push(...refResolver.extension);
        // `kind`-routing for the live-preview: look up the embed-context
        // envelope and hand the classified tag (+ resolved source URL for
        // video/audio) to the image-block builder. The `src` field enables the
        // native player path in MediaEmbedBlockWidget. It is always a
        // moss-source:// URL produced by sourceAssetUrl — never raw user input.
        resolveEmbed = (target) => {
          const env = refResolver.resolvedCache.get(target, true);
          if (!env) return undefined;
          const kind = env.kind.kind;
          // Forward the source URL for playable media kinds so the widget can
          // render a native <video>/<audio> element. Other kinds get no `src`
          // (the placeholder card path).
          // Followable exactly when the envelope names a source file — one
          // predicate, so the card's affordance and the click agree.
          const follow = followFilePath(env)
            ? () => { navigateToReference(refResolver.resolvedCache, { text: target, isEmbed: true }); }
            : undefined;
          if ((kind === 'video' || kind === 'audio') && env.resolved?.request_url) {
            const bustQuery = assetBustQuery(env.resolved.request_url);
            return { kind, src: sourceAssetUrl(env.resolved.request_url, bustQuery), follow };
          }
          return { kind, folder: env.folder, follow };
        };

        // Every content problem (missing, moved, mismatched, ambiguous) as one
        // record, marked on its span and explained in the reference card.
        // Reads the SAME shared cache; build evidence arrives from editor-main.
        extensions.push(problemsExtension({
          resolvedCache: refResolver.resolvedCache,
          refForPath: uiHost.assetRefForPath && (async (absolutePath) => {
            const page = options.getFromFile?.();
            const root = options.getProjectRoot?.();
            if (!page || !root) return null;
            return (await uiHost.assetRefForPath?.(absolutePath, toRelative(page, root)))?.insert ?? null;
          }),
          projectRoot: () => options.getProjectRoot?.() ?? null,
        }));
        // Cmd+Click nav reads `resolved_path` off the same envelopes; the
        // clickable mark needs no resolution at all.
        extensions.push(linkClickableExtension());
        extensions.push(linkNavExtension(refResolver.resolvedCache, (target) => {
          if (!target.isEmbed || !options.getCurrentArticle || !isCurrentArticleMapAt(this.view.state, target.from)) return false;
          const article = options.getCurrentArticle();
          if (article.locations.length) article.revealLocations();
          else openCardAt(this.view, target.from);
          return true;
        }));
        // A link-wrapped embed renders as one clickable image; Cmd/Ctrl+click on
        // it must land exactly where the same href would from link TEXT, so it
        // shares linkNavExtension's rule rather than re-deriving one.
        navigateLink = (href) => navigateToLinkTarget(refResolver.resolvedCache, href);
        // The affordance half of the same rule: a card wrapped in an external,
        // anchor-only or not-yet-written link has nowhere to go, and must not
        // paint a hand or offer an Open that does nothing.
        canFollowLink = (href) =>
          followTarget(refResolver.resolvedCache.get(href, false)) !== null;

        // Render images directly from source via moss-source:// — no preview
        // server required. Images are always available regardless of server state.
        resolveImage = (target) => {
          const env = refResolver.resolvedCache.get(target, true);
          const requestUrl = env?.resolved?.request_url ?? '';
          return resolveCachedImage(env, assetBustQuery(requestUrl));
        };
        // The reference card: dwell on a link or, chord-held, any reference —
        // reading the SAME unified reference cache the problem marks read.
        // Keyed (text, is_embed).
        //
        // The three link-kind providers (page link, external, and
        // the last-resort link card) each gain a URL field + Open/Unlink through
        // `withLinkControls`, wrapped HERE rather than inside
        // cm-reference-card.ts — the wrap needs the SAME `resolvedCache` the
        // providers already close over.
        const withControls = (base: CardProvider): CardProvider =>
          (state, pos) => {
            const spec = base(state, pos);
            return spec && withLinkControls(spec, state, pos, refResolver.resolvedCache);
          };
        const pathOpts = { fromFile: () => options.getFromFile?.() ?? '' };
        extensions.push(
          referenceCardExtension(),
          chordTrackingExtension(),
          ...(options.getCurrentArticle ? [cardProviders.of(currentArticleCardProvider(options.getCurrentArticle))] : []),
          cardProviders.of(assetCardProvider(refResolver.resolvedCache, {
            pagePath: () => options.getFromFile?.() ?? '',
            projectRoot: () => options.getProjectRoot?.() ?? null,
            applyWidth: applyEmbedWidth,
          })),
          cardProviders.of(footnoteCardProvider),
          cardProviders.of(withControls(externalCardProvider(refResolver.resolvedCache))),
          cardProviders.of(withControls(
            pageLinkCardProvider(refResolver.resolvedCache, engine.editorReferenceCard, pathOpts),
          )),
          cardProviders.of(withControls(linkCardProvider)),
          // A problem is not a provider: it adds its sentence and fixes to
          // whichever card claims the position, so a missing image keeps the
          // asset card's Replace… and a moved link keeps the page card.
          cardAugmenters.of(problemCardAugmenter),
        );
      }

      // `onSetWidth` performs the drag-resize / double-click-reset doc edit.
      // `this.view` is assigned at the end of the constructor (after this
      // extension is built), so read it lazily at gesture time.
      extensions.push(blockEmbedsExtension({
        resolveImage,
        resolveEmbed,
        navigateLink,
        canFollowLink,
        onSetWidth: (from, _to, width) => { void applyEmbedWidth(this.view, from, width); },
      }));
      const currentFile = () => options.getFromFile?.() ?? '';
      const host = uiHost.bus;
      extensions.push(pictureAtCaretField);
      extensions.push(focusPointExtension({
        path: currentFile, host,
        resolveAsset: async (target) => { const r = await engine.editorResolveAsset(target, currentFile()); return r.status === 'ok' ? r.data : null; },
      }));
      extensions.push(heroWordsExtension(currentFile, host));
      extensions.push(expandDerivedExtension({ label: () => t('editor.expand_derived'), getFrontmatter: options.getFrontmatter, applyFrontmatter: options.applyFrontmatter, onFacts: options.onDerivedFacts, expand: expandViaBackend(engine.expandDerived, () => options.getFromFile?.()) }));
      extensions.push(actAtCaretExtension());
      extensions.push(shortcodeBlockExtension({
        // Thunks, not values: the widgets call these at toDOM time (exactly
        // where t() used to be called), so a locale switch stays live.
        strings: {
          legacyDividerLabel: () => t('shortcode.divider.legacy_label'),
          legacyDividerTooltip: () => t('shortcode.divider.legacy_tooltip'),
        },
      }));
      extensions.push(shortcodeDivergenceCheck());
      // OS-drag horizontal drop indicator driven from Tauri over-coords.
      // dropCursor() cannot see OS drags; osDropCaret() handles them instead.
      extensions.push(...osDropCaret());
      // Internal tree→editor drag indicator: replaces the built-in dropCursor()
      // (omitted for markdown above) with the same horizontal block-boundary
      // line so both drag paths show identical UI.
      extensions.push(...blockDropCursor());

      // Internal tree→editor drop: reads
      // `application/x-moss-entries` from the dataTransfer and inserts
      // an embed per entry at the block-snapped drop position. OS file drops
      // (`Files` type) are returned false — they are handled by the Tauri path.
      // Only registered when a `resolveEntries` resolver is provided.
      if (options.resolveEntries) {
        extensions.push(dropExtension({
          resolveEntries: options.resolveEntries,
          destinationFor: options.destinationFor ?? (async () => null),
        }));
      }
    }

    // Persistent bottom toolbar: markdown-only (inserts + formatting use
    // markdown syntax). Comment routes to cm-comment only when the comment
    // UI is enabled (default).
    if (isMarkdown && options.insertBar) {
      extensions.push(insertBarExtension({
        ...options.insertBar,
        destinationFor: options.destinationFor,
        onComment: options.comments !== false ? insertCommentAndEdit : undefined,
      }));
      // Same gate as the insert bar: the composer's body editor sets no
      // options.insertBar and has no band/path-bar/file-tree to fade.
      extensions.push(quietChromeExtension());
    }

    // Link validation (@codemirror/lint) + Cmd+Click nav are wired INSIDE the
    // markdown block above — both pull from the one shared reference cache
    // (cm-reference-resolver), so there is no separate link-resolution RPC path.

    // Wikilink + section heading autocomplete. This installs its own
    // autocompletion() override; minimalSetup does NOT include autocomplete,
    // so there is no competing instance.
    if (isMarkdown && options.wikilinkComplete) {
      const wl = options.wikilinkComplete;
      extensions.push(wikilinkCompleteExtension({
        ...wl,
        // Metadata for the preview card CM6 renders beside the HIGHLIGHTED
        // asset row. Resolved by the candidate's insert value against the live
        // getFromFile() — the same command and the same fromFile the reference
        // resolver uses — so the card shows exactly the file the accepted
        // reference will render, fuzzy fallbacks included. Wired here rather
        // than in editor-main because getFromFile already lives here.
        resolveAssetPreview: wl.resolveAssetPreview ?? (async (insert) => {
          const r = await engine.editorResolveAsset(insert, options.getFromFile?.() ?? '');
          return r.status === 'ok' ? r.data : null;
        }),
      }));
    }

    // The file-picker escape hatch on that same completion tooltip.
    if (isMarkdown && options.assetBrowse) {
      extensions.push(assetBrowseExtension(options.assetBrowse));
    }

    // Caller-supplied extensions go last so they can augment or override any
    // built-in facet (e.g. email-guard rules added by the composer).
    if (options.extraExtensions?.length) {
      extensions.push(...options.extraExtensions);
    }

    const state = options.restore
      ? EditorState.fromJSON(options.restore, { extensions }, { history: historyField })
      : EditorState.create({ doc: options.content, extensions });

    this.view = new EditorView({ state, parent: options.container });
    this.onBeforeDestroy = options.onBeforeDestroy;
    // Background-click caret placement (cm-background-click.ts): clicks in the
    // pane that miss the contentDOM still place the cursor. A plain DOM
    // listener, not EditorView.domEventHandlers — CM6 registers those on the
    // contentDOM, which by definition never receives a dead-zone click.
    this.uninstallBackgroundClick = installBackgroundClickCaret(
      this.view,
      options.container,
    );
  }

  /** Expose the underlying EditorView (e.g. for scroll-sync). */
  getView(): EditorView {
    return this.view;
  }

  getContent(): string {
    return this.view.state.doc.toString();
  }

  /** This file's text, selection and undo history, for `restore` when it is opened again. */
  toJSON(): unknown {
    return this.view.state.toJSON({ history: historyField });
  }

  /**
   * Seal/unseal the editor. Read-only is a live Compartment
   * reconfigure (no remount) — preserves cursor, scroll, and undo history.
   *
   * A `.cm-seal-scrim` overlay is the SOLE sealed signal (R10). The old
   * `opacity:0.55` dim on `.cm-editor.sealed` is removed; the scrim provides
   * the visual indicator + sole pointer-event barrier.
   *
   * Asymmetric fade (R11): `is-sealing` on the scrim drives 150ms ease-out
   * fade-IN; removing it drives 200ms ease-out fade-OUT. A single CSS
   * `transition` cannot do both directions, so we toggle the class here.
   */
  setReadOnly(readOnly: boolean, reason: SealReason = 'publish'): void {
    this.view.dispatch({
      effects: this.editableCompartment.reconfigure([
        EditorView.editable.of(!readOnly),
        EditorState.readOnly.of(readOnly),
      ]),
    });
    this.view.dom.classList.toggle('sealed', readOnly);
    this._updateScrim(readOnly, reason);
  }

  private _updateScrim(seal: boolean, reason: SealReason): void {
    const dom = this.view.dom;
    let scrim = dom.querySelector<HTMLElement>('.cm-seal-scrim');

    if (seal) {
      if (!scrim) {
        scrim = document.createElement('div');
        scrim.className = 'cm-seal-scrim';
        // Indeterminate transit line at the pane's top edge (R13). Decorative —
        // the caption below is the accessible status text.
        const transit = document.createElement('div');
        transit.className = 'cm-seal-scrim__transit';
        transit.setAttribute('aria-hidden', 'true');
        scrim.appendChild(transit);
        const status = document.createElement('div');
        status.className = 'cm-seal-scrim__status';
        // Announced, not just drawn — same idiom as the inline-status badges.
        status.setAttribute('role', 'status');
        status.setAttribute('aria-live', 'polite');
        scrim.appendChild(status);
        dom.appendChild(scrim);
      }
      // The caption names WHY the buffer is frozen. A rename or move commit
      // used to show "Publishing your site…" here — in a document window,
      // where no site exists at all.
      const caption = scrim.querySelector<HTMLElement>('.cm-seal-scrim__status');
      if (caption) {
        caption.dataset.i18n = SEAL_CAPTION[reason];
        applyStaticStrings(scrim);
      }
      // Restore the click barrier (defers to the stylesheet's `pointer-events`).
      // This reverts the unseal override below so a SECOND publish seals again.
      scrim.style.pointerEvents = '';
      // Asymmetric fade-IN (R11): add .is-sealing to switch to 150ms ease-out.
      // The class is applied synchronously; in real browsers a single-frame delay
      // is not needed because the scrim starts at opacity:0 in CSS and the class
      // transitions it to opacity:1.  The immediate-add approach is also testable
      // (jsdom does not run rAF automatically between microtasks).
      scrim.classList.add('is-sealing');
    } else if (scrim) {
      // Asymmetric fade-OUT: remove .is-sealing to switch to 200ms ease-out.
      scrim.classList.remove('is-sealing');
      // Drop the pointer barrier IMMEDIATELY on unseal. The scrim div is kept in
      // the DOM so the opacity fade-out can play, but it must NOT keep swallowing
      // clicks — otherwise an invisible (opacity:0) overlay leaves the editor
      // permanently un-clickable after the first publish (the v0.7.9 "editor
      // cannot be edited / can't place cursor" regression). Belt-and-suspenders
      // with the `.cm-seal-scrim` stylesheet rule, which only blocks while
      // `.is-sealing` is present.
      scrim.style.pointerEvents = 'none';
    }
  }

  /**
   * Apply a change another writer made (the file changed on disk) as an
   * EXTERNAL update: not in the undo history, and skipped by onChange, so
   * autosave does not echo it back. The selection maps through the change,
   * so a caret outside the changed text stays on the same text, and undo
   * events outside it survive. Touching only the changed spans matters: a
   * whole-document replace remaps every undo event to an empty change, which
   * CM6's history then prunes. Restores DOM focus if the editor had it.
   */
  applyExternal(changes: ChangeSet): void {
    if (changes.empty) return;
    const view = this.view;
    const wasFocused = view.hasFocus;
    const wash = outsideWashOf(changes, changes.apply(view.state.doc));
    view.dispatch({
      changes,
      effects: wash ? [wash] : [],
      annotations: [Transaction.addToHistory.of(false), externalUpdate.of(true)],
    });
    if (wash) clearOutsideWashLater(view);
    if (wasFocused) view.focus();
  }

  /**
   * Apply an edit moss makes on the person's behalf, at their request (Undo
   * of a kept outside edit): an ordinary edit, in the undo history and saved
   * like typing, with the changed lines washed so it shows where it landed.
   */
  applyOwnEdit(changes: ChangeSet): void {
    if (changes.empty) return;
    const view = this.view;
    const wash = outsideWashOf(changes, changes.apply(view.state.doc));
    view.dispatch({ changes, effects: wash ? [wash] : [], userEvent: 'input' });
    if (wash) clearOutsideWashLater(view);
  }

  destroy(): void {
    this.uninstallBackgroundClick();
    this.onBeforeDestroy?.();
    this.view.destroy();
  }
}
