// CM6 extension for Cmd+Click navigation on resolved links.
// Intercepts click events with metaKey (Mac) or ctrlKey (Win/Linux) held,
// finds the link at the click position via the Lezer syntax tree, and dispatches
// an `editor:file-selected` custom event if the target is resolved with a known
// file path.  Also adds a pointer cursor when Cmd is held over resolved links
// (Obsidian-style).

import { Decoration, EditorView, ViewPlugin, keymap, type DecorationSet, type ViewUpdate } from '@codemirror/view';
import { isMacPlatform } from "../utils/kbd-label";
import { Extension, Prec, StateEffect, StateField, type EditorState } from '@codemirror/state';
import { syntaxTree } from '@codemirror/language';
import { embedParts, extractImageTargets, footnoteJumpTarget, isEmbedNode, type ReferenceCacheRead, type TreeNode } from '@symbiosis-lab/moss-syntax/cm6';
import { linkUrlRange } from './cm-markdown-style';
import { revealLines } from './cm-reveal-flash';
import { openExternal } from "../utils/open-external";
import { host as uiHost } from "../host";

// ── Which link or embed is at a position ────────────────────────────────────
// The one answer to that question. The hover card, the link controls, ⌘K,
// ⌘-click and Mod-Enter all ask it, so they cannot disagree about what is a
// reference or where it starts and ends. Every character of a reference is on
// screen, so it is found by position in the syntax tree; there is no
// widget over it to hit-test instead.
//
// Two questions, kept apart. "Which reference is this caret in?" is the whole
// construct (`referenceSiteAt`): ⌘K, Mod-Enter and the card's controls act from
// any character of it, being explicit commands. "Which reference is the POINTER
// on?" is its destination only (`hitSiteAt`): a link's URL, a wikilink's page,
// an embed's path — never the brackets, the visible text, the label or the alt
// text. The link is the destination; the words around it are just words, so
// pointing at them does nothing and only the characters that say where it goes
// open a card or follow.

/** A link (`[text](url)`, `[[Page]]`) or an embed (`![alt](path)`, `![[path]]`) in the document. */
export interface ReferenceSite {
  /** The whole construct. */
  from: number;
  to: number;
  /** What is looked up: a URL, a page name, an asset path. */
  target: string;
  /** The span of `target` alone, brackets excluded: what an edit of the destination replaces, and the ACTIVE part — where a hover card opens and ⌘-click follows, and nothing else. A markdown link's URL, a wikilink's page, an embed's path. */
  targetFrom: number;
  targetTo: number;
  isEmbed: boolean;
  /** True for `[[Page]]`; false for a markdown link or an embed. */
  wiki: boolean;
}

/** The slice of a Lezer node this module walks (@lezer/common is not a direct dependency). */
type SiteNode = TreeNode & {
  parent: SiteNode | null;
  node: { getChild(name: string): { from: number; to: number } | null };
  cursor(): { firstChild(): boolean; nextSibling(): boolean; name: string; from: number; to: number };
};

/**
 * The reference whose characters `pos` is on, over the WHOLE construct. `strict`
 * excludes both ends of the construct: Mod-Enter uses it, because at an end the
 * caret is beside the reference, not on it (Mod-Enter is `insertBlankLine`
 * there). The hover card and the link controls are inclusive: a caret parked on
 * either edge counts. The pointer asks `hitSiteAt` instead.
 *
 * `embeds` also finds an embed; a link that encloses the embed (`[![cover](c.png)](/about/)`)
 * wins, as it does when the rendered picture is clicked.
 */
export function referenceSiteAt(state: EditorState, pos: number, opts: { strict?: boolean; embeds?: boolean } = {}): ReferenceSite | null {
  const inside = opts.strict ? (a: number, b: number) => pos > a && pos < b : (a: number, b: number) => pos >= a && pos <= b;
  const link = linkSiteAt(state, pos, inside);
  const embed = opts.embeds ? embedSiteAt(state, pos, inside) : null;
  return embed && !(link && link.from <= embed.from && link.to >= embed.to) ? embed : link;
}

/**
 * The reference the POINTER is on: the character at `pos` lies in the
 * destination of one, and only there (`pos` is a character's index, as
 * `charAtPointer` gives it — not a boundary between two). `follow` asks what
 * ⌘-click does rather than what hover shows, which differs for a linked
 * picture: hovering its path shows the picture, and following it goes where the
 * page goes, to the enclosing link's URL. Hovering or clicking that outer URL
 * itself acts on the link. The picture's alt text and brackets are inert, as
 * are the link's own text and punctuation around it.
 */
export function hitSiteAt(state: EditorState, pos: number, opts: { follow?: boolean } = {}): ReferenceSite | null {
  const within = (a: number, b: number) => pos >= a && pos < b;
  const link = linkSiteAt(state, pos, within);
  const embed = embedSiteAt(state, pos, within);
  const hit = embed ?? link;
  if (!hit || !within(hit.targetFrom, hit.targetTo)) return null;
  return embed && opts.follow && link ? link : hit;
}

/**
 * The character under the pointer, or null where none is. `posAtCoords` always
 * answers the nearest BOUNDARY, even from the hung margin, the gap between two
 * lines or beside a wrapped line's end, so it cannot say "off the text"; the
 * pointer must lie inside the box the browser drew for one of the two
 * characters beside that boundary. Boxes come from `Range.getClientRects`, so
 * a wrapped or right-to-left character is judged by where it is actually drawn.
 * With no layout to ask (jsdom) the boundary stands for the character after it.
 */
export function charAtPointer(view: EditorView, x: number, y: number): number | null {
  const pos = view.posAtCoords({ x, y });
  if (pos === null) return null;
  try {
    if (!view.coordsAtPos(pos, 1)) return pos;
  } catch { return pos; }
  for (const c of [pos, pos - 1]) {
    if (c < 0 || c >= view.state.doc.length) continue;
    for (const r of charRects(view, c)) if (x >= r.left && x < r.right && y >= r.top && y < r.bottom) return c;
  }
  return null;
}

/** The boxes the browser drew for the character at `c`, none when it is not text on screen (a widget, a line break). */
function charRects(view: EditorView, c: number): DOMRect[] {
  const { node, offset } = view.domAtPos(c, 1);
  let text: Node | null = node.nodeType === 3 ? node : node.childNodes[offset] ?? null;
  // Boundaries between spans can answer an element, or the end of the previous text node: take the next text.
  const walker = document.createTreeWalker(view.contentDOM, NodeFilter.SHOW_TEXT);
  let at = node.nodeType === 3 ? offset : 0;
  if (text) walker.currentNode = text;
  if (text?.nodeType !== 3 && text) text = walker.nextNode();
  while (text && at >= (text as Text).length) { text = walker.nextNode(); at = 0; }
  if (!text) return [];
  const range = document.createRange();
  range.setStart(text, at);
  range.setEnd(text, at + 1);
  return [...range.getClientRects()];
}

function linkSiteAt(state: EditorState, pos: number, inside: (a: number, b: number) => boolean): ReferenceSite | null {
  // The outermost link or wikilink over `pos`, found from the position: no walk of the whole document.
  let found: SiteNode | null = null;
  for (const side of [1, -1] as const) {
    for (let n = syntaxTree(state).resolveInner(pos, side) as unknown as SiteNode | null; n; n = n.parent) {
      if ((n.name === 'Link' || n.name === 'Wikilink') && inside(n.from, n.to) && (!found || n.from <= found.from)) found = n;
    }
    if (found) break;
  }
  return found ? linkSite(state, found) : null;
}

function embedSiteAt(state: EditorState, pos: number, inside: (a: number, b: number) => boolean): ReferenceSite | null {
  let node = syntaxTree(state).resolveInner(pos, 1) as unknown as SiteNode | null;
  while (node && !isEmbedNode(node.name) && node.name !== 'ShortcodeOpenLine') node = node.parent;
  if (!node || !inside(node.from, node.to)) return null;
  if (isEmbedNode(node.name)) return embedSite(state, node);
  // Attribute destinations have no Image node. Use the catalog-derived spans
  // already read by batch resolution and asset cards, including legacy paths.
  const attr = extractImageTargets(state).find(t => t.attr && inside(t.from, t.to));
  return attr ? {
    from: attr.from, to: attr.to, target: attr.target,
    targetFrom: attr.from, targetTo: attr.to, isEmbed: true, wiki: false,
  } : null;
}

/** An embed's site: its path, trimmed, is both the target and the destination — `embedParts` spans a wikilink embed's whole node, brackets and `|width` included. */
function embedSite(state: EditorState, node: SiteNode): ReferenceSite | null {
  const parts = embedParts(node, state.doc);
  if (!parts) return null;
  let from = parts.targetFrom, to = parts.targetTo;
  const path = node.name === 'WikilinkEmbed' ? node.node.getChild('WikilinkTarget') : null;
  if (path) {
    const raw = state.doc.sliceString(path.from, path.to);
    from = path.from + (raw.length - raw.trimStart().length);
    to = from + parts.target.length;
  }
  return { from: node.from, to: node.to, target: parts.target, targetFrom: from, targetTo: to, isEmbed: true, wiki: false };
}

/** A link's or wikilink's site, from its `Link` / `Wikilink` node: the one resolver that hover, follow and the inert marks all read. */
function linkSite(state: EditorState, node: SiteNode): ReferenceSite | null {
  const base = { from: node.from, to: node.to, isEmbed: false };
  if (node.name === 'Link') {
    const url = linkUrlRange(node);
    if (!url) return null;
    return { ...base, target: state.doc.sliceString(url.from, url.to), targetFrom: url.from, targetTo: url.to, wiki: false };
  }
  // `[[Page]]` or `[[Page|Label]]`: the page is the destination, the label inert.
  const page = node.node.getChild('WikilinkTarget');
  if (!page) return null;
  const raw = state.doc.sliceString(page.from, page.to);
  const target = raw.trim();
  const from = page.from + (raw.length - raw.trimStart().length);
  return { ...base, target, targetFrom: from, targetTo: from + target.length, wiki: true };
}

/**
 * Every part of a reference that is NOT its destination, as marks. The editor
 * paints a hand over a link's whole node while ⌘ is held (moss-syntax's
 * `cm-link-clickable`); these marks take it back over the punctuation, text,
 * label and alt text, so the pointer promises exactly what `hitSiteAt`
 * delivers. A picture inside a link keeps its own path as a hole in the link's
 * inert span: that path is active too.
 */
const MARK_INERT = Decoration.mark({ class: 'cm-ref-inert' });
const MARK_DESTINATION = Decoration.mark({ class: 'cm-link-clickable' });
function destinationMarks(view: EditorView): DecorationSet {
  const { state } = view;
  const sites: ReferenceSite[] = [];
  const seen = new Set<string>(); // a node spanning two visible ranges is met twice
  for (const { from, to } of view.visibleRanges) {
    syntaxTree(state).iterate({ from, to, enter(node) {
      const isLink = node.name === 'Link' || node.name === 'Wikilink';
      if (!isLink && !isEmbedNode(node.name)) return;
      const key = `${node.name}:${node.from}`;
      if (seen.has(key)) return false;
      seen.add(key);
      const n = node.node as unknown as SiteNode;
      const site = isLink ? linkSite(state, n) : embedSite(state, n);
      if (site) sites.push(site);
      if (node.name === 'Wikilink') return false;
    } });
  }
  // A picture inside a link marks its own inert parts and is active at its own path: cut it out of the link's span.
  const holes = sites.filter((s) => s.isEmbed);
  const ranges: ReturnType<typeof MARK_INERT.range>[] = [];
  for (const site of sites) {
    let spans: [number, number][] = [[site.from, site.targetFrom], [site.targetTo, site.to]];
    for (const h of holes) spans = spans.flatMap(([a, b]): [number, number][] => h.to <= a || h.from >= b || h === site ? [[a, b]] : [[a, h.from], [h.to, b]]);
    for (const [a, b] of spans) if (b > a) ranges.push(MARK_INERT.range(a, b));
  }
  for (const target of extractImageTargets(state)) {
    if (target.attr) ranges.push(MARK_DESTINATION.range(target.from, target.to));
  }
  return Decoration.set(ranges, true);
}
const inertPlugin = ViewPlugin.fromClass(class {
  decorations: DecorationSet;
  constructor(view: EditorView) { this.decorations = destinationMarks(view); }
  update(u: ViewUpdate) { if (u.docChanged || u.viewportChanged || syntaxTree(u.state) !== syntaxTree(u.startState)) this.decorations = destinationMarks(u.view); }
}, { decorations: (v) => v.decorations });

/**
 * A reference the user can follow: the text to look up, and which half of the
 * (text, is_embed) cache holds its envelope.
 */
export interface FollowTarget {
  text: string;
  isEmbed: boolean;
  /** Range of the whole construct, used to hit-test a position. */
  from: number;
  to: number;
}

/**
 * The reference at `pos`, or null. ONE definition, used by all three gestures
 * (Cmd/Ctrl+click, Mod-Enter, the card's context menu) so "what is followable
 * here" cannot answer differently depending on how you asked.
 *
 * Embeds are included, which is the point: `extractLinkTargets` deliberately
 * skips `WikilinkEmbed` because a lint underline belongs on link
 * text, not on a rendered card — but an embed is exactly as followable as a
 * link, and until now nothing could follow one.
 *
 * The hit test uses the ENCLOSING construct, not the target span, and both
 * ends are excluded. Mod-Enter is bound in `defaultKeymap` (which
 * `minimalSetup` includes) to `insertBlankLine`, and this binding takes
 * precedence — so an inclusive test would eat that command for a caret at the
 * very start of a line beginning with a link, or parked just past a closing
 * `]]`. At those positions the caret is beside the reference, not on it, and
 * Enter should still be Enter. Widening either end is not harmless for the
 * click either: `posAtCoords` answers `line.to` for a click anywhere in a block
 * widget or the blank space right of a line, so an inclusive end would make
 * Cmd+clicking an embed card navigate to whatever link that line ends with.
 * A linked embed (`[![cover](cover.png)](/about/)`) is both at once, and the
 * enclosing link wins, as it does when the rendered widget is clicked.
 */
export function followTargetAt(state: EditorState, pos: number, opts: { pointer?: boolean } = {}): FollowTarget | null {
  // From the caret (Mod-Enter) any character of the construct follows, being an
  // explicit command; under the pointer only the destination does (`hitSiteAt`).
  const site = opts.pointer ? hitSiteAt(state, pos, { follow: true }) : referenceSiteAt(state, pos, { strict: true, embeds: true });
  return site ? { text: site.target, isEmbed: site.isEmbed, from: site.from, to: site.to } : null;
}

/**
 * Does this click carry the FOLLOW chord on this platform?
 *
 * Cmd on macOS, Ctrl elsewhere — deliberately not "either, everywhere". On
 * macOS Ctrl+click IS the secondary click: WebKit fires `contextmenu` and
 * `click` for one gesture, so honouring Ctrl there would open a context menu
 * and then navigate out from under it before it could be read.
 *
 * Exported because THREE things must agree about it — this module's click
 * handler, the `cm-meta-held` class that paints the pointer, and the embed
 * card's own click handler (cm-media-embed.ts). A cursor that promises a follow
 * the click then refuses is the exact defect this branch started from.
 */
export function isFollowChord(e: { metaKey: boolean; ctrlKey: boolean }): boolean {
  return isMacPlatform() ? e.metaKey : e.ctrlKey || e.metaKey;
}

/** Does pressing this key arm the follow chord on this platform?
 *
 *  Asked OF `isFollowChord` rather than restated beside it — holding the key
 *  alone is the click that key would make. A key that navigates with no
 *  pointer, and a pointer that then refuses to navigate, are the same defect
 *  pointing in opposite directions, and two copies of the rule are how you
 *  get one. */
function isChordKey(key: string): boolean {
  return isFollowChord({ metaKey: key === 'Meta', ctrlKey: key === 'Control' });
}

/** Whether the follow chord is currently held — shared by `linkNavExtension`
 *  (the pointer cursor) and `cm-reference-card.ts` (chord+hover arming), so
 *  there is exactly one keydown/keyup pair toggling `cm-meta-held` rather
 *  than two independently-drifting copies. */
export const chordHeldEffect = StateEffect.define<boolean>();
export const chordHeldField = StateField.define<boolean>({
  create: () => false,
  update(value, tr) {
    for (const e of tr.effects) if (e.is(chordHeldEffect)) value = e.value;
    return value;
  },
});

/** Toggle `cm-meta-held` and `chordHeldField` together. Installed ONCE
 *  (cm-editor.ts) — `linkNavExtension` no longer carries its own copy. */
export function chordTrackingExtension(): Extension {
  const setHeld = (view: EditorView, held: boolean) => {
    view.dom.classList.toggle('cm-meta-held', held);
    view.dispatch({ effects: chordHeldEffect.of(held) });
  };
  return [
    chordHeldField,
    EditorView.domEventHandlers({
      keydown(event, view) { if (isChordKey(event.key)) setHeld(view, true); return false; },
      keyup(event, view) { if (isChordKey(event.key)) setHeld(view, false); return false; },
    }),
    ViewPlugin.fromClass(class {
      private handler: () => void;
      constructor(private view: EditorView) {
        this.handler = () => setHeld(view, false);
        window.addEventListener('blur', this.handler);
      }
      update() {}
      destroy() { window.removeEventListener('blur', this.handler); }
    }),
  ];
}

/**
 * The single "follow a reference" rule: look the target up in the shared
 * reference cache and, when its envelope names a source file, announce the
 * navigation on `document`.
 *
 * Two fields can name that file, and both are absolute source FILES:
 *   - `resolved_path` — a resolved internal link, or a folder embed (whose
 *     value is the folder's INDEX SOURCE, never the directory);
 *   - `resolved.absolute_path` — an asset embed (pdf/video/audio/image/…),
 *     which `editor:file-selected` opens in the file viewer rather than CM6.
 *
 * Everything else (external URLs, anchors, unresolved, ambiguous, a page with
 * no source such as a synthesized index) names no file, so this no-ops and
 * returns false. `followTarget` below is the full rule — it adds the
 * source-less pages that the PREVIEW can show — and is what decides whether a
 * follow affordance appears at all.
 *
 * The event goes on `document` because the `editor:file-selected` listener
 * (editor-main.ts) lives there, not on a CM6 ancestor.
 *
 * Split out so an embed card, which can only open a FILE, asks the same
 * question the navigation answers. Returns the absolute source file, or null.
 */
export function followFilePath(
  env: { resolved_path: string | null; resolved: { absolute_path: string } | null } | undefined,
): string | null {
  return env?.resolved_path ?? env?.resolved?.absolute_path ?? null;
}

/**
 * Where following a reference lands: a source FILE to open in the editor, a
 * site-relative URL to show in the main preview, or an external URL to open
 * through the platform opener. The second outcome exists for pages the build
 * synthesizes — an index-less folder's listing, a generated author/tag page —
 * which resolve to a deployed URL with no source file. The same rule the
 * author/tag chips follow (chip-term-nav.ts). Anything else (anchor,
 * not-found, ambiguous) returns null, and that null is also the affordance
 * answer: no hand, no Open, for a reference that goes nowhere.
 */
export function followTarget(
  env: {
    kind: { kind: string; data?: unknown };
    url: string | null;
    resolved_path: string | null;
    resolved: { absolute_path: string } | null;
  } | undefined,
): { file: string } | { url: string } | { external: string } | null {
  const filePath = followFilePath(env);
  if (filePath) return { file: filePath };
  if (env && (env.kind.kind === 'link' || env.kind.kind === 'moved') && env.url?.startsWith('/')) {
    return { url: env.url };
  }
  const external = env?.kind.kind === 'external' ? (env.kind.data as { url?: string } | undefined)?.url : undefined;
  if (external) return { external };
  return null;
}

export function navigateToReference(
  resolvedCache: ReferenceCacheRead,
  target: FollowTarget | { text: string; isEmbed: boolean },
): boolean {
  const dest = followTarget(resolvedCache.get(target.text, target.isEmbed));
  if (!dest) return false;
  if ('file' in dest) {
    document.dispatchEvent(new CustomEvent('editor:file-selected', {
      detail: { filePath: dest.file },
    }));
  } else if ('url' in dest) {
    uiHost.bus.emit('editor:navigate-preview', { urlPath: dest.url });
  } else {
    void openExternal(dest.external);
  }
  return true;
}

/**
 * Follow a LINK target by text. The pre-embed form of `navigateToReference`,
 * kept because widgets that own their own click surface (the linked-embed image
 * block in cm-live-preview.ts) hold an href, not a document position.
 *
 * @returns true when a navigation was dispatched.
 */
export function navigateToLinkTarget(
  resolvedCache: ReferenceCacheRead,
  target: string,
): boolean {
  return navigateToReference(resolvedCache, { text: target, isEmbed: false });
}

/**
 * Follow the footnote at `pos`, if there is one. Returns whether it jumped.
 *
 * A footnote is a reference, so it answers the same two gestures as every
 * other reference — but it navigates WITHIN the document, so it moves the
 * caret instead of dispatching `editor:file-selected`. Both directions:
 * a marker goes to its definition, a definition's marker goes back to its
 * first reference, mirroring the `↩` back-link the built page gives a reader.
 *
 * The destination line is washed with the same reveal flash a preview click
 * uses, for the same reason: a jump that only moves the caret leaves the
 * reader hunting for what moved. Unlike that gesture this one DOES take the
 * caret — it is the full jump, not the orienting half.
 */
function followFootnote(view: EditorView, pos: number): boolean {
  const target = footnoteJumpTarget(view.state, pos);
  if (!target) return false;
  const line = view.state.doc.lineAt(target.labelFrom).number;
  view.dispatch({ selection: { anchor: target.labelFrom }, scrollIntoView: true });
  // `preserveScroll` because the dispatch above already scrolled — this call is
  // here for the wash and its timer bookkeeping, not for the scroll.
  revealLines(view, line, line, { preserveScroll: true });
  view.focus();
  return true;
}

/** "Act on the thing at the caret" — a footnote jump, or the generic
 *  follow rule (links, wikilinks, embeds): Mod-Enter's one "what does Open
 *  mean here" answer. */
export function followAtCaret(view: EditorView, pos: number, resolvedCache: ReferenceCacheRead, follow?: (target: FollowTarget) => boolean): boolean {
  if (followFootnote(view, pos)) return true;
  const ref = followTargetAt(view.state, pos);
  return ref ? (follow?.(ref) || navigateToReference(resolvedCache, ref)) : false;
}

/**
 * Create a CM6 extension that handles Cmd+Click (or Ctrl+Click) on links,
 * and shows a pointer cursor when Cmd/Ctrl is held over resolved links.
 *
 * Reads the SHARED reference cache (cm-reference-resolver) keyed (text,
 * is_embed:false): a link is navigable when its envelope carries a
 * `resolved_path` (the absolute source file the command enriched for resolved
 * internal links). External/anchor/mismatch/broken links carry no
 * `resolved_path`, so a Cmd+Click on them no-ops.
 *
 * @param resolvedCache Shared reference cache (read-only lookup).
 */
export function linkNavExtension(
  resolvedCache: ReferenceCacheRead,
  follow?: (target: FollowTarget) => boolean,
): Extension {
  const domHandlers = EditorView.domEventHandlers({
    // MOUSEDOWN, not click: a follow must not move the caret. `click` fires
    // after the browser's own mousedown-driven caret placement, so by the time
    // a click handler ran the caret would already have moved into the
    // construct being followed — a visible flash on the way out, and (for a
    // multi-line construct) a caret left in a different place than a
    // cancelled follow would have. Handling `mousedown` and calling
    // `preventDefault()` intercepts before either happens.
    mousedown(event: MouseEvent, view: EditorView): boolean {
      // Primary button only. Secondary and middle presses have their own
      // meanings (the context menu, paste-on-X11), and neither is a follow.
      if (event.button !== 0) return false;

      // Cmd+Click on macOS, Ctrl+Click elsewhere — see `isFollowChord`.
      if (!isFollowChord(event)) return false;

      // The character under the press: only a reference's destination follows
      const pos = charAtPointer(view, event.clientX, event.clientY);
      if (pos === null) return false;

      // Footnotes first: they are the one reference kind that resolves inside
      // the document, so they never reach the cache `followTargetAt` reads.
      if (followFootnote(view, pos)) {
        event.preventDefault();
        return true;
      }

      // ONE follow rule — links AND embeds, the same one Mod-Enter and the
      // embed card's context menu use.
      const ref = followTargetAt(view.state, pos, { pointer: true });
      if (!ref) return false;

      if (!(follow?.(ref) || navigateToReference(resolvedCache, ref))) return false;

      // Prevent default behavior (e.g., link following, selection)
      event.preventDefault();
      return true;
    },
  });

  // ── Mod-Enter: follow the reference at the cursor ──────────────────────
  // The visible-control rule (design-vocabulary.md) says a chord is never the
  // sole path to an action, and Cmd+click is a chord — mouse-only, and useless
  // to anyone driving the editor from the keyboard. This is the keyboard face
  // of the same rule; the embed card's context menu is the pointable one.
  //
  // Mod-Enter is Obsidian's binding for exactly this, and it was free here (no
  // other keymap in the editor claims it).
  //
  // Prec.high so it beats the default keymap's Enter handling, and it returns
  // false when the cursor is on nothing followable, which lets a plain Enter
  // insert a newline as usual.
  const followKeymap = Prec.high(keymap.of([{
    key: 'Mod-Enter',
    run: (view: EditorView) => followAtCaret(view, view.state.selection.main.head, resolvedCache, follow),
  }]));

  return [domHandlers, followKeymap, chordTrackingExtension(),
    // Highest so that where an inert part starts with its construct (`[`, `![`,
    // `[[Page|`) its span nests INSIDE the whole-node `cm-link-clickable` and
    // wins the cursor; at equal starts the lower precedence wraps outermost.
    Prec.highest(inertPlugin),
  ];
}
