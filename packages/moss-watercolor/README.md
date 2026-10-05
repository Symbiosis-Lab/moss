# @symbiosis-lab/moss-watercolor

A reusable watercolor-wash scene transition: a WebGL2 shallow-water pigment simulation, a recording-and-playback layer that turns two captured prints into a three-phase crossfade, and a small host utility that paces the simulation's own clock toward a caller-supplied goal. Extracted from a moss landing page, where the engine renders scene-to-scene transitions as paint rather than as a cut or a CSS cross-fade.

## What it is

Each scene is captured as a flat image (a "print"). A transition between two prints runs in three phases of one position `p` in `[0, 1]`:

1. **Out** (`p` up to `playOut`, default 0.42) -- the outgoing print's own recorded dispersion plays forward: the scene breaks up into pigment on a wetted sheet.
2. **Mix** (`playOut` to `playIn`) -- the two dispersions' well-mixed washes crossfade, in optical thickness (Kubelka-Munk absorbance), not in pixel colour.
3. **In** (`playIn` onward, default 0.58) -- the incoming print's own recorded dispersion plays *backward*, so the next scene gathers out of its own wash rather than fading in flat.

A dispersion is recorded once per print (`pair`/`record`), budgeted across frames so recording never stalls a transition that is already in motion, then played back by position (`play`) -- reversible for free, since playback is interpolation over stored frames, not a running simulation.

## Layers

- **`./engine`** -- `createSim(options)`, the solver and its recording-and-playback methods (`setPrints`, `reset`, `step`, `draw`, `probe`, `pair`, `record`, `recorded`, `show`, `free`, `play`, `onContextLost`, `dispose`). Returns `null` without WebGL2 + `EXT_color_buffer_float`. Also exports `DEFAULT_PRESET` (every paper/fluid timing constant the solver reads) and `advance` (the host utility: paces a sim's clock toward a goal, however that goal is produced -- scroll, a timer, a test driving it by hand. This package has no opinion on which).
- **`./paper`** -- `createPaper(options)`, the generated paper texture (tileable value noise: r relief, g absorbency, b fibre, a pore). A fixed default seed makes two calls reproduce the same sheet bit-for-bit.
- **`./model`** -- `phaseOf` and `retarget`, the three-phase leg as pure functions of `p`, independent of the engine, with `DEFAULT_PHASE_BOUNDS` read from `DEFAULT_PRESET`. `retarget(from, to, p, next)` redirects an in-flight transition without a visual jump, in the terms the engine's `play()` draws it (outgoing recording at `p / playOut`, incoming recording at `(1 - p) / (1 - playIn)`, blended by `smooth(playOut, playIn, p)`): in the out phase, and in the first half of the mix, it keeps `from` and swaps the destination; from the second half of the mix on, and in the in phase, the old destination is what is on screen, so it becomes the new source at the matching position. Retargeting back to `from` returns the reverse leg, `{ from: to, to: from, p }` with `p` mapped through the bounds so the same picture stays on screen.
- **`.`** -- re-exports all three.

## Usage

```ts
import { createSim, DEFAULT_PRESET, advance, type ClockState } from "@symbiosis-lab/moss-watercolor/engine";

const sim = createSim({
  canvas,
  texW: 820, texH: 780,   // composition pixels before `divisor`
  divisor: 2,              // the simulation grid is texW/divisor x texH/divisor
  rect: () => canvas.getBoundingClientRect(),
});
if (!sim) {
  // no WebGL2 + EXT_color_buffer_float -- fall back to a cut or a CSS cross-fade
}

const pair = sim.pair();
const state: ClockState = { t: 0, drawn: -1 };

// Drive the sim's clock from whatever position your own app tracks (scroll,
// a timeline, a test): call `advance` once per frame with the current goal.
function onFrame(goalSeconds: number) {
  advance(sim, state, goalSeconds, /* fwd */ true, 0, 0, 0, (t) => sim.draw(true, 0));
}

// Once a pair is recorded (sim.recorded(pair)), present a position directly:
sim.setPrints(outgoingPrintCanvas, incomingPrintCanvas);
sim.show(pair, true, 0.5); // records up to one frame's budget, then plays p=0.5
```

A host that hands off to its own page at `p = 0` and `p = 1` gets the prints themselves there: `show(pair, fwd, 0)` draws the outgoing print and `show(pair, fwd, 1)` the incoming one, with no refraction, absorption change, paper clearing or wash colour. All of those fade in together over `DEFAULT_PRESET.endpointMargin` (0.08) of the leg at each end, through one envelope, `endpointPresence(p, margin)`; mid-leg frames are unchanged. A host drawing with `draw(fwd, cure, clearance)` passes its position as a fourth argument to get the same ends. `pnpm --filter @symbiosis-lab/moss-watercolor run test:browser` renders both ends in headless Chromium and compares them with the prints.

The wash is drawn against one page colour, the tint: the paper on a paper ground, and on a transparent ground the colour of whatever the host shows behind the canvas. By default it is the `--bg` custom property on the document root, read once at `createSim`; a host whose colours live elsewhere passes `tint` (a CSS hex colour, `#rgb` or `#rrggbb`, or `[r, g, b]` in sRGB 0 to 1) instead. When the host's background changes, a theme toggle say, it calls `sim.setTint(colour)`. Nothing is polled: the host knows when its theme changes, and the sim would otherwise force a style recalculation every frame to find out. A recording made against another tint counts as unrecorded from then on (`recorded()` is false, `play()` draws nothing from it, `record()` starts it again), because its pigment was measured against the old colour. So a host changing the tint mid-leg should hand off to its own page first, as it would for any other interruption: the leg it was playing cannot be finished. `play()` returns false when a side is unrecorded and leaves the canvas untouched (the previous frame stays, since the canvas preserves its drawing buffer), so the host hands off rather than relying on `play()`. The voided recordings keep their GPU memory until the caller re-records them or `free`s them. A tint with a luma below 0.5 is drawn the other way round: the wash runs on the complement, so the print's light marks are the pigment and clear sheet is the paper, instead of shadow taken out of a light page.

The consumer owns the clock or scroll position; nothing in this package decides how `p` or `t` advances.

## A transparent ground

By default the canvas is drawn over the page colour the host publishes as `--bg`. A host whose page is a live background behind the canvas (an animated grid, say) passes `ground: "transparent"` to `createSim` instead. The canvas is then the print, paper included, and fully opaque at both ends of a leg; within the first 8% of the leg the paper goes and only pigment is left over transparency; the pigment thins smoothly to a share of its thickness (`drainFloor`, default 0.35, in the preset; 0 clears the canvas entirely at the middle of the leg) and the next print thickens back out of it (`drain` in `engine/math.ts`). On a light page the pigment darkens what is behind it, on a dark page it lightens it, both by plain source-over.

One direction serves every print, the tint's. A print darker than a light ground is all darkening pigment and one lighter than a dark ground all lightening pigment, so a light page shown in front of a dark ground becomes a pale film that thins into it rather than vanishing in the first frames. Only marks on the far side of the tint, such as a page background whiter than a near-white ground, carry no pigment, and show the ground from the end of the endpoint margin. Choosing the direction per print would trade those marks for the ones on the other side, and drawing both would double the recorded state, so neither is done.

A leg whose incoming print is not on hand yet can end on nothing: compose a pair whose incoming side was never recorded (`sim.pair({ only: true }).sides[1]`) and set `toNothing: true` on it. The outgoing print is exact at `p = 0`, its pigment is fully drained by the middle of the leg (whatever `drainFloor` is) and stays so to `p = 1`, and none of the incoming print is drawn, so the host can fade its own incoming page in over the clear canvas. It is ignored on a paper ground. If the incoming side was recorded, it is drained away unseen.

## Requirements

WebGL2 with the `EXT_color_buffer_float` extension (needed for the half-float render targets the solver and the recorded frames use). `createSim` returns `null` when either is unavailable, rather than throwing -- check for `null` and fall back.

## Memory cost

A kept recorded frame is two RGBA16F textures (liquid+depth, deposit+dissolved-fraction) at the simulation grid's own resolution, 16 bytes per texel. With `DEFAULT_PRESET` (`recDuration` 1.7s, `recEvery` 8 steps, `dt` 1/120s -- 27 frames kept per side) and a 410x390 grid (the landing's own desktop default: 820x780 composition pixels over a divisor of 2), one side's recording is about 27 x 410 x 390 x 16 bytes ~= 66 MiB, and a `pair()` (two sides) is about 132 MiB. A smaller grid or a shorter `recDuration`/coarser `recEvery` scales this down directly -- the landing itself uses a divisor of 4 (about a quarter the grid area) on narrow viewports for this reason. `pair`/`record`/`free` are explicit, so a caller is expected to free a pair once its transition is no longer reachable rather than keep every boundary recorded at once.

## Physics credit

The shallow-water film (depth, velocity, fibre saturation), paper relief and the rim current at a drying edge follow Curtis, Anderson, Seims, Fleischer and Salesin, ["Computer-Generated Watercolor"](https://grail.cs.washington.edu/projects/watercolor/paper_small.pdf) (SIGGRAPH 1997). The shaders and the recording/playback scheme are an original implementation of that model, not a port of any other codebase's shader source.

## Build output

`dist/` is committed because the desktop app installs this package from a git dependency (`github:...#path:/packages/moss-watercolor`), which installs without running a build -- the same reason `packages/moss-syntax` commits its `dist/`. So any change under `src/` (or to `tsdown.config.ts`/`tsconfig.json`) needs `pnpm --filter @symbiosis-lab/moss-watercolor run build` run and its output staged in the same commit; a local pre-commit hook (`.githooks/pre-commit`, `scripts/check-dist-freshness.mjs`) checks this and refuses a commit that skips it.

## What stayed out, and why

Two things the landing does around this engine were deliberately left in the landing rather than generalized here:

- **Turning a live scene into a print.** The landing rasters a whole DOM subtree (its editor/preview iframes included) through a cloned, CSS-inlined `<foreignObject>` SVG, with its own rules about what to inline, what to drop and what counts as "on screen" for a lazy-loaded image. That is a moss-preview-specific document pipeline, not a generic image/canvas/video painter -- there is nothing here resembling a small, swappable "capture one element" registry to extract.
- **Hiding the live scene while its print is shown.** The landing covers the whole live page with one CSS class and custom property (`stage.classList.add('morphing')`, `--wash-cover`) for the duration of a transition and clears it afterward -- already two lines, tied to the landing's own stylesheet and DOM (`stage`, `cell`), and not a per-element membership mechanism worth a module of its own.

Both are callable from a host app the same way the landing calls them; they just are not part of this package's contract.
