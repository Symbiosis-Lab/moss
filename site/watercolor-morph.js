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
// A scene's dissolve is recorded once per print, on the GPU, as a handful of
// stored frames of the film (the engine's recordDissolve), and every frame
// shown is two stored frames mixed: neighbouring frames of one dissolve in the
// outer phases, the two well-mixed washes in the middle. So what is shown is a
// pure function of p, the same going forwards or back, with nothing replayed.
// The middle mixes the two washes' pigment rather than re-tinting water on the
// sheet: a well-mixed wash has no spatial structure left for physics to move,
// so exchanging its pigment is a change of uniform concentration, which
// interpolation states exactly, reversibly and without a clock.
//
// The phase bounds are symmetric (A_END = 1 - B_START), so a leg read from the
// other end is the same leg: A to B at p shows what B to A shows at 1 - p. The
// middle gets the smaller share because nothing structural happens in it.
//
// The engine is makeSim's instance (landing.js): recordDissolve, advanceRecord,
// disposeRecord, setPrints, holds and present. This file owns the model and the cache,
// nothing about scenes, scroll or the DOM.
(function (global) {
  'use strict';
  const A_END = 0.35, B_START = 0.65;
  const smoothstep = (x) => { x = Math.min(1, Math.max(0, x)); return x * x * (3 - 2 * x); };

  // Where p falls: which two stored frames to mix and by how much. `us` are
  // the fractions of the dissolve its stored frames sit at (the last is 1,
  // the well-mixed wash); frame 0 is the print at rest. Pure, so a check can
  // hold the page to it.
  function frameAt(p, us) {
    const K = us.length;
    const along = (q) => {
      // q runs along a dissolve, 0 at the print and 1 at its wash
      const at = [0, ...us];
      let i = 0; while (i < K - 1 && q > at[i + 1]) i++;
      return { k0: i, k1: i + 1, w: (q - at[i]) / (at[i + 1] - at[i]) };
    };
    if (p <= A_END) { const f = along(Math.max(0, p) / A_END); return { f0: ['a', f.k0], f1: ['a', f.k1], w: f.w }; }
    if (p < B_START) return { f0: ['a', K], f1: ['b', K], w: smoothstep((p - A_END) / (B_START - A_END)) };
    const f = along(Math.max(0, 1 - p) / (1 - B_START));
    return { f0: ['b', f.k1], f1: ['b', f.k0], w: 1 - f.w };
  }

  // engine: the makeSim instance. frames: where along a dissolve its stored
  // frames sit (denser early, where the splash and blooms move fastest).
  // steps: the dissolve's length in sim steps. keep: how many prints' records
  // stay on the GPU at once (a leg needs two; a third lets the next one in
  // either direction be warm).
  function create(engine, { frames, steps, keep = 3 }) {
    const records = new Map();   // print (a canvas) -> record, oldest first
    const recordFor = (print) => {
      let rec = records.get(print);
      if (rec) { records.delete(print); records.set(print, rec); return rec; }
      rec = engine.recordDissolve(print, frames, steps);
      records.set(print, rec);
      for (const [old, r] of records) {
        if (records.size <= keep) break;
        if (current && (r === current.ra || r === current.rb)) continue;   // the leg on screen is drawing from it
        engine.disposeRecord(r); records.delete(old);
      }
      return rec;
    };
    let current = null;
    return {
      frames, steps, A_END, B_START,
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
      has: (print) => records.has(print),
      // Mounts the leg from print a to print b: both are held for display
      // and their dissolves are recorded as the leg is rendered. `tag` is the
      // caller's name for the leg, handed back by current().
      leg(a, b, tag = null) {
        engine.setPrints(a, b);
        current = null;
        const ra = recordFor(a), rb = recordFor(b);
        const leg = current = { a, b, ra, rb, tag, p: -1 };
        return {
          // Presents p, spending at most `budget` steps finishing a record this
          // leg still lacks (a's first: it is the one shown first). Returns the
          // steps spent and whether the frame shown is exactly p's -- false
          // only while a record is still being made, which asks the caller to
          // render again next frame.
          render(p, budget) {
            // Another caller (the warmer's cover, the closing wash) may have
            // put its own prints in the display slots since the last frame.
            if (!engine.holds(a, b)) engine.setPrints(a, b);
            let spent = 0;
            for (const rec of [ra, rb]) if (spent < budget) spent += engine.advanceRecord(rec, budget - spent);
            const at = frameAt(p, frames);
            const rec = { a: ra, b: rb };
            // A frame not yet recorded shows the furthest a has reached: the
            // same record for a's own, and a's side of the middle for b's.
            const ok = ([side, k]) => k <= rec[side].done;
            const pick = (f) => ok(f) ? { slot: f[0] === 'a' ? 'src' : 'tgt', rec: rec[f[0]], k: f[1] } : { slot: 'src', rec: ra, k: ra.done };
            const exact = (at.w >= 1 || ok(at.f0)) && (at.w <= 0 || ok(at.f1));
            engine.present(pick(at.f0), pick(at.f1), at.w);
            leg.p = p;
            return { spent, exact };
          },
          ready: () => ra.done === frames.length && rb.done === frames.length,
        };
      },
      // The leg is off screen: its records may be released like any other.
      end() { current = null; },
      // For the harness: the leg on screen, the p it last presented, its prints and tag.
      current: () => current && { p: current.p, a: current.a, b: current.b, tag: current.tag },
    };
  }

  global.WatercolorMorph = { create, frameAt, A_END, B_START };
})(window);
