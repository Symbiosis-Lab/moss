# @symbiosis-lab/moss-watercolor

A reusable watercolor-wash scene transition: a WebGL2 shallow-water pigment simulation, a recording-and-playback layer that turns captured prints into a three-phase crossfade between any two of them, and a small host utility that paces the simulation's own clock toward a caller-supplied goal. Extracted from a moss landing page, where the engine renders scene-to-scene transitions as paint rather than as a cut or a CSS cross-fade.

## What it is

Each scene is captured as a flat image (a "print"). A transition between two prints runs in three phases of one position `p` in `[0, 1]`:

1. **Out** (`p` up to `playOut`, default 0.42) -- the outgoing print's own recorded dispersion plays forward: the scene breaks up into pigment on a wetted sheet.
2. **Mix** (`playOut` to `playIn`) -- the two dispersions' well-mixed washes crossfade, in optical thickness (Kubelka-Munk absorbance), not in pixel colour.
3. **In** (`playIn` onward, default 0.58) -- the incoming print's own recorded dispersion plays *backward*, so the next scene gathers out of its own wash rather than fading in flat.

A dispersion is recorded once per scene (`sceneSide`/`recordSide`), budgeted across frames so recording never stalls a transition that is already in motion, then played back by position (`playSides`) -- reversible for free, since playback is interpolation over stored frames, not a running simulation. Every scene is recorded over one footprint they all share, so any two scenes' recordings play together, and a transition can turn toward a third scene midway.

## Layers

- **`./engine`** -- `createSim(options)`, the solver and its recording-and-playback methods (`setPrints`, `reset`, `step`, `draw`, `probe`, `setTint`, the scene-side methods `sceneSide`, `recordSide`, `sideRecorded`, `playSides`, `bake`, `freeSide`, and `onContextLost`, `dispose`). Returns `null` without WebGL2 + `EXT_color_buffer_float`. Also exports `DEFAULT_PRESET` (every paper/fluid timing constant the solver reads) and `advance` (the host utility: paces a sim's clock toward a goal, however that goal is produced -- scroll, a timer, a test driving it by hand. This package has no opinion on which).
- **`./paper`** -- `createPaper(options)`, the generated paper texture (tileable value noise: r relief, g absorbency, b fibre, a pore). A fixed default seed makes two calls reproduce the same sheet bit-for-bit.
- **`./model`** -- the three-phase leg as pure functions of `p`, independent of the engine: `phaseOf` (which phase `p` is in), `legAt` (the dissolves and mix `playSides` draws at `p`) and `turnAt` (where a leg turned toward a third scene resumes; see "Transitions that turn"), with `DEFAULT_PHASE_BOUNDS` read from `DEFAULT_PRESET`.
- **`.`** -- re-exports all three.

## Usage

```ts
import { createSim, DEFAULT_PRESET } from "@symbiosis-lab/moss-watercolor/engine";
import { legAt } from "@symbiosis-lab/moss-watercolor/model";

const sim = createSim({
  canvas,
  texW: 820, texH: 780,   // composition pixels before `divisor`
  divisor: 2,              // the simulation grid is texW/divisor x texH/divisor
  rect: () => canvas.getBoundingClientRect(),
});
if (!sim) {
  // no WebGL2 + EXT_color_buffer_float -- fall back to a cut or a CSS cross-fade
}

const water = document.createElement("canvas");   // the footprint: here, water over the whole canvas
water.width = 820; water.height = 780; water.getContext("2d")!.fillRect(0, 0, 820, 780);
const a = sim.sceneSide(outgoingPrintCanvas, water), b = sim.sceneSide(incomingPrintCanvas, water);

// Once per frame: record within the frame's budget (one side at a time, the outgoing one first),
// then draw the position p your own app tracks (scroll, a timeline, a test).
function onFrame(p: number) {
  const pending = [a, b].find((side) => !sim.sideRecorded(side));
  if (pending) sim.recordSide(pending, DEFAULT_PRESET.stepsPerFrame);
  const { ux, uy, k } = legAt(p);
  sim.playSides(a, ux, b, uy, k, p);
}
```

A host that hands off to its own page at `p = 0` and `p = 1` gets the prints themselves there: `playSides` at `p = 0` draws the outgoing side's print and at `p = 1` the incoming side's, with no refraction, absorption change, paper clearing or wash colour. All of those fade in together over `DEFAULT_PRESET.endpointMargin` (0.08) of the leg at each end, through one envelope, `endpointPresence(p, margin)`; mid-leg frames are unchanged. A host stepping the film live with `step` (paced toward a goal by `advance`) and drawing it with `draw(fwd, cure, clearance)` passes its position as a fourth argument to get the same ends. `pnpm --filter @symbiosis-lab/moss-watercolor run test:browser` renders both ends in headless Chromium and compares them with the prints.

The wash is drawn against one page colour, the tint: the paper on a paper ground, and on a transparent ground the colour of whatever the host shows behind the canvas. By default it is the `--bg` custom property on the document root, read once at `createSim`; a host whose colours live elsewhere passes `tint` (a CSS hex colour, `#rgb` or `#rrggbb`, or `[r, g, b]` in sRGB 0 to 1) instead. When the host's background changes, a theme toggle say, it calls `sim.setTint(colour)`. Nothing is polled: the host knows when its theme changes, and the sim would otherwise force a style recalculation every frame to find out. A recording made against another tint counts as unrecorded from then on (`sideRecorded()` is false, `playSides()` draws nothing from it, `recordSide()` starts it again), because its pigment was measured against the old colour. So a host changing the tint mid-leg should hand off to its own page first, as it would for any other interruption: the leg it was playing cannot be finished. `playSides()` returns false when its outgoing side is unrecorded and leaves the canvas untouched (the previous frame stays, since the canvas preserves its drawing buffer), so the host hands off rather than relying on `playSides()`. The voided recordings keep their GPU memory until the caller re-records them or frees them with `freeSide`. A tint with a luma below 0.5 is drawn the other way round: the wash runs on the complement, so the print's light marks are the pigment and clear sheet is the paper, instead of shadow taken out of a light page.

The consumer owns the clock or scroll position; nothing in this package decides how `p` or `t` advances.

## A transparent ground

By default the canvas is drawn over the page colour the host publishes as `--bg`. A host whose page is a live background behind the canvas (an animated grid, say) passes `ground: "transparent"` to `createSim` instead. The canvas is then the print, paper included, and fully opaque at both ends of a leg; within the first 8% of the leg the paper goes and only pigment is left over transparency; the pigment thins smoothly to a share of its thickness (`drainFloor`, default 0.35, in the preset; 0 clears the canvas entirely at the middle of the leg) and the next print thickens back out of it (`drain` in `engine/math.ts`). On a light page the pigment darkens what is behind it, on a dark page it lightens it, both by plain source-over.

One direction serves every print, the tint's. A print darker than a light ground is all darkening pigment and one lighter than a dark ground all lightening pigment, so a light page shown in front of a dark ground becomes a pale film that thins into it rather than vanishing in the first frames. Only marks on the far side of the tint, such as a page background whiter than a near-white ground, carry no pigment, and show the ground from the end of the endpoint margin. Choosing the direction per print would trade those marks for the ones on the other side, and drawing both would double the recorded state, so neither is done.

A leg whose incoming print is not on hand yet can end on nothing: `playSides(a, ux, null, 0, 0, p, { toNothing: true })`. The outgoing print is exact at `p = 0`, its pigment is fully drained by the middle of the leg (whatever `drainFloor` is) and stays so to `p = 1`, and no incoming print is drawn, so the host can fade its own incoming page in over the clear canvas. It is ignored on a paper ground. If an incoming side is passed, it is drained away unseen.

## Transitions that turn

Because every side is recorded over the same footprint, any two of them mix, so a host whose transitions change destination midway can turn a leg toward any other recorded scene with no change on screen.

`legAt(p, bounds)` in `./model` gives the `ux`, `uy` and `k` a leg draws at `p`, and `turnAt(p, bounds)` turns it toward a third scene with no change on screen. Before the mix, the incoming side is swapped for the new scene; while the incoming scene gathers, it becomes the outgoing side at the matching dissolve; and while the two washes mix, `bake(a, ux, b, uy, k, p)` keeps the mix on screen as one side, which plays as the outgoing side toward the new scene from the `p` `turnAt` returns. A baked side keeps the envelope it was baked at, so the frame holds on either ground and with any endpoint margin up to the middle of the leg; the bounds must be symmetric. Free it with `freeSide` once the leg is done. `sceneSide` takes `dose` (how strongly the print's colour is charged), `rinse` and `light` (the flush per step once the wash is mixed, while its mean pigment is above `light`: a rinse of 0 keeps a dark wash whose colour is the point, and a light of 0 flushes a wash off the sheet), and a `look` that shapes how the ink lets go: `stir` and `lift` (how much the film is stirred and how fast the print lets go, as scales of the recording's schedule), `splash` and `mix` (0 for paint laid flat on still water rather than splashed and stirred to one colour), `flow` and `flowSize` (slow eddies with no direction), `bloom` and `reach` (dense colour running apart faster than thin), `diffuse`, `dilute` and `diluteFrom` (thinning into the water's depth), and `liftRamp`. Prints are uploaded once and kept by identity, up to ten; past that the oldest goes (any but the two last recorded against), and one dropped is uploaded again the next time a side plays it. Each side's recording costs GPU memory until it is freed (see "Memory cost").

The browser test (`pnpm --filter @symbiosis-lab/moss-watercolor run test:browser`) renders these in real WebGL, which the vitest suite cannot: run it after changing the engine. It checks both ends of a leg against their prints on light, dark and fully saturated page colours and on a transparent ground, that mid-leg frames hold wash, that a transparent ground composited over the page matches the paper ground and drains as `drainFloor` says, a leg to nothing, and a theme flip before and during a recording. For turning legs it checks both ends against their own prints, a recording interrupted by another against an uninterrupted one, a bake against the mix it was baked from, that every look field reaches the recording, a print dropped from the cache and drawn again, a freed side, and a side voided by a new page colour.

## Requirements

WebGL2 with the `EXT_color_buffer_float` extension (needed for the half-float render targets the solver and the recorded frames use). `createSim` returns `null` when either is unavailable, rather than throwing -- check for `null` and fall back.

## Memory cost

A kept recorded frame is two RGBA16F textures (liquid+depth, deposit+dissolved-fraction) at the simulation grid's own resolution, 16 bytes per texel. With `DEFAULT_PRESET` (`recDuration` 1.7s, `recEvery` 8 steps, `dt` 1/120s -- 27 frames kept per side) and a 410x390 grid (the landing's own desktop default: 820x780 composition pixels over a divisor of 2), one side's recording is about 27 x 410 x 390 x 16 bytes ~= 66 MiB, so a leg between two recorded scenes holds about 132 MiB. A smaller grid or a shorter `recDuration`/coarser `recEvery` scales this down directly -- the landing itself uses a divisor of 4 (about a quarter the grid area) on narrow viewports for this reason. `recordSide`/`freeSide` are explicit, so a caller is expected to free a side once no transition it takes part in is reachable, rather than keep every scene recorded at once.

## Physics credit

The shallow-water film (depth, velocity, fibre saturation), paper relief and the rim current at a drying edge follow Curtis, Anderson, Seims, Fleischer and Salesin, ["Computer-Generated Watercolor"](https://grail.cs.washington.edu/projects/watercolor/paper_small.pdf) (SIGGRAPH 1997). The shaders and the recording/playback scheme are an original implementation of that model, not a port of any other codebase's shader source.

## Build output

`dist/` is committed because the desktop app installs this package from a git dependency (`github:...#path:/packages/moss-watercolor`), which installs without running a build -- the same reason `packages/moss-syntax` commits its `dist/`. So any change under `src/` (or to `tsdown.config.ts`/`tsconfig.json`) needs `pnpm --filter @symbiosis-lab/moss-watercolor run build` run and its output staged in the same commit; a local pre-commit hook (`.githooks/pre-commit`, `scripts/check-dist-freshness.mjs`) checks this and refuses a commit that skips it.

## What stayed out, and why

Two things the landing does around this engine were deliberately left in the landing rather than generalized here:

- **Turning a live scene into a print.** The landing rasters a whole DOM subtree (its editor/preview iframes included) through a cloned, CSS-inlined `<foreignObject>` SVG, with its own rules about what to inline, what to drop and what counts as "on screen" for a lazy-loaded image. That is a moss-preview-specific document pipeline, not a generic image/canvas/video painter -- there is nothing here resembling a small, swappable "capture one element" registry to extract.
- **Hiding the live scene while its print is shown.** The landing covers the whole live page with one CSS class and custom property (`stage.classList.add('morphing')`, `--wash-cover`) for the duration of a transition and clears it afterward -- already two lines, tied to the landing's own stylesheet and DOM (`stage`, `cell`), and not a per-element membership mechanism worth a module of its own.

Both are callable from a host app the same way the landing calls them; they just are not part of this package's contract.
