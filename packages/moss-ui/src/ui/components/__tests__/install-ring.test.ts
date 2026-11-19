/**
 * Geometry tests for the install ring.
 *
 * These assert the things that are true of the SHAPES, which is where the
 * morph's correctness actually lives — jsdom lays nothing out, so anything
 * about how it LOOKS belongs in a render gate. What can be proved here is that
 * the poses are the same two strokes, that a round trip returns byte-identical
 * path data, that the check is a real check rather than a cross, and that the
 * paced floor cannot claim the work is finished.
 */

import { describe, it, expect, vi, afterEach } from "vitest";
import { InstallRing, __testing } from "../install-ring";

const { strokeD, pose, lerpPose, floorAt, arcValue, CIRC, R_RING, Marks, W_RING, W_REST } = __testing;

/** Endpoints of a rendered stroke, whether it is a bar or an arc. */
function ends(d: string): [[number, number], [number, number]] {
  const n = d.match(/-?\d+\.?\d*/g)!.map(Number);
  // "M x,y L x,y" → 4 numbers; "M x,y A rx,ry rot laf sf x,y" → 9.
  return [[n[0], n[1]], [n[n.length - 2], n[n.length - 1]]];
}
const dist = (p: number[], q: number[]) => Math.hypot(p[0] - q[0], p[1] - q[1]);

describe("install ring poses", () => {
  it("a minus is a plus with stroke B collapsed — not a different drawing", () => {
    const plus = pose.plus(), minus = pose.minus();
    expect(minus.a).toEqual(plus.a);
    expect(minus.b.len).toBe(0);
    expect(strokeD(minus.b.P, minus.b.phi, minus.b.k, minus.b.len, minus.b.bias)).toBe("");
  });

  it("every pose is the same two strokes, so each has a counterpart to morph to", () => {
    for (const p of [pose.plus(), pose.minus(), pose.ring(50), pose.check(), pose.fail()]) {
      expect(Object.keys(p).sort()).toEqual(["a", "b"]);
    }
  });

  it("the check's two arms meet at the elbow instead of crossing", () => {
    const c = pose.check();
    const [a0, a1] = ends(strokeD(c.a.P, c.a.phi, c.a.k, c.a.len, c.a.bias));
    const [b0, b1] = ends(strokeD(c.b.P, c.b.phi, c.b.k, c.b.len, c.b.bias));
    // The nearest pair of endpoints is a shared point, to sub-pixel precision.
    const gap = Math.min(dist(a0, b0), dist(a0, b1), dist(a1, b0), dist(a1, b1));
    expect(gap).toBeLessThan(0.01);
    // …and the short arm really is short (Feather's proportions), which is the
    // difference between a check and an X.
    expect(c.b.len).toBeLessThan(c.a.len * 0.6);
  });

  it("the ring's arc is a fraction of its circle, and 100% is the whole circle", () => {
    expect(pose.ring(100).b.len).toBeCloseTo(CIRC, 6);
    expect(pose.ring(25).b.len).toBeCloseTo(CIRC / 4, 6);
    expect(pose.ring(50).a.len).toBe(0);   // the shared bar steps aside while the arc runs
  });

  it("the arc runs on the enclosure circle, starting at 12 o'clock", () => {
    const b = pose.ring(25).b;
    const [start] = ends(strokeD(b.P, b.phi, b.k, b.len, b.bias));
    expect(start[0]).toBeCloseTo(14, 3);
    expect(start[1]).toBeCloseTo(14 - R_RING, 3);
    expect(1 / b.k).toBeCloseTo(R_RING, 6);
  });

  it("a resting plus and minus sit inside the circle, centred on it", () => {
    for (const p of [pose.plus(), pose.check(), pose.fail()]) {
      for (const s of [p.a, p.b]) {
        for (const pt of ends(strokeD(s.P, s.phi, s.k, s.len, s.bias))) {
          expect(dist(pt, [14, 14])).toBeLessThan(R_RING - 1);
        }
      }
    }
  });

  it("the stroke weight and track fade follow the arc, not the shared bar", () => {
    // The arc is stroke B. Reading ringness off A, which never curves, would
    // leave a working ring at resting weight with its track at full strength.
    const host = document.createElement("div");
    const m = new Marks(host);
    m.draw(pose.ring(50));
    expect(m.svg.style.getPropertyValue("--install-ring-w")).toBe(String(W_RING));
    m.draw(pose.minus());
    expect(m.svg.style.getPropertyValue("--install-ring-w")).toBe(String(W_REST));
  });

  it("a round trip through the ring returns byte-identical path data", () => {
    // The invariant behind "the affordance is one element": if plus → ring →
    // plus does not land on the exact same `d`, the tile settles a hair off
    // and every install nudges it further.
    const plus = pose.plus();
    const out = lerpPose(plus, pose.ring(40), 1);
    const back = lerpPose(out, pose.plus(), 1);
    const draw = (p: typeof plus) =>
      strokeD(p.a.P, p.a.phi, p.a.k, p.a.len, p.a.bias) + "|" +
      strokeD(p.b.P, p.b.phi, p.b.k, p.b.len, p.b.bias);
    expect(draw(back)).toBe(draw(plus));
  });
});

describe("the paced floor", () => {
  it("rises, decelerates, and never arrives", () => {
    const samples = [0, 250, 1000, 4000, 20000, 120000].map(floorAt);
    for (let i = 1; i < samples.length; i++) expect(samples[i]).toBeGreaterThan(samples[i - 1]);
    expect(samples[samples.length - 1]).toBeLessThan(92);
    // Decelerating: the first second gains more than the tenth.
    expect(floorAt(1000) - floorAt(0)).toBeGreaterThan(floorAt(10000) - floorAt(9000));
  });

  it("does not rush ahead early — the failure that made TAU_MS 4000", () => {
    // At 900ms into a download whose truth is 15%, a floor reading 52% then
    // visibly stalling is the single most distrusted progress behaviour.
    expect(floorAt(900)).toBeLessThan(25);
  });
});

describe("the arc is the number", () => {
  // The bug: the arc was max(real, floor) while the hovered percentage was the
  // real number alone, so a ring a third of the way round sat beside a tooltip
  // saying 15%. Whatever the caption says, the arc must be showing it.
  it("ignores the floor once a real measurement exists", () => {
    expect(arcValue(4, 15, true, 900)).toBe(15);
    // …which is the point: unretired, the floor would have won here.
    expect(floorAt(900)).toBeGreaterThan(15);
  });

  it("keeps pacing while nothing has ever been measured", () => {
    expect(arcValue(4, null, false, 2000)).toBeCloseTo(floorAt(2000), 6);
  });

  it("does not fall back to the floor on a gap in the feed", () => {
    // A feed that reports 40 and then a null frame must hold at 40, not jump
    // to whatever the clock has reached by then.
    expect(arcValue(40, null, true, 20000)).toBe(40);
  });

  it("never runs backwards, and never closes the ring on its own", () => {
    expect(arcValue(40, 30, true, 5000)).toBe(40);   // late-reporting feed
    expect(arcValue(4, 100, true, 100)).toBe(99);    // only the work may close it
  });
});

describe("a hidden window", () => {
  // WebKit sends no animation frames to an occluded window. A remove pressed
  // there must still happen, and the run must still end.
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("starts the work at once and finishes without a single frame", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "performance"] });
    vi.stubGlobal("requestAnimationFrame", () => 0);
    const host = document.createElement("div");
    const ring = new InstallRing(host, "minus");
    const work = vi.fn(async () => true);
    const labels = {
      busy: () => "", caption: () => undefined, rest: "", failedCaption: "", failedTooltip: "",
    };
    let outcome: string | undefined;
    void ring.run({ installing: false, work, labels }).then((o) => { outcome = o; });
    expect(work).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(10_000);
    expect(outcome).toBe("removed");
  });
});
