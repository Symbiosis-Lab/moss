/**
 * source-target.ts — maps a preview DOM element to the source it was built from.
 *
 * Annotations, checked per node:
 *   data-source-fm="field"   frontmatter field
 *   data-source-range="N-M"  shortcode line range
 *   data-source-line="N"     body markdown line
 *   data-source-file="path"  site-relative file that owns the element
 *   data-source-none         template chrome, stops the walk
 *
 * resolveSourceTarget picks one target for click, double-click and drag.
 * resolveSourceTargets lists every target from the element outward, nearest
 * first, for the right-click menu, each naming the element that carries it
 * (`section.moss-hero`, `p`) so the menu can label the row. Both attach `path`
 * from the nearest data-source-file ancestor. They differ on purpose: the
 * single pick prefers a frontmatter field over a nearer line, and the list
 * stops at chrome.
 */

export type SourceTarget =
  | { kind: 'frontmatter'; field: string; path?: string; element?: string }
  | { kind: 'body-range'; startLine: number; endLine: number; path?: string; element?: string }
  | { kind: 'body-line'; line: number; path?: string; element?: string }
  | { kind: 'page'; path: string; element?: string }
  | { kind: 'none' };

/** A target that names a place in a file. */
type Located = Exclude<SourceTarget, { kind: 'none' }>;

const ANNOTATED = '[data-source-fm],[data-source-range],[data-source-line],[data-source-file],[data-source-none]';

function pathOf(el: Element): { path?: string } {
  const path = el.closest('[data-source-file]')?.getAttribute('data-source-file');
  return path == null ? {} : { path };
}

function bodyRange(node: Element): Located | null {
  const parts = node.getAttribute('data-source-range')!.split('-');
  const start = Number(parts[0]);
  const end = Number(parts[1]);
  if (isNaN(start) || isNaN(end)) return null;
  return { kind: 'body-range', startLine: start, endLine: end, ...pathOf(node) };
}

function bodyLine(node: Element): Located | null {
  const n = parseInt(node.getAttribute('data-source-line')!, 10);
  return n > 0 ? { kind: 'body-line', line: n, ...pathOf(node) } : null;
}

/** `tag` or `tag.moss-class`, the element's own moss component if it has one. */
function describe(node: Element): string {
  const tag = node.tagName.toLowerCase();
  const own = Array.from(node.classList).find((c) => c.startsWith('moss-'));
  return own ? `${tag}.${own}` : tag;
}

function sameTarget(a: Located, b: Located): boolean {
  const keys = Object.keys(b) as (keyof Located)[];
  return Object.keys(a).length === keys.length && keys.every((k) => a[k] === b[k]);
}

/** The one target a single annotated node carries, in priority order. */
function targetOnNode(node: Element): Located | null {
  const fm = node.getAttribute('data-source-fm');
  if (fm !== null) return { kind: 'frontmatter', field: fm, ...pathOf(node) };
  if (node.hasAttribute('data-source-range')) return bodyRange(node);
  if (node.hasAttribute('data-source-line')) return bodyLine(node);
  const file = node.getAttribute('data-source-file');
  return file !== null ? { kind: 'page', path: file } : null;
}

export function resolveSourceTarget(el: Element | null): SourceTarget | null {
  if (!el) return null;

  const fm = el.closest('[data-source-fm]');
  if (fm) return { kind: 'frontmatter', field: fm.getAttribute('data-source-fm')!, ...pathOf(fm) };

  const range = el.closest('[data-source-range]');
  if (range) return bodyRange(range);

  const line = el.closest('[data-source-line]');
  if (line) return bodyLine(line);

  if (el.closest('[data-source-none]')) return { kind: 'none' };

  return null;
}

export function resolveSourceTargets(el: Element | null): SourceTarget[] {
  const targets: SourceTarget[] = [];
  let prev: Located | null = null;
  let node = el?.closest(ANNOTATED) ?? null;
  while (node) {
    if (node.hasAttribute('data-source-none')) break;
    const t = targetOnNode(node);
    if (t && !(prev && sameTarget(prev, t))) targets.push({ ...t, element: describe(node) });
    prev = t;
    node = node.parentElement?.closest(ANNOTATED) ?? null;
  }
  return targets;
}

const THEME_SHEET_PATH = /^\/_moss\/theme\/style(\.[\w-]+)?\.css$/;
const MAX_THEME_SELECTORS = 3;

function themeSheet(): CSSStyleSheet | null {
  return Array.from(document.styleSheets).find((s) => !!s.href && THEME_SHEET_PATH.test(new URL(s.href).pathname)) ?? null;
}

/**
 * Style-rule selectors in sheet order, descending into @layer / @media /
 * @supports. A style rule is checked first: under CSS nesting it has
 * `cssRules` too, and its nested selectors are relative to it.
 */
function collectSelectors(rules: CSSRuleList, out: string[]): string[] {
  for (const rule of Array.from(rules)) {
    if ("selectorText" in rule) out.push((rule as CSSStyleRule).selectorText);
    else if ("cssRules" in rule) collectSelectors((rule as CSSGroupingRule).cssRules, out);
  }
  return out;
}

function matchesSafely(el: Element, selector: string): boolean {
  try {
    return el.matches(selector);
  } catch {
    return false;
  }
}

/**
 * The theme-sheet selectors that style what was clicked: walking up from `el`
 * (stopping before body), the first element that any rule matches wins, and
 * its matching selectors are returned in sheet order, at most three. The
 * nearest element is the one that was clicked, so an outer rule that only
 * matches an ancestor is left out.
 */
export function themeSelectors(el: Element | null, sheet = themeSheet()): string[] {
  if (!el || !sheet) return [];
  let selectors: string[];
  try {
    selectors = collectSelectors(sheet.cssRules, []);
  } catch {
    return [];
  }
  for (let node: Element | null = el; node && node.tagName !== "BODY"; node = node.parentElement) {
    const matched = selectors.filter((sel) => matchesSafely(node, sel));
    if (matched.length > 0) {
      return Array.from(new Set(matched)).slice(0, MAX_THEME_SELECTORS);
    }
  }
  return [];
}
