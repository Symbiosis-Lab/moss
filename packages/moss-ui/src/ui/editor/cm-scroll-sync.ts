// Line-based scroll sync between CM6 editor and preview webview.
//
// Primary strategy: send the top visible line number from CM6; the preview
// finds the nearest element with `data-source-line` and scrolls to it.
//
// Fallback strategy (for content without data-source-line): heading-anchor
// based sync. The anchor ID algorithm must match Rust's
// obsidian_heading_anchor() in crates/moss-core/src/heading/anchor.rs.

import { EditorView } from '@codemirror/view';

// ── Pure functions (tested directly) ────────────────────────────────────

/** Characters stripped from heading anchors (matches Rust STRIPPED). */
const STRIPPED = new Set(['#', '^', '|', '[', ']', '\\']);

/**
 * Convert heading text to an anchor ID matching Obsidian's behavior.
 *
 * Algorithm (must match `obsidian_heading_anchor` in heading/anchor.rs):
 * 1. Strip `#`, `^`, `|`, `[`, `]`, `\`
 * 2. Replace spaces and tabs with hyphens
 * 3. Lowercase all characters
 * 4. Collapse consecutive hyphens into one
 * 5. Trim leading/trailing hyphens
 */
export function headingToAnchor(heading: string): string {
  let result = '';
  for (const ch of heading) {
    if (STRIPPED.has(ch)) continue;
    if (ch === ' ' || ch === '\t') {
      result += '-';
    } else {
      result += ch.toLowerCase();
    }
  }

  // Collapse consecutive hyphens
  result = result.replace(/-{2,}/g, '-');

  // Trim leading/trailing hyphens
  return result.replace(/^-+|-+$/g, '');
}

export interface HeadingInfo {
  /** The heading text (without the `#` prefix). */
  text: string;
  /** The heading level (1-6). */
  level: number;
  /** The 1-based line number where the heading appears. */
  line: number;
}

/**
 * Find the nearest markdown heading at or above the given line number.
 *
 * @param doc - The full document text
 * @param lineNumber - 1-based line number to search from
 * @returns The nearest heading above (or at) the line, or null if none found
 */
export function findNearestHeading(doc: string, lineNumber: number): HeadingInfo | null {
  const lines = doc.split('\n');
  // Clamp to valid range
  const maxLine = Math.min(lineNumber, lines.length);

  // Scan backwards from the target line
  for (let i = maxLine - 1; i >= 0; i--) {
    const match = lines[i].match(/^(#{1,6})\s+(.+)/);
    if (match) {
      return {
        text: match[2].trim(),
        level: match[1].length,
        line: i + 1,
      };
    }
  }
  return null;
}

// ── Line-based scroll sync ────────────────────────────────────────────

/**
 * The viewport fraction used as the sync "focus line": 0 = top edge, 0.5 =
 * center, 1 = bottom edge. Both panes anchor the same source line at this
 * fraction of their viewport, so the line you're reading stays aligned.
 *
 * Center (0.5) is chosen over top (0) because moss's editor renders SOURCE while
 * the preview renders the (often much taller) transformed output — e.g. a
 * one-line `![[embed]]` becomes a full-height iframe. With a top anchor the
 * editor's short scroll range leaves the file's last viewport-worth of lines
 * unable to reach the top, so headers there can never become the anchor (a
 * "dead zone"); the center line reaches further into the document and dissolves
 * it. MUST stay in lockstep with `FOCUS_FRACTION` in
 * crates/moss-build/src/js-src/bridge/iframe-bridge.ts (the preview side aligns to the same
 * fraction).
 */
export const FOCUS_FRACTION = 0.5;

/**
 * Compute the offset of cm-content within its scroller (cm-scroller).
 * Equals the editor's file-title heading height when present, 0 otherwise.
 *
 * Two callers:
 * - `getTopVisibleLine` subtracts this offset before calling
 *   `lineBlockAtHeight` so the y-coordinate is in cm-content's coordinate
 *   system, not cm-scroller's.
 * - `onEditorScroll` (editor-main.ts) uses it to compute
 *   `atTop = scrollDOM.scrollTop < offset + TOP_THRESHOLD`, so the
 *   heading-visible range counts as "at top" — symmetric with the
 *   preview's `window.scrollY < TOP_THRESHOLD` which includes its own
 *   pre-content chrome.
 *
 * Uses getBoundingClientRect rather than offsetTop so the math is correct
 * regardless of CM6's default `position: relative` on .cm-scroller — the
 * bounding rects are CSS-position-independent.
 */
export function computeContentOffset(view: { scrollDOM: HTMLElement; contentDOM?: HTMLElement }): number {
  const scrollDOM = view.scrollDOM;
  const contentDOM = view.contentDOM;
  if (!contentDOM
      || typeof scrollDOM.getBoundingClientRect !== 'function'
      || typeof contentDOM.getBoundingClientRect !== 'function') {
    return 0;
  }
  return contentDOM.getBoundingClientRect().top
    - scrollDOM.getBoundingClientRect().top
    + scrollDOM.scrollTop;
}

/**
 * Get the 1-based line number at the editor's focus line — `fraction` of the
 * viewport down from the top (default {@link FOCUS_FRACTION}, the center).
 *
 * @param view - The CM6 EditorView instance
 * @param fraction - viewport fraction (0 = top, 0.5 = center, 1 = bottom)
 * @returns The 1-based line number at the focus line
 */
export function getFocusLine(view: { scrollDOM: HTMLElement; contentDOM?: HTMLElement; lineBlockAtHeight(h: number): { from: number }; state: { doc: { lineAt(pos: number): { number: number } } } }, fraction: number = FOCUS_FRACTION): number {
  const offset = computeContentOffset(view);
  const innerY = Math.max(0, view.scrollDOM.scrollTop - offset + view.scrollDOM.clientHeight * fraction);
  const block = view.lineBlockAtHeight(innerY);
  const line = view.state.doc.lineAt(block.from);
  return line.number;
}

/**
 * Scroll the editor so that the given 1-based line number sits at the focus
 * line — `fraction` of the viewport down from the top (default
 * {@link FOCUS_FRACTION}, the center). Mirrors the preview bridge's
 * center-alignment so a line reported by one pane lands at the same fraction in
 * the other.
 *
 * Note: scroll-to-top / -bottom semantics are handled by `scrollEditorToTop` /
 * `scrollEditorToBottom`; every receiver dispatches on the explicit `atTop` /
 * `atBottom` wire fields first.
 *
 * CM6 does the positioning, because only CM6 can place a line it has not
 * rendered: it renders the target first, then scrolls by its real coordinates
 * (`coordsAtPos` is null for any line outside the rendered viewport). `start` plus a margin puts the line's top `fraction` of the
 * viewport down; the margin is counted from below the scroll margin cm-editor
 * reserves for the title and chips (`contentDOM.offsetTop`), which CM6 adds
 * first. (`center` would ignore the margin and land below the middle.)
 *
 * @param view - The CM6 EditorView instance
 * @param lineNumber - 1-based line number to align
 * @param fraction - viewport fraction to align the line at
 */
export function scrollEditorToLine(view: Pick<EditorView, 'scrollDOM' | 'contentDOM' | 'state' | 'dispatch'>, lineNumber: number, fraction: number = FOCUS_FRACTION): void {
  const clampedLine = Math.max(1, Math.min(lineNumber, view.state.doc.lines));
  const pos = view.state.doc.line(clampedLine).from;
  const yMargin = Math.max(0, view.scrollDOM.clientHeight * fraction - view.contentDOM.offsetTop);
  view.dispatch({ effects: EditorView.scrollIntoView(pos, { y: 'start', yMargin }) });
}

/**
 * Scroll the editor to scrollTop=0. Used by the scroll-sync receiver
 * when the preview reports atTop: true, so the editor's file-title
 * heading stays visible alongside line 1's content rather than being
 * aligned-past by scrollEditorToLine's coords-based formula.
 *
 * Symmetric counterpart of iframe-bridge's window.scrollTo({top:0})
 * when the bridge receives scrollToSourceLine RPC with atTop.
 */
export function scrollEditorToTop(view: { scrollDOM: HTMLElement }): void {
  view.scrollDOM.scrollTop = 0;
}

/**
 * Scroll the editor to its maximum scrollTop. Used by the scroll-sync receiver
 * when the preview reports atBottom: true, so reaching the end of the page in
 * the preview reaches the end of the file in the editor — the symmetric
 * counterpart of scrollEditorToTop, and of the bridge's scrollTo(max) when it
 * receives scrollToSourceLine with atBottom.
 *
 * Assigning scrollHeight (past the real maximum) is clamped by the browser, so
 * this parks the scroller at its true bottom regardless of viewport height.
 */
export function scrollEditorToBottom(view: { scrollDOM: HTMLElement }): void {
  view.scrollDOM.scrollTop = view.scrollDOM.scrollHeight;
}

/**
 * Move the cursor to the beginning of the given 1-based line number
 * and scroll it into view. Used for click-to-source navigation.
 *
 * @param view - The CM6 EditorView instance
 * @param lineNumber - 1-based line number to navigate to
 */
export function gotoLine(view: { dispatch(spec: { selection: { anchor: number }; scrollIntoView: boolean }): void; focus(): void; state: { doc: { lines: number; line(n: number): { from: number } } } }, lineNumber: number): void {
  const clampedLine = Math.max(1, Math.min(lineNumber, view.state.doc.lines));
  const line = view.state.doc.line(clampedLine);
  view.dispatch({
    selection: { anchor: line.from },
    scrollIntoView: true,
  });
  view.focus();
}

let selectorSheet: CSSStyleSheet | undefined;

/** The engine's own spelling of a selector, or null when it rejects it. */
export function normalizeSelector(selector: string): string | null {
  selectorSheet ??= new CSSStyleSheet();
  try {
    selectorSheet.replaceSync(`${selector}{}`);
  } catch {
    return null;
  }
  const rule = selectorSheet.cssRules[0];
  return rule instanceof CSSStyleRule ? rule.selectorText : null;
}

/** At-rules whose block holds more rules, so style rules inside are reachable. */
const NESTING_AT_RULES = /^@(?:media|supports|layer|container|scope|document)\b/i;

/** Index just past the string or comment starting at `i`, or `i` when none starts there. */
function skipQuoted(src: string, i: number): number {
  const c = src[i];
  if (c === '/' && src[i + 1] === '*') {
    const end = src.indexOf('*/', i + 2);
    return end < 0 ? src.length : end + 2;
  }
  if (c !== '"' && c !== "'") return i;
  for (let j = i + 1; j < src.length; j++) {
    if (src[j] === '\\') j++;
    else if (src[j] === c || src[j] === '\n') return j + 1;
  }
  return src.length;
}

/** Index just past the `}` closing the block whose `{` precedes `i`. */
function skipBlock(src: string, i: number): number {
  for (let depth = 1; i < src.length; ) {
    const j = skipQuoted(src, i);
    if (j !== i) { i = j; continue; }
    if (src[i] === '{') depth++;
    else if (src[i] === '}' && --depth === 0) return i + 1;
    i++;
  }
  return src.length;
}

/** Split at commas outside parentheses, brackets and strings. */
function splitSelectorList(header: string): string[] {
  const items: string[] = [];
  let depth = 0;
  let start = 0;
  for (let i = 0; i < header.length; i++) {
    const j = skipQuoted(header, i);
    if (j !== i) { i = j - 1; continue; }
    const c = header[i];
    if (c === '(' || c === '[') depth++;
    else if (c === ')' || c === ']') depth--;
    else if (c === ',' && depth === 0) { items.push(header.slice(start, i)); start = i + 1; }
  }
  items.push(header.slice(start));
  return items.map((item) => item.trim()).filter(Boolean);
}

/**
 * The 1-based line of the first style-rule header with a list item that the
 * engine spells the same as `selector`. The preview reports selectors in the
 * engine's normalised form (`[a="b"]`, `::before`), so the author's selectors
 * are normalised the same way before comparing. Rules inside `@media` and
 * similar at-rules are searched; comments, strings and rejected selectors are not.
 */
export function findRuleLine(
  source: string,
  selector: string,
  normalize: (selector: string) => string | null = normalizeSelector,
): number | null {
  let header = '';
  let headerLine = 0;
  let line = 1;
  const advance = (from: number, to: number) => {
    for (let k = from; k < to; k++) if (source[k] === '\n') line++;
  };
  for (let i = 0; i < source.length; ) {
    const j = skipQuoted(source, i);
    if (j !== i) {
      const isComment = source[i] === '/';
      if (!isComment && !header.trim()) headerLine = line;
      if (!isComment) header += source.slice(i, j);
      else header += ' ';
      advance(i, j);
      i = j;
      continue;
    }
    const c = source[i];
    if (c === ';' || c === '}') {
      header = '';
    } else if (c === '{') {
      const text = header.trim();
      header = '';
      if (text.startsWith('@')) {
        if (!NESTING_AT_RULES.test(text)) { const end = skipBlock(source, i + 1); advance(i, end); i = end; continue; }
      } else {
        for (const item of splitSelectorList(text)) {
          if (normalize(item) === selector) return headerLine;
        }
        const end = skipBlock(source, i + 1);
        advance(i, end);
        i = end;
        continue;
      }
    } else {
      if (c === '\n') line++;
      if (!header.trim() && !/\s/.test(c)) headerLine = line;
      header += c;
    }
    i++;
  }
  return null;
}

/** Markdown formatting markers to strip when searching for preview text in source. */
const MD_MARKERS = /\*{1,2}|~~|`/g;

/**
 * Search for `text` near the given source line range and select it in the editor.
 * Falls back to gotoLine(startLine) if text is not found.
 *
 * @param view - The CM6 EditorView instance
 * @param startLine - 1-based start line (from data-source-line)
 * @param endLine - 1-based end line (same as start for single-block selections)
 * @param text - The selected text from the preview (plain text, no HTML tags)
 */
export function selectTextNearLine(
  view: {
    dispatch(spec: { selection: { anchor: number; head: number }; scrollIntoView: boolean }): void;
    focus(): void;
    state: { doc: { lines: number; line(n: number): { from: number; to: number }; sliceString(from: number, to: number): string } };
  },
  startLine: number,
  endLine: number,
  text: string,
): void {
  const doc = view.state.doc;
  const fromLine = Math.max(1, Math.min(startLine, doc.lines));
  const toLine = Math.max(fromLine, Math.min(endLine, doc.lines));

  // Build the search range: from start of fromLine to end of toLine
  const rangeFrom = doc.line(fromLine).from;
  const rangeTo = doc.line(toLine).to;
  const source = doc.sliceString(rangeFrom, rangeTo);

  // Try exact match first
  let idx = source.indexOf(text);
  let matchLen = text.length;

  // If not found, strip markdown formatting markers and retry
  if (idx === -1) {
    const stripped = source.replace(MD_MARKERS, '');
    const strippedIdx = stripped.indexOf(text);
    if (strippedIdx !== -1) {
      // Map stripped index back to original source position.
      // Walk the original, skipping marker chars, counting content chars.
      idx = mapStrippedToOriginal(source, strippedIdx);
      // Walk forward text.length content chars to find the end position
      const endOrig = mapStrippedToOriginal(source, strippedIdx + text.length);
      matchLen = endOrig - idx;
    }
  }

  if (idx !== -1) {
    // Found — select the text
    const anchor = rangeFrom + idx;
    const head = rangeFrom + idx + matchLen;
    view.dispatch({
      selection: { anchor, head },
      scrollIntoView: true,
    });
  } else {
    // Fallback: just go to the line
    const line = doc.line(fromLine);
    view.dispatch({
      selection: { anchor: line.from, head: line.from },
      scrollIntoView: true,
    });
  }
  view.focus();
}

/**
 * Map a character index in marker-stripped text back to the original source.
 * Walks the original source, skipping markdown markers, counting content
 * characters until the target count is reached.
 */
export function mapStrippedToOriginal(source: string, strippedIndex: number): number {
  let origIdx = 0;
  let count = 0;
  while (origIdx < source.length) {
    // Skip markers at current position
    const markerMatch = source.slice(origIdx).match(/^(\*{1,2}|~~|`)/);
    if (markerMatch) {
      origIdx += markerMatch[0].length;
      continue;
    }
    // We've landed on a content character
    if (count === strippedIndex) return origIdx;
    origIdx++;
    count++;
  }
  return origIdx;
}
