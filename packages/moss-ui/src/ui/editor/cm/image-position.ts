/**
 * Where an image is cropped to: the position word in an embed's pipe options
 * (`![[a.jpg|cover top-left]]`, `![[a.jpg|focus:20%,62%]]`), in a markdown
 * image's `(path|options)`, or in a hero's `image="path|options"` attribute.
 *
 * Reading and writing follow moss-core's `parse_media_attrs`: a position is one
 * of nine names (also `topleft` and the two-word `top left`) or `focus:X%,Y%`
 * with integer percentages. A pipe segment made only of such keywords is the
 * frame's options; any other segment (a width, a caption) is left alone.
 */

import { heroAttrs, type HeroAttr } from './hero-opener';

export interface Point { x: number; y: number }

const NAMED: Record<string, Point> = {
  center: { x: 50, y: 50 },
  top: { x: 50, y: 0 },
  bottom: { x: 50, y: 100 },
  left: { x: 0, y: 50 },
  right: { x: 100, y: 50 },
  'top-left': { x: 0, y: 0 },
  'top-right': { x: 100, y: 0 },
  'bottom-left': { x: 0, y: 100 },
  'bottom-right': { x: 100, y: 100 },
};

/** A dot this close (in percentage points, on both axes) to a named position takes the name. */
export const SNAP_POINTS = 4;

const FIT = new Set(['cover', 'contain', 'fill', 'none', 'scale-down', 'scaledown']);
const ALIGN = new Set(['align-left', 'align-right', 'alignleft', 'alignright']);

/** The point a position word names, or null when it is not one. */
export function pointOfPosition(word: string): Point | null {
  const w = word.trim().toLowerCase().replace(/\s+/g, '-');
  const named = NAMED[w] ?? NAMED[w.replace(/^(top|bottom)(left|right)$/, '$1-$2')];
  if (named) return { ...named };
  const m = /^focus:(\d{1,3})%,(\d{1,3})%$/.exec(w);
  if (!m) return null;
  const x = Number(m[1]);
  const y = Number(m[2]);
  return x <= 100 && y <= 100 ? { x, y } : null;
}

/** The word to write for a point: a name when within {@link SNAP_POINTS} of one, else `focus:X%,Y%`. */
export function positionWord(px: number, py: number): string {
  const clamp = (v: number) => Math.min(100, Math.max(0, v));
  const x = clamp(px);
  const y = clamp(py);
  let best: { name: string; d: number } | null = null;
  for (const [name, p] of Object.entries(NAMED)) {
    const dx = Math.abs(p.x - x);
    const dy = Math.abs(p.y - y);
    if (dx > SNAP_POINTS || dy > SNAP_POINTS) continue;
    const d = Math.max(dx, dy);
    if (!best || d < best.d) best = { name, d };
  }
  if (best) return best.name;
  return `focus:${Math.round(x)}%,${Math.round(y)}%`;
}

/**
 * The part of an `object-fit: cover` picture that a frame shows, as fractions of
 * the picture, when the focus point is (x, y) in 0..1.
 */
export function coverRect(imageAspect: number, frameAspect: number, x: number, y: number): { left: number; top: number; width: number; height: number } {
  const wide = frameAspect > imageAspect;
  const width = wide ? 1 : frameAspect / imageAspect;
  const height = wide ? imageAspect / frameAspect : 1;
  return { left: (1 - width) * x, top: (1 - height) * y, width, height };
}

// ── Pipe options ─────────────────────────────────────────────────────

interface Token { text: string; position: boolean }

function tokens(segment: string): Token[] {
  const words = segment.split(/\s+/).filter(Boolean);
  const out: Token[] = [];
  for (let i = 0; i < words.length; i++) {
    const pair = i + 1 < words.length ? `${words[i]} ${words[i + 1]}` : null;
    if (pair && pointOfPosition(pair)) { out.push({ text: pair, position: true }); i++; continue; }
    out.push({ text: words[i], position: pointOfPosition(words[i]) !== null });
  }
  return out;
}

function isKeyword(t: Token): boolean {
  const w = t.text.toLowerCase();
  return t.position || FIT.has(w) || ALIGN.has(w) || w.startsWith('color=');
}

/** A segment of nothing but frame keywords; anything else is a width or a caption. */
function frameTokens(segment: string): Token[] | null {
  const ts = tokens(segment);
  return ts.length > 0 && ts.every(isKeyword) ? ts : null;
}

/** The last position among the frame segments, as a point. */
export function positionOfSegments(segments: readonly string[]): Point | null {
  let found: Point | null = null;
  for (const seg of segments) {
    for (const t of frameTokens(seg) ?? []) if (t.position) found = pointOfPosition(t.text);
  }
  return found;
}

/**
 * The segments with `word` as their one position: it replaces the first position
 * found, any other position is dropped, every other token stays. With none to
 * replace it joins the first frame segment, or becomes a segment of its own.
 */
export function segmentsWithPosition(segments: readonly string[], word: string): string[] {
  const out = [...segments];
  let placed = false;
  for (let i = 0; i < out.length; i++) {
    const ts = frameTokens(out[i]);
    if (!ts || !ts.some((t) => t.position)) continue;
    const next: string[] = [];
    for (const t of ts) {
      if (!t.position) next.push(t.text);
      else if (!placed) { next.push(word); placed = true; }
    }
    out[i] = next.join(' ');
  }
  if (placed) return out;
  const frame = out.findIndex((s) => frameTokens(s));
  if (frame >= 0) out[frame] = `${out[frame].trim()} ${word}`;
  else out.push(word);
  return out;
}

// ── An embed: `![[path|opts]]` or `![alt](path|opts)` ────────────────

const WIKILINK_EMBED = /^(!\[\[[^\]|]*)((?:\|[^\]]*)?)(\]\])$/;
const MARKDOWN_EMBED = /^(!\[[^\]]*\]\([^)|]*)((?:\|[^)]*)?)(\))$/;

function splitEmbed(md: string): { head: string; segments: string[]; tail: string } | null {
  const m = WIKILINK_EMBED.exec(md) ?? MARKDOWN_EMBED.exec(md);
  return m ? { head: m[1], segments: m[2].slice(1).split('|').filter((s) => s.trim()), tail: m[3] } : null;
}

export function embedPosition(md: string): Point | null {
  const e = splitEmbed(md);
  return e ? positionOfSegments(e.segments) : null;
}

/** The embed with `word` as its position, or the embed unchanged when `md` is not one. */
export function embedWithPosition(md: string, word: string): string {
  const e = splitEmbed(md);
  if (!e) return md;
  return `${e.head}|${segmentsWithPosition(e.segments, word).join('|')}${e.tail}`;
}

// ── A hero's opening line: `:::hero {image="path|opts" …}` ───────────

const BAREWORD = /^[\p{L}\p{N}:/._-]+$/u;

/** The `image=` attribute of a hero's opening line. */
export function imageAttrOf(line: string): HeroAttr | null {
  const attr = heroAttrs(line).attrs.find((a) => a.key === 'image');
  return attr?.value ? attr : null;
}

export function imageAttrPosition(attr: HeroAttr): Point | null {
  return positionOfSegments(attr.value.split('|').slice(1));
}

/** The replacement text for the whole `image=` attribute. */
export function imageAttrWithPosition(attr: HeroAttr, word: string): string {
  const [path, ...rest] = attr.value.split('|');
  const value = [path, ...segmentsWithPosition(rest, word)].join('|');
  return BAREWORD.test(value) ? `image=${value}` : `image="${value.replace(/[\\"]/g, '\\$&')}"`;
}
