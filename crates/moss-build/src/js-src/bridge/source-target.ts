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
