/**
 * The plus/cross glyph toggle — a button whose "+" turns into a "✗" (and
 * back) via icon-morph.ts's two-stroke engine, rather than swapping text
 * characters or DOM subtrees. Shared so every such button (the settings
 * panel's add-channel button, the Versions surface's save toggle) has the
 * exact same behavior instead of its own copy. icon-morph.ts stays
 * opinion-free about any particular glyph (its own header comment); this file
 * is where the plus/cross specifics live.
 *
 * Each caller owns its own `GlyphToggleState` (`createGlyphToggleState()`) —
 * two independent buttons must not share one running tween, or the second
 * button's open/close fights the first's.
 */
import { type Pose, drawStroke, tweenPose } from './icon-morph';

/* A plus and a cross are the same two bars through the same centre, 45°
   apart — so the button does not swap one icon for another, it turns the
   one it has. Both poses are Lucide's 24-box proportions (the `+` was
   already drawn that way), and the cross's arms are longer only because a
   diagonal of the same visual weight has to reach further to look equal.

   Angles are chosen for the SHORTEST roll: 180°→225° and 270°→225°+90°, so
   each bar turns 45° rather than spinning most of the way round. */
const G = 12; // the 24-box centre
const bar = (deg: number, len: number) => ({
  P: [G, G] as [number, number],
  phi: (deg * Math.PI) / 180,
  k: 0,
  len,
  bias: 0.5,
  op: 1,
});
export const PLUS_CROSS_GLYPH: Record<'plus' | 'cross', Pose> = {
  plus: { a: bar(180, 14), b: bar(270, 14) },
  cross: { a: bar(225, 17), b: bar(135, 17) },
};
const MORPH_MS = 260;

/** What is ON SCREEN right now for ONE button — not the semantic previous
 *  state, and not shared with any other button's toggle. ⌘, then Escape
 *  starts the second tween while the first is still running, and from a
 *  semantic `from` the glyph jump-cuts back to a full plus and then the two
 *  loops fight over the same two `d` attributes. Starting from the live pose
 *  is what `InstallRing` does, and for the same reason. */
export interface GlyphToggleState {
  cur: Pose;
  run: number;
}
export function createGlyphToggleState(): GlyphToggleState {
  return { cur: PLUS_CROSS_GLYPH.plus, run: 0 };
}

/* `aria-expanded` is where the OPEN/CLOSED fact is read from, not a second
   copy of it kept in `state`. The attribute has to be right anyway, and one
   source of truth cannot drift from itself. */
export function posePlusCrossGlyph(
  button: HTMLElement,
  open: boolean,
  state: GlyphToggleState,
  ms = MORPH_MS,
): void {
  const was = button.getAttribute('aria-expanded') === 'true';
  button.setAttribute('aria-expanded', String(open));
  const marks = button.querySelectorAll<SVGPathElement>('.moss-add-glyph__mk');
  if (marks.length !== 2) return; // pre-morph markup, or a test fixture
  if (open === was) return;

  const run = ++state.run;
  void tweenPose(state.cur, open ? PLUS_CROSS_GLYPH.cross : PLUS_CROSS_GLYPH.plus, ms, (p) => {
    if (run !== state.run) return; // a newer tween owns the glyph now
    state.cur = p;
    marks[0].setAttribute('d', drawStroke(p.a));
    marks[1].setAttribute('d', drawStroke(p.b));
  });
}
