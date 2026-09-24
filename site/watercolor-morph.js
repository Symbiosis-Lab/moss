// WatercolorMorph: the one transition between two scenes, on every leg and
// both layouts. The site owner's model, in three phases of one position p:
//
//   [0, A_END]       scene A dissolves, alone, into a well-mixed wash that
//                    holds A's own pigment, amount and colour ratio;
//   [A_END, B_START] that wash changes, gradually, into the well-mixed wash
//                    B's own dissolve reaches;
//   [B_START, 1]     scene B consolidates out of it: B's own dissolve, run
//                    backwards.
//
// A scene's dissolve is the simulation itself, run on the GPU once per print
// (the engine's recordDissolve) with a checkpoint of the film's whole state
// every few steps. The frame shown at p is that dissolve at the step p names,
// replayed from the checkpoint before it: the real film, so a bloom spreads
// as p moves instead of fading in between two stored frames, and a pure
// function of p, since the dissolve's inputs are fixed (a fixed dt, no
// scroll speed, no tilt) and replay reaches the same state going forwards or
// back. The middle mixes the two washes' pigment rather than re-tinting water
// on the sheet: a well-mixed wash has no spatial structure left for physics
// to move, so exchanging its pigment is a change of uniform concentration,
// which interpolation states exactly, reversibly and without a clock.
//
// The phase bounds are symmetric (A_END = 1 - B_START), so a leg read from the
// other end is the same leg: A to B at p shows what B to A shows at 1 - p. The
// middle gets the smaller share because nothing structural happens in it.
//
// Membership: whatever a print carries of an element that stays in the page
// (a card, a video, the logos) is a member of the leg, and the live element
// is hidden, by one class, from the frame after the canvas first shows the
// print that holds it until the leg lets go of it: never half live and half
// print, and never a frame with neither. An element its scene could not
// capture is not in the print, so it cannot hide behind it: it fades itself
// out with its own scene's phase instead (in over B's), never a pop.
//
// The engine is makeSim's instance (landing.js): recordDissolve, advanceRecord,
// seek, disposeRecord, setPrints, holds and present. This file owns the model,
// the cache and the members' visibility; nothing about scenes or scroll.
//
// One leg (SHIPS<->DEPLOY, landing.js's publishBridge) is not consolidated
// by this model at all: scene 4 has no pigment identity of its own to
// consolidate into, since its content is scene 3's own Publish control,
// scaled up. That leg still dissolves its background the same three-phase
// way -- SHIPS has other ink to clear -- but is mounted with `carry: true`
// (leg()'s own parameter) so a caller reading current() knows the leg's
// readable content is the live control landing.js holds solid over the
// wash, not anything this file's dissolve produces.
(function (global) {
  'use strict';
  const A_END = 0.35, B_START = 0.65;
  const smoothstep = (x) => { x = Math.min(1, Math.max(0, x)); return x * x * (3 - 2 * x); };
  const MEMBER = 'wash-member';

  // Where p falls: which step of which dissolve to show, or in the middle
  // which two washes to mix and by how much. A dissolve is N steps from the
  // print at rest (0) to its well-mixed wash (N). Pure, so a check can hold
  // the page to it.
  function frameAt(p, N) {
    if (p <= A_END) { const s = Math.round(N * Math.max(0, p) / A_END); return { f0: ['a', s], f1: ['a', s], w: 0 }; }
    if (p < B_START) return { f0: ['a', N], f1: ['b', N], w: smoothstep((p - A_END) / (B_START - A_END)) };
    const s = Math.round(N * Math.max(0, 1 - p) / (1 - B_START));
    return { f0: ['b', s], f1: ['b', s], w: 0 };
  }

  // engine: the makeSim instance. steps: a dissolve's length in sim steps.
  // every: the checkpoint spacing, which bounds the replay one frame costs.
  // keep: how many prints' records stay on the GPU at once (a leg needs two;
  // a third lets the next one in either direction be warm).
  function create(engine, { steps, every, keep = 3 }) {
    const records = new Map();   // print (a canvas) -> record, oldest first
    const recordFor = (print) => {
      let rec = records.get(print);
      if (rec) { records.delete(print); records.set(print, rec); return rec; }
      rec = engine.recordDissolve(print, steps, every);
      records.set(print, rec);
      for (const [old, r] of records) {
        if (records.size <= keep) break;
        if (current && (r === current.ra || r === current.rb)) continue;   // the leg on screen is drawing from it
        engine.disposeRecord(r); records.delete(old);
      }
      return rec;
    };
    let current = null;
    // The members of the leg on screen: shown again, and any fade undone.
    const release = () => {
      if (!current) return;
      cancelAnimationFrame(current.hiding);
      for (const m of current.members) { m.el.classList.remove(MEMBER); m.el.style.opacity = ''; }
      current.hidden = false; current.hiding = 0;
    };
    // Called with every frame presented: members hide while the canvas shows
    // the leg between its ends, and only once it has shown it for a frame.
    const showMembers = (leg, p) => {
      if (!(p > 0 && p < 1)) { release(); return; }
      if (!leg.hidden && !leg.hiding) leg.hiding = requestAnimationFrame(() => {
        leg.hiding = 0; if (current !== leg) return;
        for (const m of leg.members) if (m.captured) m.el.classList.add(MEMBER);
        leg.hidden = true;
      });
      for (const m of leg.members) if (!m.captured) m.el.style.opacity = String(m.side === 'a' ? 1 - smoothstep(p / A_END) : smoothstep((p - B_START) / (1 - B_START)));
    };
    return {
      steps, every, A_END, B_START,
      // Records a print's dissolve to the end now, if it is not already on
      // hand and a slot is free; returns the steps that took. Never evicts:
      // a warmer that evicted would re-record what it evicted on its next
      // tick, forever. retain() is what frees slots.
      warm(print) {
        if (!print || records.has(print) || records.size >= keep) return 0;
        return engine.advanceRecord(recordFor(print), Infinity);
      },
      // Releases every record whose print is not in `prints` and not drawn
      // by the leg on screen: a retaken print's old record is dead weight.
      retain(prints) {
        for (const [print, r] of records) {
          if (prints.includes(print) || (current && (r === current.ra || r === current.rb))) continue;
          engine.disposeRecord(r); records.delete(print);
        }
      },
      // Mounts the leg from print a to print b: both are held for display
      // and their dissolves are recorded as the leg is rendered. `tag` is the
      // caller's name for the leg, handed back by current(). `members`:
      // [{ el, side: 'a' | 'b', captured }], the live elements whose pixels
      // each print carries (captured) or should have and does not. `carry`:
      // this leg's own dissolve still runs underneath (SHIPS's background
      // still needs to clear), but the leg's *readable content* is not that
      // dissolve -- it is a single live element (the Publish control) held
      // solid across the whole leg by its own caller (publishBridge in
      // landing.js), scaling continuously between its two scenes' sizes
      // rather than being carried by pigment consolidating out of a wash.
      // Declared here, not inferred by a checker from scene indices, so
      // anything reading current() (check-landing-morph.mjs's noBlank
      // clause, which the owner asked replaced for this leg with the
      // control's own visibility and scale instead) has it from the one
      // place that actually knows.
      leg(a, b, tag = null, members = [], carry = false) {
        engine.setPrints(a, b);
        release();
        current = null;
        const ra = recordFor(a), rb = recordFor(b);
        const leg = current = { a, b, ra, rb, tag, carry, p: -1, exact: true, members, hidden: false, hiding: 0 };
        return {
          // Presents p, spending at most `budget` steps recording a dissolve
          // this leg still lacks (a's first: it is the one shown first), plus
          // the replay to p's own step. Returns the steps spent and whether
          // the frame shown is exactly p's -- false only while a record is
          // still being made, which asks the caller to render again next frame.
          render(p, budget) {
            // Another caller (the warmer's cover, the closing wash) may have
            // put its own prints in the display slots since the last frame.
            if (!engine.holds(a, b)) engine.setPrints(a, b);
            let spent = 0;
            for (const rec of [ra, rb]) if (spent < budget) spent += engine.advanceRecord(rec, budget - spent);
            const at = frameAt(p, steps);
            const rec = { a: ra, b: rb };
            // A step not yet recorded shows the furthest a has reached: the
            // same dissolve for a's own, and a's side of the middle for b's.
            const ok = ([side, s]) => s <= rec[side].n;
            const exact = ok(at.f0) && ok(at.f1);
            const [f0, f1] = exact ? [at.f0, at.f1] : [['a', ra.n], ['a', ra.n]];
            const frame = ([side, s]) => ({ slot: side === 'a' ? 'src' : 'tgt', rec: rec[side], s });
            // Only one frame is ever between checkpoints (the outer phases show
            // a single step; the middle, two finished washes), so one seek does,
            // and a step on a checkpoint is shown from the checkpoint itself.
            if (f0[1] % every) spent += engine.seek(rec[f0[0]], f0[1]);
            engine.present(frame(f0), frame(f1), exact ? at.w : 0);
            leg.p = p; leg.exact = exact;
            showMembers(leg, p);
            return { spent, exact };
          },
        };
      },
      // The leg is off screen: its members are live again, and its records
      // may be released like any other.
      end() { release(); current = null; },
      // For the harness: the leg on screen, the p it last presented, whether
      // that frame was exactly p's, its prints, tag and carry flag.
      current: () => current && { p: current.p, exact: current.exact, a: current.a, b: current.b, tag: current.tag, carry: current.carry },
    };
  }

  global.WatercolorMorph = { create, frameAt, A_END, B_START, MEMBER };
})(window);
