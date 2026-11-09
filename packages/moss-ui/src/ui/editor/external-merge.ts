// Merging a disk text another writer produced into the open document.
//
// Three texts: the base (the disk text the buffer was built from), the disk
// (what is there now) and the buffer (the base plus the person's unsaved
// edits). The disk's changes are applied to the buffer when they touch no line
// the person changed, mapped through the person's changes so their caret,
// selection and undo history stay on their own text. Where a disk change
// overlaps or touches one of the person's, the person's text wins and the disk
// change is set aside; `outsideWins` is the same merge with the other writer
// winning instead, which is what Undo puts back. Pure, so it is tested without
// an editor.

import { ChangeSet } from '@codemirror/state';
import { deepEqual } from './document-history';

export interface DocumentText { body: string; frontmatter: Record<string, unknown> }

/** A replacement of `a.slice(from, to)` by `insert`. */
export interface Hunk { from: number; to: number; insert: string }

export interface ExternalMerge {
  /** Applies to the buffer's body: the disk's edits that touch no line the person changed. */
  changes: ChangeSet;
  /** Merged by key; on a key both sides changed, the person's value. */
  frontmatter: Record<string, unknown>;
  /** The disk's changes set aside because they overlap the person's, in base offsets. */
  overlaps: Hunk[];
  /** Keys both sides changed to different values, where the person's value was kept. */
  keys: string[];
}

/** Nothing of the other writer's was set aside. */
export const isClean = (merge: ExternalMerge): boolean => merge.overlaps.length === 0 && merge.keys.length === 0;

/** Past this many line pairs the diff stops looking for common lines inside the changed middle. */
const MAX_DIFF_CELLS = 4_000_000;

function lines(text: string): string[] {
  return text.match(/[^\n]*\n|[^\n]+$/g) ?? [];
}

/** Whole-line hunks turning `a` into `b`, in `a`'s offsets: a longest-common-subsequence line diff. */
export function lineDiff(a: string, b: string): Hunk[] {
  const la = lines(a), lb = lines(b);
  let pre = 0;
  while (pre < la.length && pre < lb.length && la[pre] === lb[pre]) pre++;
  let suf = 0;
  while (suf < la.length - pre && suf < lb.length - pre && la[la.length - 1 - suf] === lb[lb.length - 1 - suf]) suf++;
  const ma = la.slice(pre, la.length - suf), mb = lb.slice(pre, lb.length - suf);
  const offset = la.slice(0, pre).reduce((n, l) => n + l.length, 0);
  if (ma.length === 0 && mb.length === 0) return [];
  if (ma.length * mb.length > MAX_DIFF_CELLS) {
    return [{ from: offset, to: offset + ma.join('').length, insert: mb.join('') }];
  }
  // lcs[i][j] = common lines of ma[i..] and mb[j..]
  const w = mb.length + 1;
  const lcs = new Uint32Array((ma.length + 1) * w);
  for (let i = ma.length - 1; i >= 0; i--) {
    for (let j = mb.length - 1; j >= 0; j--) {
      lcs[i * w + j] = ma[i] === mb[j] ? lcs[(i + 1) * w + j + 1] + 1 : Math.max(lcs[(i + 1) * w + j], lcs[i * w + j + 1]);
    }
  }
  const hunks: Hunk[] = [];
  let pos = offset, i = 0, j = 0;
  let open: Hunk | null = null;
  const close = () => { if (open) { hunks.push(open); open = null; } };
  while (i < ma.length || j < mb.length) {
    if (i < ma.length && j < mb.length && ma[i] === mb[j]) {
      close();
      pos += ma[i].length; i++; j++;
    } else if (j < mb.length && (i === ma.length || lcs[i * w + j + 1] >= lcs[(i + 1) * w + j])) {
      open ??= { from: pos, to: pos, insert: '' };
      open.insert += mb[j++];
    } else {
      open ??= { from: pos, to: pos, insert: '' };
      pos += ma[i].length; open.to = pos; i++;
    }
  }
  close();
  return hunks;
}

/** `hunk` trimmed to the characters that differ, so a caret on the unchanged part of a line stays put. */
function refine(a: string, hunk: Hunk): Hunk {
  const old = a.slice(hunk.from, hunk.to);
  const max = Math.min(old.length, hunk.insert.length);
  let p = 0;
  while (p < max && old.charCodeAt(p) === hunk.insert.charCodeAt(p)) p++;
  let s = 0;
  while (s < max - p && old.charCodeAt(old.length - 1 - s) === hunk.insert.charCodeAt(hunk.insert.length - 1 - s)) s++;
  return { from: hunk.from + p, to: hunk.to - s, insert: hunk.insert.slice(p, hunk.insert.length - s) };
}

/** Two hunks in the same text overlap or touch: the same or adjacent lines. */
function collide(x: Hunk, y: Hunk): boolean {
  return x.from <= y.to && y.from <= x.to;
}

const same = (x: Hunk, y: Hunk) => x.from === y.from && x.to === y.to && x.insert === y.insert;

function mergeFrontmatter(
  base: Record<string, unknown>, disk: Record<string, unknown>, buffer: Record<string, unknown>, winner: 'disk' | 'buffer',
): { merged: Record<string, unknown>; conflicts: string[] } {
  const merged: Record<string, unknown> = {};
  const conflicts: string[] = [];
  const keys = new Set([...Object.keys(buffer), ...Object.keys(disk), ...Object.keys(base)]);
  for (const key of keys) {
    const b = base[key], d = disk[key], u = buffer[key];
    let value: unknown;
    if (deepEqual(d, b) || deepEqual(d, u)) value = u;
    else if (deepEqual(u, b)) value = d;
    else { conflicts.push(key); value = winner === 'disk' ? d : u; }
    if (value !== undefined) merged[key] = value;
  }
  return { merged, conflicts };
}

/**
 * Bring `disk` into `buffer`, both descended from `base`. A disk line change
 * that overlaps or touches a line the person changed is set aside in
 * `overlaps`, and a key both changed keeps the person's value; everything
 * else of the disk's is in `changes`, which applies to the buffer's body.
 */
export function mergeExternal(base: DocumentText, disk: DocumentText, buffer: DocumentText): ExternalMerge {
  const ours = lineDiff(base.body, buffer.body);
  const theirs = lineDiff(base.body, disk.body).filter((t) => !ours.some((o) => same(o, t)));
  const overlaps = theirs.filter((t) => ours.some((o) => collide(o, t)));
  const kept = theirs.filter((t) => !overlaps.includes(t));
  const { merged, conflicts } = mergeFrontmatter(base.frontmatter, disk.frontmatter, buffer.frontmatter, 'buffer');
  const length = base.body.length;
  const changes = ChangeSet.of(kept.map((h) => refine(base.body, h)), length)
    .map(ChangeSet.of(ours.map((h) => refine(base.body, h)), length));
  return { changes, frontmatter: merged, overlaps, keys: conflicts };
}

/**
 * The same three texts merged with the other writer winning every overlap:
 * all of the disk's changes, and the person's that touch none of them.
 */
export function outsideWins(base: DocumentText, disk: DocumentText, buffer: DocumentText): DocumentText {
  const theirs = lineDiff(base.body, disk.body);
  const ours = lineDiff(base.body, buffer.body).filter((o) => !theirs.some((t) => collide(o, t)));
  let body = base.body;
  for (const h of [...theirs, ...ours].sort((x, y) => y.from - x.from)) body = body.slice(0, h.from) + h.insert + body.slice(h.to);
  return { body, frontmatter: mergeFrontmatter(base.frontmatter, disk.frontmatter, buffer.frontmatter, 'disk').merged };
}

/** The edit turning `a` into `b`, touching only the characters that differ in each changed line. */
export function changesBetween(a: string, b: string): ChangeSet {
  return ChangeSet.of(lineDiff(a, b).map((h) => refine(a, h)), a.length);
}
