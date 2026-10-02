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

The consumer owns the clock or scroll position; nothing in this package decides how `p` or `t` advances.

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
