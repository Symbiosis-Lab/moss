# The demo: one moss interface per page, many scenes

A documentation page shows one real moss interface once, pinned beside the text. A Play button under a heading runs a *scene* on that interface — a short scripted demonstration of what the heading describes — and the reader can take over at any moment. There is never a second interface on the page, and the demo frame has no controls of its own: the text is both the list of scenes and the way to start them.

This directory replaces the previous per-demonstration editor element (one editor per demo, rather than one editor per page), and its earlier name, `site/ui/stage`: it now covers more than one *surface* — the editor today, others later — so the directory, the elements, and the CSS classes are named for what they are, `demo`, not for the one surface that used to be the only one.

## Add a demo

1. Write the sentence or list item as you normally would, ending it with a plain Markdown link: `[▶](#scene=my-scene)`. `[▶](#scene=)` (an empty name) just opens the interface with no scripted scene — use it for a page that only needs a working sandbox to try things in (see "Markdown markers" below).
2. Record the gestures it performs against the real interface: `node scripts/record-scene.mjs my-scene`. This opens a headed browser on the real surface; click, right-click, double-click, or type-and-press-Enter exactly as the scene should play back, then close the window. It writes `scenes/my-scene.json` and prints the link to paste — if you typed a different name in step 1, use the one the recorder printed instead.
3. Preview the page: the theme upgrades your link into a marker automatically (nothing else to wire up), so building and opening the page is enough to see and play it.
4. `npm run test:demo-player` — the no-browser check (player unit tests plus `check-demo-scenes.mjs`) that catches a missing scene, an unreachable target, a scene that has drifted past the pace budget, or a page that links a scene no other page uses, in seconds, before you ever open a browser.

An interaction the recorder doesn't recognize prints which element it landed on and why — the interface doesn't yet have a named target there. Add a `data-action` to the desktop app's element (or a new entry to the `TARGETS` map in `surfaces/editor.js`) and record again; the recorder never invents a target for you, so a scene never depends on a selector nobody chose on purpose.

## Authoring

Authors write a plain Markdown link, never HTML:

```markdown
Click the site root in the breadcrumb to expand the file tree. Click a page to open it. [▶](#scene=tree)
```

A page may have scene links and never mention the demo frame at all: the theme loader inserts it automatically, once, after the article's first paragraph, the moment it finds any `#scene=` link anywhere on the page. A page with no `#scene=` link anywhere gets no demo frame — a page about, say, publishing that never demonstrates the editor doesn't pay for one. A page that wants a working sandbox but no scripted gesture at all still needs one link to earn its frame: `[▶](#scene=)`, "just open the interface" (see "Who controls the demo" below).

A marker always sits inline at the END of the paragraph or list item whose gestures it performs, one per paragraph or item, and the prose is written for it: the paragraph names the gestures in the order the scene performs them, in the words the UI uses. There is no other form of marker. Once upgraded it is a filled round button the height of the text, centred on the line's middle, carrying ▶ (■ while playing) — prominent enough to be found when skimming, since it is the only control the demonstration has. Its accessible name is the verb plus its heading by reference, and it is described by its own paragraph (`aria-describedby`), so several markers under one heading stay distinguishable. Where a guide used a screenshot or a recording to show a gesture, the sentence and its marker replace it; a static image stays only as the `<noscript>` fallback, written as an ordinary sibling of the link (not nested inside it — a plain Markdown link has no room for a block child).

Without JavaScript a `#scene=` link renders as nothing visible: `demo.css` hides `a[href^="#scene="]` outright, and the only thing that ever un-hides it is the upgrade replacing the anchor with a `<moss-demo-marker>` — there is no second toggle for a script failure to leave stuck. A page's `<noscript>` fallback, if it has one, is unaffected either way.

Scenes are for what a reader cannot discover by typing: gestures and features particular to moss. Do not script plain typing.

Scene names are language-independent, so the three locales of a guide share one scene file, and scene files carry no visible text. A marker's button is named for assistive technology by reference to the heading it sits under (`aria-labelledby`), so no heading text is copied.

## Surfaces

`demo-frame.js` is a base class, `DemoFrameElement`, that owns everything common to showing and driving ONE framed interface regardless of which one: the iframe, one load at a time (a single `AbortController` per load), theme + locale sync, the pointer overlay, playing a scene through `player.js`/`driver.js`, reader input stopping playback at once, the WebKit scroll backstop, and announcing state. It knows nothing about any one surface.

A **surface** is one framed interface: what document it opens, what its named click targets are, how it reports readiness, and how the page's theme reaches it. A surface is two files:

- `surfaces/<name>.js` — the adapter. Exports:
  - `DEFAULT_FIXTURE` — what a scene that names no `"fixture"` opens.
  - `findTarget(doc, name) -> Element | null` — resolves a named target, or `null` if not yet rendered (the timing-tolerant half `driver.js` polls with).
  - `TARGET_NAMES` — every name the adapter knows, read by `scripts/check-demo-scenes.mjs` (a scene step naming a target outside this set fails the static check) and by `scripts/record-scene.mjs` (the reverse lookup a recording session resolves a click against).
  - `setTheme(doc, theme) -> void` — applies the page's light/dark theme to the framed document.
  - `frameUrl(fixture, locale) -> string` — the URL that opens the framed document.
  - `waitForReady(win, doc) -> Promise<handle>` — resolves once the framed document is interactive; its resolved value becomes the frame's `getHandle()`, which `driver.js`'s `setText`/`typeInto` verbs use.
- `moss-<name>-demo.js` — the element: `class extends DemoFrameElement { get surface() { return <the adapter's exports>; } }`, then `customElements.define('moss-<name>-demo', ...)`. That is the whole file — everything else lives in the base or the adapter.

`surfaces/editor.js` is today's only surface (the former `harvest.js`, renamed and moved), and `moss-editor-demo.js` is its element. Adding a second surface — a `<moss-preview-demo>`, say, framing the publish preview instead of the editor — means a new `surfaces/preview.js` and an element file this size; `demo-frame.js` does not change, because it never imported a surface directly. `driver.js` takes `findTarget` as a constructor dependency for the same reason, rather than importing a surface's adapter itself.

**One surface per page today.** The theme's upgrade inserts the frame for whichever surface a page's scenes ask for — but since only one surface exists, it inserts `<moss-editor-demo>` unconditionally rather than fetching every linked scene's JSON first just to ask; `scripts/check-demo-scenes.mjs` is what actually enforces that a page never mixes surfaces across its own markers, so the day a page tries to, the static check catches it before the runtime's shortcut would render the wrong frame. Swapping one frame's surface mid-page ("if a page mixes surfaces, insert one frame and let it swap surface per scene") was left unbuilt: it would mean the frame's own tag no longer picks its surface, which is a real design change to `demo-frame.js`'s contract, not a trivial add — the one-frame-per-surface-per-page shape stays until a second surface actually exists and a real page needs both.

## Modules

Each module has one reason to change. Nothing imports upward.

| Module | Owns | Knows nothing about |
|---|---|---|
| `player.js` | Running a scene against a driver: the step vocabulary, cancellation, instant (reduced-motion) playback | The host page, iframes, layout, UI strings |
| `driver.js` | Turning the player's verbs into actions on a framed surface: typing, clicking a named target, moving the pointer overlay | Scenes, markers, when playback starts or stops, which surface it is driving (target lookup and the ready handle come in as constructor deps) |
| `demo-frame.js` | `DemoFrameElement`, the base every surface element extends: the iframe; loading a scene's opening state and playing it when asked; stopping when the reader touches the interface; announcing what it is doing | Where markers are, how the page is laid out, anything surface-specific (delegated to `get surface()`) |
| `moss-editor-demo.js` | The "editor" surface's element — `DemoFrameElement` + `surfaces/editor.js`, nothing else | How a scene runs, how a target resolves |
| `moss-demo-marker.js` | The marker: its button, asking the frame to play or stop, showing Play or Stop for its own scene | How a scene runs |
| `surfaces/editor.js` | Everything known about the harvested `editor.html`: named click targets, its theme attributes, the default fixture, how a fixture becomes its URL, its ready check | Everything else |
| `strings.js` | The few UI strings per locale: Play, Stop, the frame's accessible title, the load-failure line | Everything else |
| `scenes/<name>.json` | One scene as data | Code |
| `../../.moss/theme/script.js` | Upgrading `a[href^="#scene="]` links into markers, and inserting the one surface element a page's scenes need | How a scene runs, layout |
| `../../.moss/theme/style.css` | Page layout on pages with a demo frame: the two-column sticky layout and the page width | Component internals |

`player.js` takes its driver as an argument and touches no globals, so the same scene files can later drive Playwright checks and recordings against the real app — one scenario format, several consumers (`scripts/check-docs-demo.mjs` and `scripts/record-scene.mjs` both do). Keep it importable from Node.

`surfaces/editor.js` belongs with the harvest and moves into the desktop app repository's `scripts/landing/` once the harvested driver grows a `load(fixture)` method (see "The harvested document" below).

## Markdown markers

A plain link at the end of the paragraph or list item it performs: `[▶](#scene=tree)`; `[▶](#scene=)` opens the frame with no scripted scene. moss's own build leaves a bare `#scene=…` fragment href completely alone — verified by building a scratch page carrying one: the emitted `href` is byte-identical, no broken-link or anchor warning is printed (`crates/moss-build/src/build/manifest/link_audit.rs`'s `candidate_keys` only ever looks at root-relative `/…` hrefs; a fragment-only href returns `None` and is never checked), and the link stays exactly where it was written, inline in its `<p>`/`<li>`. No moss change was needed, so none was made — this is standing behavior, not a convention moss had to be taught.

The theme loader (`../../.moss/theme/script.js`) upgrades every `a[href^="#scene="]` under `article` into a `<moss-demo-marker name="...">` (the part after `#scene=`, percent-decoded, empty for "just open the surface"), and — if the page carries at least one such link — inserts one `<moss-editor-demo>` after the article's first paragraph, once, regardless of which paragraph actually held the link that triggered it. `demo.css` hides an unupgraded link outright (`a[href^="#scene="] { display: none }`), so a no-JS reader never sees a dead ▶; the upgrade removes the hiding simply by replacing the hidden anchor with the marker element, which that CSS rule does not match — there is no second flag to keep in sync with it.

## Scene format

```json
{
  "surface": "editor",
  "open": "",
  "steps": [
    { "click": "tree.breadcrumb" }
  ]
}
```

`"surface"` names which surface's adapter the scene's targets resolve against (`surfaces/<name>.js`); omit it and it defaults to `"editor"`, the only surface today — a scene file is free to leave it out, and every scene in this directory does.

A scene's *opening state* is a fresh load of the framed document — on the default fixture unless the scene names its own `"fixture"` — optionally followed by `open`, text that replaces the document before anything plays. Most scenes omit `open` and play against whatever the fixture itself already contains; name it only when a scene needs the document to start as something other than that. A step is an object with exactly one verb key. Verbs live in one table in `player.js`; adding a verb is adding an entry. `click`, `context` (a right-click, which is how the app's own menus open), and `dblclick` take a *named target* from the scene's surface adapter, never a selector. `typeInto` pairs a named target with text to type into it, for the site's own `<input>` fields (a rename prompt, a properties search box) that aren't the harvested editor's own typing surface — there is no bare `type` verb; see "Do not script plain typing" above.

**Layout of an illustrated section.** A section whose gestures form ONE task (open versions, save one, restore it) is one paragraph ending in one marker. A section that offers several INDEPENDENT tasks (expand the tree, create a page, create a folder, create from a template, collapse the tree) is a short lead-in sentence followed by a bullet list: one task per item, each item ending in its own marker, so the markers line up down the list's right edge instead of hiding mid-paragraph. Explanation that no scene performs (what a folder becomes on the site, how templates are stored) stays in a plain paragraph after the list, without a marker.

**A scene is one paragraph's or one list item's worth: the gestures that paragraph describes, in order, kept to about four gestures and ten seconds.** Two failures bound it. One long run loses the viewer — completion of guided demos falls off a cliff past a handful of steps (roughly 72% for three-step tours against 16% for seven). But a marker after every sentence fragments the reading just as badly: three buttons in one paragraph ask for three decisions where the reader wanted one. So each illustrated paragraph ends with ONE marker, and its scene performs that paragraph; when a paragraph describes more than about four gestures the scene keeps the essential path and leaves the rest to the prose. A task told over several paragraphs is several scenes. A scene that needs the app already somewhere names the scene that gets it there: `"after": "open-versions"`. After the fresh load the player runs that scene — and whatever it comes after, in turn — instantly, with no animation, then animates only this scene's own steps. Instant playback already exists for reduced motion, so composition adds no second mechanism. `scripts/check-demo-scenes.mjs` enforces a slightly wider bound than "about four/ten" (eight steps, fifteen estimated seconds) — a static check can't see a step's real on-screen travel distance, so its duration estimate is necessarily rough, and the six-step "versions" scene (one paragraph describing five real gestures) is exactly the kind of content "about four" was always meant to admit.

Only script a gesture the harvested interface really performs. A section whose gesture the harvest cannot perform gets no marker.

## The static check

`node scripts/check-demo-scenes.mjs` (also `npm run test:demo-player`, alongside `player.js`'s own unit tests) — no browser, done in seconds. It fails when: a page links a scene with no matching file; a scene file no page links; a step names a target its scene's surface adapter doesn't have; a scene's `after` names a scene that doesn't exist, or the `after` graph cycles; a scene exceeds the spec's bound (steps, and a pace-estimated duration built from `driver.js`'s two exported pure functions, `travelDurationMs`/`holdAfterResultMs`); or a page mixes surfaces across its own markers. It is the first line of defense — `scripts/check-docs-demo.mjs` (below) is what actually proves a scene plays correctly in a browser, but this catches the far more common mistakes (a typo'd scene name, an orphaned scene file, a target that doesn't exist) before a browser is ever opened.

## The recorder

`node scripts/record-scene.mjs <name> [--lang en|zh-hant|zh-hans] [--surface editor]` serves `site/ui` locally, opens the surface document in a headed Chromium, and records an author's own clicks, right-clicks, double-clicks, and typed-and-committed input fields, mapping each to a named target by the same reverse lookup `driver.js` itself uses (the surface adapter's `findTarget`, tried against every name in `TARGET_NAMES`). An interaction on an element with no named target is reported, not silently dropped, naming the element so the author can add a `data-action` or a `TARGETS` entry. On window close (or Ctrl-C) it writes `scenes/<name>.json` — `steps` and a default `reveals` (2, `driver.js`'s own default) on every click/context/dblclick step — and prints the link to paste.

`--headless --script <file>` drives the page with Playwright instead of a person, for testing the recorder itself: Playwright's own input simulation dispatches trusted DOM events, the same ones a real click produces, so it exercises the exact listeners unchanged. Verified by writing a driver that reproduces the "tree" scene's own two clicks and diffing the recorded JSON's verb/target sequence against `scenes/tree.json` — they matched exactly (only `reveals` differs, which is expected: the recorder writes the default, a hand-tuned scene often doesn't use it).

## Who controls the demo

The reader outranks the script, and scrolling is reading: scrolling never changes the demo frame.

- **The frame is never empty.** When it connects it loads the default fixture, so a reader who never clicks still sees a working editor. No marker is involved.
- **Play.** A marker's button asks the frame to play its scene. The frame reloads the interface to the scene's opening state, then plays. Every play starts from a fresh load because a scene may leave the app anywhere — versions mode, an open menu, a different file — and a reload is the only reset that is true for all of them; it costs about a fifth of a second. While that scene plays, that marker's button reads Stop; every other marker reads Play. A marker with an empty name (`[▶](#scene=)`) is different: it never reloads or plays anything — it only brings the already-loaded frame into view and focuses it, since there is no scene to run, and so it never reads Stop either.
- **Stop.** Pressing Stop, or any pointer or key press inside the interface, stops playback at once and keeps what is there. Play always starts again from the opening state — a scene is a few seconds, and resuming mid-scene would need every verb to know how to resume (clicking a toggle twice undoes it).
- **One load at a time.** Every play request starts a new load and aborts the one before it; a single `AbortController` per load both stops its playback and makes any of its still-pending steps a no-op, including a pending iframe navigation. There is no second mechanism for "this was superseded".
- **Failure.** If a scene cannot be loaded, its marker goes back to Play and shows one short line saying so; pressing Play tries again.
- With `prefers-reduced-motion`, playing a scene applies every step instantly and shows its final state.

The two elements talk only through events on `document`: a marker dispatches `moss-demo-play` / `moss-demo-stop` with its scene name, and the frame dispatches `moss-demo-state` with `{ name, phase }`. The frame never looks for markers; a marker never reaches into the frame.

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

`travelDurationMs` and `holdAfterResultMs` are the two beats exported as pure functions — no DOM, no timers — so `scripts/check-demo-scenes.mjs` and `scripts/check-docs-demo.mjs` can both pace-bound a scene without a browser.

A two-gesture scene therefore runs about five seconds, not one. No scene may run past WCAG's five-second limit for motion a reader cannot stop without the reader having a Stop control — the marker's own button is that control.

## Pressing Play moves nothing

Starting a scene must not shift the page: not the scroll position, not the prose, not the demo frame. On the wide layout the frame is already in view, so nothing scrolls; focus goes into the editor with `preventScroll`; a marker's button is a fixed-size box so that swapping ▶ for ■ cannot re-wrap its line; the failure line takes no space until it is shown. Only on the narrow layout, and only when the frame is mostly out of view, does Play scroll it into view, smoothly.

## Fitting the page

On a page with a demo frame the theme widens the whole page, not just the article: it sets `--moss-site-max-width`, the one variable moss's `.container` reads, so the header's divider, the article, and the footer's divider share one width.

The editor follows the page's theme: when the page's `data-theme` changes, the harvested document's theme attributes change with it, so a dark page never frames a light editor.

The demo frame's box must show the whole interface the prose describes. The app's bottom toolbar appears only once the editor has focus; when it does it must fit inside the frame.

In the page source the demo frame is inserted after the opening paragraph. Source order is what a narrow screen shows, so a phone reader meets a sentence of text before the editor; on a wide screen the theme's grid places the frame in its own column regardless of source order.

## The harvested document

The "editor" surface frames `../app-editor.html`: the app's real `editor.html` and `editor-main.ts`, bundled by the desktop app's harvest script (`scripts/landing/harvest-ui.mjs`) with only the Tauri seams mocked, so opening files, saving, the app's menus and versions mode are the app's own code. A command the mocks do not implement answers "Not available in this demo" rather than pretending. The landing page frames the older hand-assembled `../editor.html`; the two are separate outputs of the same harvest and neither depends on the other.
