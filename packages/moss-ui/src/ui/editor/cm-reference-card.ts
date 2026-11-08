// cm-reference-card.ts — the reference card: the ONE place any reference
// (link, wikilink, image/media embed, footnote, external URL,
// unresolved/moved/ambiguous) shows its details. Hover over a link's URL, a
// wikilink's page or an embed's path (its destination only, see `hitSiteAt`) opens it read-only after
// a short dwell; ⌘K opens it editable, with fields and actions.

import { EditorView, ViewPlugin, keymap, showTooltip, type Tooltip } from '@codemirror/view';
import { EditorState, Extension, Facet, StateEffect, StateField } from '@codemirror/state';
import { extractImageTargets, footnoteIndex, folderParamsFromEmbed, type ReferenceCacheRead } from '@symbiosis-lab/moss-syntax/cm6';
import { syntaxTree } from '@codemirror/language';
import { charAtPointer, hitSiteAt, navigateToReference, referenceSiteAt } from './cm-link-nav';
import { linkTextRange, type LinkTextNode } from './cm-markdown-style';
import { blockEmbedOnLine, embedAt } from './cm-block-embeds';
import { buildHoverDom, type AssetReferenceEnvelope } from './asset-preview';
import type { ApplyEmbedWidthFn } from './embed-width';
import { pickAssetRef } from './asset-picker/browse-button';
import { t } from "../i18n/index";
import { armOutsideDismiss } from "../shared/ui/dismiss-outside";
import { isMacPlatform } from "../utils/kbd-label";

/** Structural subset of the (text, is_embed) cache every synchronous
 *  provider reads — matching `AssetHoverOptions.resolvedCache`'s shape.
 *  `anchor` is optional: every real `EditorReferenceResolution` carries it,
 *  but only `pageLinkCardProvider` needs it (to decide the heading-vs-
 *  description branch without a second RPC) — the other providers' fakes
 *  simply omit it. One envelope shape for all four providers, rather than a
 *  second near-identical cache type that silently drifts from this one. */
type ResolvedCache = {
  get(text: string, isEmbed: boolean): (AssetReferenceEnvelope & { anchor?: string | null }) | undefined;
};

// ── Card model ───────────────────────────────────────────────────────────

export interface CardAction { label: string; run: (view: EditorView) => void; }
export interface CardField { label: string; value: string; commit: (view: EditorView, value: string) => void; }
export interface CardSpec { title: string; body: HTMLElement; hint?: string; actions?: CardAction[]; fields?: CardField[]; }
export type CardProvider = (state: EditorState, pos: number) => CardSpec | null;
export const cardProviders = Facet.define<CardProvider>();
/** Runs after the providers, in facet order, on whatever they produced (or
 *  null): adds to a card rather than competing to be it. */
export type CardAugmenter = (spec: CardSpec | null, state: EditorState, pos: number) => CardSpec | null;
export const cardAugmenters = Facet.define<CardAugmenter>();

interface OpenCard {
  pos: number;
  from: number;
  to: number;
  spec: CardSpec;
  editable: boolean;
}

const setCard = StateEffect.define<OpenCard | null>();

/** The open card, if any. Exported unconditionally as a test seam — matching
 *  `slashMenuField`'s own precedent (cm-slash-menu.ts) of exporting an
 *  internal field "for unit tests" rather than reaching in through a second,
 *  test-only door. */
export const cardField = StateField.define<OpenCard | null>({
  create: () => null,
  update(card, tr) {
    for (const e of tr.effects) if (e.is(setCard)) card = e.value;
    // Typing closes any open card — the spec's "closes on ... typing".
    if (card && tr.docChanged) card = null;
    return card;
  },
  provide: (f) => showTooltip.from(f, (card): Tooltip | null => card && {
    pos: card.pos,
    above: true,
    create: (view) => {
      const dom = buildCardDom(card, view);
      let disarm: (() => void) | null = null;
      return {
        dom,
        // A hover card closes when the pointer leaves; a click-opened one has no
        // such exit, so it needs the shared outside-click dismissal.
        mount: () => {
          if (card.editable) disarm = armOutsideDismiss({ inside: [dom], covers: [dom], onDismiss: () => closeCard(view) });
        },
        destroy: () => disarm?.(),
      };
    },
  }),
});

/** Whether the pointer currently sits over the open card's own DOM, keyed by
 *  view. Read by `dwellPlugin`'s leave check: a read-only card must survive
 *  the pointer crossing the gap between the construct and the card above it
 *  (the `above: true` tooltip placement), and closes only once the pointer
 *  has left BOTH areas. */
const pointerOverCard = new WeakMap<EditorView, boolean>();

function buildCardDom(card: OpenCard, view: EditorView): HTMLElement {
  const dom = document.createElement('div');
  dom.className = 'cm-reference-card' + (card.editable ? ' cm-reference-card--editable' : '');
  const title = dom.appendChild(document.createElement('div'));
  title.className = 'cm-reference-card-title';
  title.textContent = card.spec.title;
  dom.appendChild(card.spec.body);
  // "A hover card is read-only; a click-opened card may host controls"
  // (spec, Architecture principles) is a RENDERING rule, not a
  // provider-content rule — a provider is free to always return
  // `fields`/`actions`, and it is `card.editable` alone that decides whether
  // this DOM shows them.
  if (card.editable) {
    for (const field of card.spec.fields ?? []) {
      const row = dom.appendChild(document.createElement('label'));
      row.className = 'cm-reference-card-field';
      row.textContent = field.label;
      const input = row.appendChild(document.createElement('input'));
      input.className = 'pop-input';
      input.setAttribute('value', field.value);
      input.addEventListener('keydown', (e) => {
        if (e.key === 'Enter') { e.preventDefault(); field.commit(view, input.value); closeCard(view); }
        if (e.key === 'Escape') { e.preventDefault(); closeCard(view); }
      });
    }
    if (card.spec.actions?.length) {
      const row = dom.appendChild(document.createElement('div'));
      row.className = 'cm-reference-card-actions';
      for (const action of card.spec.actions) {
        const btn = row.appendChild(document.createElement('button'));
        btn.type = 'button';
        btn.className = 'moss-btn moss-btn--small';
        btn.textContent = action.label;
        btn.addEventListener('click', () => action.run(view));
      }
    }
  } else {
    const hint = dom.appendChild(document.createElement('div'));
    hint.className = 'cm-reference-card-hint';
    hint.textContent = card.spec.hint ?? t('editmisc.card_open_hint', { mod: isMacPlatform() ? '⌘' : 'Ctrl' });
  }
  dom.addEventListener('mouseenter', () => pointerOverCard.set(view, true));
  dom.addEventListener('mouseleave', () => pointerOverCard.set(view, false));
  return dom;
}

export function closeCard(view: EditorView): void {
  if (view.state.field(cardField, false)) view.dispatch({ effects: setCard.of(null) });
}

/** A click-opened card that may host `fields`/`actions`, bypassing dwell. */
export function openCardAt(view: EditorView, pos: number): boolean {
  const spec = cardSpecAt(view.state, pos);
  if (!spec) return false;
  view.dispatch({ effects: setCard.of({ pos, from: pos, to: pos, spec, editable: true }) });
  return true;
}

/** Run every registered provider in facet order (first non-null wins), then
 *  every augmenter. Exported for tests — production code reaches it only
 *  through the dwell/click paths below. */
export function cardSpecAt(state: EditorState, pos: number): CardSpec | null {
  let spec: CardSpec | null = null;
  for (const provider of state.facet(cardProviders)) {
    spec = provider(state, pos);
    if (spec) break;
  }
  for (const augment of state.facet(cardAugmenters)) spec = augment(spec, state, pos);
  return spec;
}

const DWELL_MS = 150;

/** A footnote reference `[^1]` at `pos`: its characters stay on screen, faint,
 *  so it is found by position in the syntax tree. */
function footnoteRefAt(state: EditorState, pos: number): { from: number; to: number } | null {
  // Both sides: a caret parked on either edge counts, like `referenceSiteAt`.
  for (const side of [1, -1] as const) {
    for (let node: { name: string; from: number; to: number; parent: unknown } | null = syntaxTree(state).resolveInner(pos, side); node; node = node.parent as typeof node) {
      if (node.name === 'FootnoteRef') return { from: node.from, to: node.to };
    }
  }
  return null;
}

/** `[text](url)` → `text`: drop the brackets and the URL, keep the label. */
export function unlinkAt(view: EditorView, pos: number): boolean {
  const link = referenceSiteAt(view.state, pos);
  if (!link || link.wiki) return false;
  // The URL's parent is the `Link` node; its text is what stays.
  const node = syntaxTree(view.state).resolveInner(link.targetFrom, 1).parent;
  const text = node && linkTextRange(node as unknown as LinkTextNode, view.state.doc);
  if (!text) return false;
  view.dispatch({ changes: { from: link.from, to: link.to, insert: view.state.doc.sliceString(text.from, text.to) } });
  return true;
}

/** What's under `pos`, or null: a link's URL, a wikilink's page, an embed's
 *  path (`hitSiteAt` — never the brackets, the text, the label or the alt text), a footnote
 *  reference, or the rendered block (picture, card, player) drawn below an
 *  embed's source line (`onBlock`: `pos` is then that line's end, and `probe` a
 *  position inside the embed for the providers). */
function referenceAt(state: EditorState, pos: number, onBlock: boolean): { from: number; to: number; probe: number } | null {
  if (onBlock) {
    const b = blockEmbedOnLine(state, pos);
    return b ? { from: b.line.from, to: b.line.to, probe: b.parts.targetFrom } : null;
  }
  const site = hitSiteAt(state, pos);
  if (site) return { from: site.targetFrom, to: site.targetTo, probe: site.targetFrom };
  const note = footnoteRefAt(state, pos);
  return note ? { ...note, probe: pos } : null;
}

function tryOpen(view: EditorView, pos: number, onBlock = false): void {
  const ref = referenceAt(view.state, pos, onBlock);
  if (!ref) return;
  const spec = cardSpecAt(view.state, ref.probe);
  if (!spec) return;
  view.dispatch({ effects: setCard.of({ pos: ref.probe, from: ref.from, to: ref.to, spec, editable: false }) });
}

const dwellPlugin = ViewPlugin.fromClass(class {
  private timer: ReturnType<typeof setTimeout> | null = null;
  constructor(private view: EditorView) {
    view.dom.addEventListener('mousemove', this.onMove);
    view.dom.addEventListener('mouseleave', this.onLeave);
  }
  private onMove = (event: MouseEvent) => {
    // Over a rendered block the pointer's own position is meaningless (it is
    // not text); the block's anchor is the end of the source line above it.
    const row = (event.target as Element | null)?.closest?.('.cm-lp-image-row');
    const onBlock = !!row && this.view.dom.contains(row);
    const pos = onBlock ? this.view.posAtDOM(row!) : charAtPointer(this.view, event.clientX, event.clientY);
    const card = this.view.state.field(cardField, false) ?? null;
    // Persistence: stay open while the pointer is on the reference that opened it.
    if (card && !card.editable && pos !== null && referenceAt(this.view.state, pos, onBlock)?.from === card.from) return;
    if (this.timer) clearTimeout(this.timer);
    // Left the construct: close a read-only card unless the pointer landed on
    // the card itself (it sits ABOVE the construct, so the pointer crosses a
    // gap between the two on its way there).
    if (card && !card.editable && !(pointerOverCard.get(this.view) ?? false)) {
      this.view.dispatch({ effects: setCard.of(null) });
    }
    // Off the text (the margin, a gap): nothing to open.
    if (pos !== null) this.timer = setTimeout(() => tryOpen(this.view, pos, onBlock), DWELL_MS);
  };
  private onLeave = () => {
    if (this.timer) clearTimeout(this.timer);
  };
  destroy() {
    if (this.timer) clearTimeout(this.timer);
    this.view.dom.removeEventListener('mousemove', this.onMove);
    this.view.dom.removeEventListener('mouseleave', this.onLeave);
  }
});

// Only claims Escape while a card is open. Claiming it unconditionally made
// Escape a dead key everywhere this extension is installed, so a host that
// listens for Escape on an ancestor (the email composer's exit) never saw it.
const dismissKeymap = keymap.of([{
  key: 'Escape',
  run: (view) => {
    if (!view.state.field(cardField, false)) return false;
    closeCard(view);
    return true;
  },
}]);

const scrollPlugin = ViewPlugin.fromClass(class {
  private handler = () => closeCard(this.view);
  constructor(private view: EditorView) { view.scrollDOM.addEventListener('scroll', this.handler); }
  destroy() { this.view.scrollDOM.removeEventListener('scroll', this.handler); }
});

export function referenceCardExtension(): Extension {
  return [cardField, dwellPlugin, dismissKeymap, scrollPlugin];
}

// ── Providers ────────────────────────────────────────────────────────────

/** Shared URL field + Open/Unlink, attached identically whether the link
 *  currently resolves, is external, or is broken — editing the URL and
 *  unlinking must work regardless; only the read-only BODY differs by
 *  resolved kind (built separately by each of the three providers: page
 *  link, external, diagnostic). Wrapped around those three providers in
 *  `cm-editor.ts`, not called directly here — the wrap needs the
 *  SAME `resolvedCache` each provider already closes over. */
export function withLinkControls(
  spec: CardSpec,
  state: EditorState,
  pos: number,
  // The FULL cache type (not the local `ResolvedCache` the read-only
  // providers below use): `navigateToReference`'s Open action needs
  // `url`/`resolved_path`, which `ResolvedCache`'s narrower structural
  // subset (asset-preview.ts's `AssetReferenceEnvelope`) does not carry.
  resolvedCache: ReferenceCacheRead,
): CardSpec {
  const link = referenceSiteAt(state, pos);
  if (!link || link.wiki) return spec;
  const { targetFrom: from, targetTo: to, target } = link; // the destination span ONLY — never the brackets
  return {
    ...spec,
    fields: [
      ...(spec.fields ?? []),
      { label: t('chip.url.label'), value: target, commit: (view, value) => {
        view.dispatch({ changes: { from, to, insert: value } });
      } },
    ],
    actions: [
      ...(spec.actions ?? []),
      { label: t('reference.open'), run: () => { navigateToReference(resolvedCache, { text: target, isEmbed: false }); } },
      { label: t('reference.unlink'), run: (view) => { unlinkAt(view, pos); } },
    ],
  };
}

/** Live metadata already owned by the document session, not another backend read. */
export interface CurrentArticleCardData {
  title: string;
  path: string;
  locations: string[];
  revealLocations: () => void;
}

/** Match the current article's map from the canonical embed parts, shared by hover and follow. */
export function isCurrentArticleMapAt(state: EditorState, pos: number): boolean {
  const embed = embedAt(state, pos);
  if (!embed || embed.parts.target !== '#') return false;
  // Each pipe separates modifier groups; the existing keyed-param parser reads
  // comma-separated tokens within them. Markdown images have no pothole.
  const params = folderParamsFromEmbed({ ...embed.parts, pothole: embed.parts.pothole?.replaceAll('|', ',') ?? null });
  return params?.style === 'map';
}

/** The map's source is this article's live metadata, not a missing media file. */
export function currentArticleCardProvider(current: () => CurrentArticleCardData): CardProvider {
  return (state, pos) => {
    const site = referenceSiteAt(state, pos, { embeds: true });
    if (!site?.isEmbed || !isCurrentArticleMapAt(state, pos)) return null;
    const article = current();
    const body = document.createElement('div');
    const explanation = body.appendChild(document.createElement('p'));
    explanation.textContent = t('reference.current_map');
    const locations = body.appendChild(document.createElement('p'));
    locations.textContent = article.locations.length ? article.locations.join(' · ') : t('reference.current_map_empty');
    const path = body.appendChild(document.createElement('p'));
    path.textContent = article.path;
    return {
      title: article.title || t('reference.current_article'),
      body,
      hint: t(article.locations.length ? 'reference.show_locations_hint' : 'reference.show_map_details_hint', { mod: isMacPlatform() ? '⌘' : 'Ctrl' }),
      actions: article.locations.length ? [{ label: t('reference.show_locations'), run: (view) => { closeCard(view); article.revealLocations(); } }] : [],
    };
  };
}

const WIDTH_PRESETS = ['25%', '50%', '75%', '100%'] as const;

/** Read-only: the same asset preview card the old hover popover showed,
 *  reusing `buildHoverDom` — asset-preview.ts is "the ONE asset preview
 *  card". Extended with a path field, Replace…, and width-preset
 *  actions when `opts` is supplied and the position is on an embed
 *  (the hover-card path keeps the plain read-only spec by passing no opts —
 *  matching the spec's "a hover card is read-only; a click-opened card may
 *  host controls"). */
export function assetCardProvider(
  resolvedCache: ResolvedCache,
  opts?: { pagePath: () => string; projectRoot: () => string | null; applyWidth: ApplyEmbedWidthFn },
): CardProvider {
  return (state, pos) => {
    for (const target of extractImageTargets(state)) {
      if (pos < target.from || pos > target.to) continue;
      const env = resolvedCache.get(target.target, true);
      if (env?.kind.kind === 'anchor') return null;
      const body = buildHoverDom(target.target, env);
      const embed = embedAt(state, pos);
      if (!opts || !embed) return { title: target.target, body };
      // `target.from`/`target.to` span the WHOLE embed node for a wikilink
      // embed (`![[old.png|50%]]` — cm-image-extract.ts's own header: "for
      // wikilink embeds the from/to span is the WHOLE node"), not just the
      // filename; a standard `![alt](url)` image's span IS already just the
      // URL. Deviation from the plan's literal `const { from, to } = t`,
      // which would have replaced the WHOLE node — brackets, `|50%` suffix
      // and all — with the bare filename text on every wikilink embed,
      // losing the width and leaving invalid markdown; this locates the
      // filename text inside the node's own span instead, so only it is
      // replaced (both by the field and by Replace…, below).
      const full = state.doc.sliceString(target.from, target.to);
      const offset = full.indexOf(target.target);
      const targetFrom = offset >= 0 ? target.from + offset : target.from;
      const targetTo = offset >= 0 ? targetFrom + target.target.length : target.to;
      // Bound to a local so the non-null narrowing above survives into the
      // async closure below (TS does not carry a parameter's narrowing
      // across a function-expression boundary).
      const pickOpts = opts;
      return {
        title: target.target,
        body,
        fields: [{ label: t('chip.url.label'), value: target.target, commit: (view, value) => {
          view.dispatch({ changes: { from: targetFrom, to: targetTo, insert: value } });
        } }],
        actions: [
          { label: t('reference.replace'), run: async (view) => {
            const picked = await pickAssetRef({
              pagePath: pickOpts.pagePath, projectRoot: pickOpts.projectRoot,
              kinds: () => null, afterImport: undefined,
            });
            if (picked) view.dispatch({ changes: { from: targetFrom, to: targetTo, insert: picked.insert } });
          } },
          ...WIDTH_PRESETS.map((pct) => ({
            label: pct, run: (view: EditorView) => { void pickOpts.applyWidth(view, embed.from, pct); },
          })),
          { label: t('reference.reset_width'), run: (view: EditorView) => { void pickOpts.applyWidth(view, embed.from, null); } },
        ],
      };
    }
    return null;
  };
}

/** Direct children of `node` named `name`, in order — a local copy of
 *  cm-footnote.ts's own private helper of the same name (confirmed NOT
 *  exported: moss-syntax's cm6 dist only exports `footnoteIndex` /
 *  `footnoteJumpTarget` / `footnoteDecorations` / `footnoteExtension` /
 *  `footnoteTheme` for this feature). Two owners of an identical one-line
 *  helper is cheaper than a cross-package export for a single private
 *  utility. */
function childrenNamed(node: { cursor(): { firstChild(): boolean; nextSibling(): boolean; name: string; from: number; to: number } }, name: string): Array<{ from: number; to: number }> {
  const out: Array<{ from: number; to: number }> = [];
  const cur = node.cursor();
  if (cur.firstChild()) {
    do {
      if (cur.name === name) out.push({ from: cur.from, to: cur.to });
    } while (cur.nextSibling());
  }
  return out;
}

/** Read-only: a reference `[^1]` shows the note text; a definition's
 *  `[^1]:` marker shows the sentence that first cites it — the two
 *  directions `footnoteJumpTarget` (cm-link-nav.ts) already navigates. */
export const footnoteCardProvider: CardProvider = (state, pos) => {
  const tree = syntaxTree(state);
  let node = tree.resolveInner(pos, 1);
  while (node && node.name !== 'FootnoteRef' && node.name !== 'FootnoteDefinition') {
    if (!node.parent) { node = null as unknown as typeof node; break; }
    node = node.parent;
  }
  if (!node) return null;
  const { definitions, firstRefs } = footnoteIndex(state);
  const label = childrenNamed(node.node, 'FootnoteLabel')[0];
  if (!label) return null;
  const text = state.doc.sliceString(label.from, label.to);
  const body = document.createElement('div');
  if (node.name === 'FootnoteRef') {
    const def = definitions.get(text);
    body.textContent = def ? state.doc.sliceString(def.markEnd, def.to).trim() : t('editmisc.not_found');
  } else {
    const ref = firstRefs.get(text);
    body.textContent = ref ? state.doc.lineAt(ref.from).text.trim() : t('editmisc.not_found');
  }
  return { title: `[^${text}]`, body };
};

// ── External URL, and any other link ─────────────────────────────────────
// Synchronous, reading straight from the resolved cache with no backend round
// trip — matching the asset provider's target-lookup shape. A link's or an
// image's problem is not a provider: `problemCardAugmenter` (cm-problems.ts)
// adds it to whichever card claims the position.

export function externalCardProvider(resolvedCache: ResolvedCache): CardProvider {
  return (state, pos) => {
    const link = referenceSiteAt(state, pos);
    if (!link) return null;
    const env = resolvedCache.get(link.target, false);
    if (env?.kind.kind !== 'external') return null;
    const body = document.createElement('div');
    body.textContent = (env as unknown as { kind: { data: { url: string } } }).kind.data.url;
    return { title: t('editmisc.external_link'), body };
  };
}

/** The last resort for a link no other provider claims, such as one to a page
 *  that does not exist: a card titled with the address, so the link controls
 *  and any problem's sentence have somewhere to go. */
export const linkCardProvider: CardProvider = (state, pos) => {
  const link = referenceSiteAt(state, pos);
  return link ? { title: link.target, body: document.createElement('div') } : null;
};

// ── Page link (and the same-document anchor case) ────────────────────────
// The one provider that needs data only the backend has (title via heading
// computation, description via the build's excerpt logic) — it depends on
// the `editor_reference_card` command. `CardProvider` is synchronous,
// so this provider returns a `CardSpec` immediately with a body that starts
// empty and fills in once the async call resolves — the same lazily-filled-
// panel shape `asset-preview.ts`'s `assetCompletionInfo` already uses.
//
// The provider is wrapped in `withLinkControls` like the other two link-kind
// providers, and cm-editor.ts registers it, passing `engine.editorReferenceCard` as `call`.
// `call` stays a parameter rather than an inline import of `engine` here: it
// keeps the provider testable without mocking the engine.

export interface ReferenceCardInfo {
  title: string;
  description: string | null;
  source_path: string | null;
  generated: boolean;
  heading: string | null;
}

type EditorReferenceCardFn = (fromFile: string, target: string) => Promise<{ status: string; data?: ReferenceCardInfo }>;

export function pageLinkCardProvider(
  resolvedCache: ResolvedCache,
  call: EditorReferenceCardFn,
  opts: { fromFile: () => string },
): CardProvider {
  return (state, pos) => {
    const link = referenceSiteAt(state, pos);
    if (!link) return null;
    const env = resolvedCache.get(link.target, false);
    if (!env || !['link', 'moved', 'anchor'].includes(env.kind.kind)) return null;
    const body = document.createElement('div');
    call(opts.fromFile(), link.target).then((res) => {
      if (res.status !== 'ok' || !res.data) return;
      const { description, generated, heading } = res.data;
      // `env.anchor` (already in hand, no RPC needed) decides which half of
      // ReferenceCardInfo answers this card: a `#anchor` link asks the
      // heading question, a bare page link asks the description question —
      // never both, matching the spec's "with #anchor, the matching
      // heading's text OR 'no heading #anchor'" (row 98).
      if (env.anchor) {
        body.textContent = heading ?? t('editmisc.no_heading_for_anchor', { anchor: env.anchor });
      } else {
        body.textContent = description ?? '';
        if (generated) body.append(' · ', t('completion.generated'));
      }
    });
    return { title: link.target, body };
  };
}
