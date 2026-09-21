# The stage: one moss interface per page, many scenes

A documentation page shows the real moss editor once, pinned beside the text. A Play button under a heading runs a *scene* on that one editor — a short scripted demonstration of what the heading describes — and the reader can take over at any moment. There is never a second editor on the page, and the stage has no controls of its own: the text is both the list of scenes and the way to start them.

This directory replaces the previous per-demonstration editor element (one editor per demo, rather than one editor per page).

## Authoring

```html
<moss-stage></moss-stage>              <!-- once per page, after the opening paragraph -->

Click the site root in the breadcrumb to expand the file tree. Click a page to open it. <moss-scene name="tree"></moss-scene>
```

A page may have a stage and no markers: the reader simply gets a working editor to try things in, which is all a page about Markdown syntax needs. A playground page is a list of markers with a sentence each. Neither needs anything this directory does not already have.

A marker always sits inline at the END of the paragraph whose gestures it performs, one per paragraph, and the prose is written for it: the paragraph names the gestures in the order the scene performs them, in the words the UI uses. There is no other form of marker. It is a filled round button the height of the text, centred on the line's middle, carrying ▶ (■ while playing) — prominent enough to be found when skimming, since it is the only control the demonstration has. Its accessible name is the verb plus its heading by reference, and it is described by its own paragraph (`aria-describedby`), so several markers under one heading stay distinguishable. Where a guide used a screenshot or a recording to show a gesture, the sentence and its marker replace it; a static image stays only as the `<noscript>` fallback.

Scenes are for what a reader cannot discover by typing: gestures and features particular to moss. Do not script plain typing.

Scene names are language-independent, so the three locales of a guide share one scene file, and scene files carry no visible text. A marker's button is named for assistive technology by reference to the heading it sits under (`aria-labelledby`), so no heading text is copied.

## Modules

Each module has one reason to change. Nothing imports upward.

| Module | Owns | Knows nothing about |
|---|---|---|
| `player.js` | Running a scene against a driver: the step vocabulary, cancellation, instant (reduced-motion) playback | The host page, iframes, layout, UI strings |
| `driver.js` | Turning the player's verbs into actions on the harvested editor: typing, clicking a named target, moving the pointer overlay | Scenes, markers, when playback starts or stops |
| `moss-stage.js` | The iframe; loading a scene's opening state and playing it when asked; stopping when the reader touches the editor; announcing what it is doing | Where markers are, how the page is laid out, how a verb becomes an action, anything about the harvested document |
| `moss-scene.js` | The marker: its button, asking the stage to play or stop, showing Play or Stop for its own scene | How a scene runs |
| `harvest.js` | Everything known about the harvested `editor.html`: named click targets, its theme attributes, the default fixture, how a fixture becomes its URL | Everything else |
| `strings.js` | The few UI strings per locale: Play, Stop, the frame's accessible title, the load-failure line | Everything else |
| `scenes/<name>.json` | One scene as data | Code |
| `../../.moss/theme/style.css` | Page layout on pages with a stage: the two-column sticky layout and the page width | Component internals |

`player.js` takes its driver as an argument and touches no globals, so the same scene files can later drive Playwright checks and recordings against the real app — one scenario format, several consumers. Keep it importable from Node.

`harvest.js` belongs with the harvest and moves into `moss-desktop/scripts/landing/` when the harvested driver grows a `load(fixture)` method.

## Scene format

```json
{
  "open": "",
  "steps": [
    { "click": "tree.breadcrumb" }
  ]
}
```

A scene's *opening state* is a fresh load of the harvested document — on the default fixture unless the scene names its own `"fixture"` — optionally followed by `open`, text that replaces the document before anything plays. Most scenes omit `open` and play against whatever the fixture itself already contains; name it only when a scene needs the document to start as something other than that. A step is an object with exactly one key, the verb. Verbs live in one table in `player.js`; adding a verb is adding an entry. `click`, `context` (a right-click, which is how the app's own menus open), and `dblclick` take a *named target* from `harvest.js`, never a selector. `typeInto` pairs a named target with text to type into it, for the site's own `<input>` fields (a rename prompt, a properties search box) that aren't the harvested editor's own typing surface — there is no bare `type` verb; see "Do not script plain typing" above.

**A scene is one paragraph's worth: the gestures that paragraph describes, in order, kept to about four gestures and ten seconds.** Two failures bound it. One long run loses the viewer — completion of guided demos falls off a cliff past a handful of steps (roughly 72% for three-step tours against 16% for seven). But a marker after every sentence fragments the reading just as badly: three buttons in one paragraph ask for three decisions where the reader wanted one. So each illustrated paragraph ends with ONE marker, and its scene performs that paragraph; when a paragraph describes more than about four gestures the scene keeps the essential path and leaves the rest to the prose. A task told over several paragraphs is several scenes. A scene that needs the app already somewhere names the scene that gets it there: `"after": "open-versions"`. After the fresh load the player runs that scene — and whatever it comes after, in turn — instantly, with no animation, then animates only this scene's own steps. Instant playback already exists for reduced motion, so composition adds no second mechanism.

Only script a gesture the harvested interface really performs. A section whose gesture the harvest cannot perform gets no marker.

## Who controls the stage

The reader outranks the script, and scrolling is reading: scrolling never changes the stage.

- **The stage is never empty.** When it connects it loads the default fixture, so a reader who never clicks still sees a working editor. No marker is involved.
- **Play.** A marker's button asks the stage to play its scene. The stage reloads the editor to the scene's opening state, then plays. Every play starts from a fresh load because a scene may leave the app anywhere — versions mode, an open menu, a different file — and a reload is the only reset that is true for all of them; it costs about a fifth of a second. While that scene plays, that marker's button reads Stop; every other marker reads Play.
- **Stop.** Pressing Stop, or any pointer or key press inside the editor, stops playback at once and keeps what is there. Play always starts again from the opening state — a scene is a few seconds, and resuming mid-scene would need every verb to know how to resume (clicking a toggle twice undoes it).
- **One load at a time.** Every play request starts a new load and aborts the one before it; a single `AbortController` per load both stops its playback and makes any of its still-pending steps a no-op, including a pending iframe navigation. There is no second mechanism for "this was superseded".
- **Failure.** If a scene cannot be loaded, its marker goes back to Play and shows one short line saying so; pressing Play tries again.
- With `prefers-reduced-motion`, playing a scene applies every step instantly and shows its final state.

The two elements talk only through events on `document`: a marker dispatches `moss-scene-play` / `moss-scene-stop` with its scene name, and the stage dispatches `moss-stage-state` with `{ name, phase }`. The stage never looks for markers; a marker never reaches into the stage.

## The pointer

The script's pointer is a small dot, not an arrow: the reader's own arrow is over the same editor, and two arrows leave it unclear which is theirs. A press dips the dot and sends ONE ring outward, which fades; the dot itself never grows, so it never covers the control it is pressing.

A right-click must teach the reader to right-click, and colour cannot: a colour code needs a legend, fails colour-blind readers, and the site's accent green already means Stop. A right-click means "a menu opens here", and the sign people already know for a menu is three stacked dots. The driver only records which button was pressed (`data-button="left|right"` on the pointer at press time); how that looks is CSS alone. A right-click is therefore marked by shape: for the press and the hold after it, the solid dot becomes a hollow dotted circle, a little larger so its dots read as dots, while the same solid ring goes out. Treatments built on colour, a half ring, a mouse glyph, splitting the dot into the three-dot menu sign, and a second dotted ring were tried and rejected as unclear.

## Pace

A demonstration is watched, not caused, so interface-transition timings (100–500 ms) are the wrong reference: the viewer has to find the pointer, follow it, see the press, and take in what appeared. The first build glided in 420 ms and pressed 180 ms later, which is before a viewer's eyes have arrived — re-aiming them takes about 200 ms and a fixation to confirm the target about 200 ms more — and it left no time at all to look at a menu that had just opened. Every beat has a number and a reason, and they live in ONE table in `driver.js`:

| Beat | Value | Why |
|---|---|---|
| Travel | `clamp(500, 450 + 0.5 × distance_px, 1000)` ms, ease-in-out | Deliberate human pointing takes 0.8–1.5 s; slower than about a second drags. Scaled with distance so short hops are not sluggish. |
| Dwell on the target before pressing | 400 ms; 600 ms before a right-click | Eye re-aim (~200 ms) plus one fixation (~200 ms). A right-click's result is less expected, so it gets longer. |
| Press | dip ~150 ms; the ring fades over ~500 ms during the hold | Click feedback reads at 100–200 ms. |
| Gap between a double-click's two presses | 180 ms | Fast enough to read as one gesture, slow enough for each press's own dip/ring to register on its own — a double click should look like two presses, not one blurred one. |
| Hold after the result appears | `400 + 200 × min(reveals, 5)` ms | About 200 ms to look at each new item, capped at five because a viewer samples a menu rather than reading it. `reveals` is a number on the step (`{ "context": "crumb.file", "reveals": 10 }`), default 2. The last step holds too, so the result is seen before the button returns to Play. |
| Target wait (bound, not a beat) | 4000 ms | Not watched by the viewer, so it is generous rather than paced: a previous step's own effect (a menu opening, a mode swap) may still be rendering when the next step goes looking for its target. |
| `typeInto` per-character cadence | 55 ms | Fixed, not randomized — that realism lives entirely inside the harvested editor's own typing, and a plain `<input>` (a rename prompt, a search box) never needs to look human. |

A two-gesture scene therefore runs about five seconds, not one. No scene may run past WCAG's five-second limit for motion a reader cannot stop without the reader having a Stop control — the marker's own button is that control.

## Pressing Play moves nothing

Starting a scene must not shift the page: not the scroll position, not the prose, not the stage. On the wide layout the stage is already in view, so nothing scrolls; focus goes into the editor with `preventScroll`; a marker's button is a fixed-size box so that swapping ▶ for ■ cannot re-wrap its line; the failure line takes no space until it is shown. Only on the narrow layout, and only when the stage is mostly out of view, does Play scroll it into view, smoothly.

## Fitting the page

On a page with a stage the theme widens the whole page, not just the article: it sets `--moss-site-max-width`, the one variable moss's `.container` reads, so the header's divider, the article, and the footer's divider share one width.

The editor follows the page's theme: when the page's `data-theme` changes, the harvested document's theme attributes change with it, so a dark page never frames a light editor.

The stage's box must show the whole interface the prose describes. The app's bottom toolbar appears only once the editor has focus; when it does it must fit inside the stage.

In the page source the stage comes after the opening paragraph. Source order is what a narrow screen shows, so a phone reader meets a sentence of text before the editor; on a wide screen the theme's grid places the stage in its own column regardless of source order.

## The harvested document

The stage frames `../app-editor.html`: the app's real `editor.html` and `editor-main.ts`, bundled by `moss-desktop/scripts/landing/harvest-ui.mjs` with only the Tauri seams mocked, so opening files, saving, the app's menus and versions mode are the app's own code. A command the mocks do not implement answers "Not available in this demo" rather than pretending. The landing page frames the older hand-assembled `../editor.html`; the two are separate outputs of the same harvest and neither depends on the other.
