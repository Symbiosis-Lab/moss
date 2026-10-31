# @symbiosis-lab/moss-blueprint-grid

A square grid that is never drawn in full. Twelve particles drift, swirl around one another and swing around the pointer or a finger; a line shows only where a particle crosses it, brightest at the crossing and fading along the line. The grid itself never moves, so an empty area reads as ground without drawing attention. moss uses it for its waiting states and for an image the preview cannot show yet.

## Two ways in

**A page with no script of its own** loads one file, `dist/blueprint-grid.iife.js`, and places the element:

```html
<blueprint-grid color="#c9b48f" max-alpha=".2"></blueprint-grid>
<script src="blueprint-grid.iife.js"></script>
```

The element fills its own box, which the page sizes and places. A background for the whole page:

```css
blueprint-grid { position: fixed; inset: 0; z-index: -1; }
```

It follows the pointer or a finger anywhere in the window, so it still answers from behind the content. `color` is any CSS colour; without it the element reads its `--blueprint-grid-color` custom property, then falls back to blueprint blue. `max-alpha` is the brightest a crossing gets (default 0.25).

**Code that owns its own canvas** calls the function:

```ts
import { startBlueprintGrid } from "@symbiosis-lab/moss-blueprint-grid";

const stop = startBlueprintGrid(container, { canvas, color: () => "#c9b48f" });
// later
stop();
```

The canvas defaults to the container's `#blueprint-canvas`, and pointer and touch movement is read on the container unless `pointerTarget` names another target. Without `color`, the lines are blueprint blue, lighter when the root element has `data-theme="dark"`.

## Behaviour worth knowing

- Touch: pointer events stop once a touch turns into a scroll, so touches are read too, through passive listeners that leave scrolling alone.
- Reduced motion: when the reader asks for it, the grid paints one still frame per size and runs no animation loop.
- `GRID_SPACING` (40px) is exported, so a page can align its own marks to a real grid line.

## Build and test

```sh
pnpm build   # dist/: ES modules with types, plus the standalone script
pnpm test
```

`dist/` is committed, so a project can depend on this package straight from the repository.
