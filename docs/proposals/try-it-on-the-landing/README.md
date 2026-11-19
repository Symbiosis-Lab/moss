# Try moss on the landing page, with nothing to install

_Status:_ In progress

A visitor on mosspub.com drops a note or a picture, or on a phone taps a button, and sees it as a moss page in place, drawn by moss's own renderer running in the browser. Nothing is uploaded. When they are done they take the folder with them, as a zip or saved straight into a folder, and open it in moss.

## What the visitor does

- **Where it sits.** On the current landing a "Try it here" button in the first scene opens the block in a dialog. Nothing placed after the pinned columns can be reached on a desktop, because the close is a fixed layer that is fully up once the last scene has passed; a dialog sits above it at every width. The block itself does not know its host (`mount(root)` on any `[data-try-it]`), so a redesign that gives it a place in the scroll mounts it there.
- **First sight.** The block shows a folder named "My site" with one page, `index.md`, and that page as moss draws it. This first page is rendered when the site is built. The landing's first load carries none of the block: its stylesheet and script load when the dialog is first opened, and the renderer on the first press or drop inside it.
- **A computer.** Drop a `.md` note or a picture anywhere on the block, or press "Add a note or picture". A note becomes a page of the folder and opens; a note with the name of a page already there replaces it. A picture is copied into the folder and embedded at the end of the open page, the way the moss editor embeds a dropped picture (`![](name)`, the path written by the engine's own `relative_destination`).
- **A phone.** There is nothing to drag from, so the first button is "Choose a picture", which asks only for pictures so the phone offers its camera and photo library. "Add a sample picture" works with no files at all.
- **Writing.** "Add a page" adds `Untitled.md` and opens it for typing; "Write" and "Preview" switch the open page between its text and its page. A link between pages opens the page it points to.
- **Taking it away.** "Download folder" gives a zip of the folder; where the browser can write to a folder the visitor picks (Chromium on a computer), "Save to a folder" writes it there. "Download moss" sits beside them.

## Where the browser stops, and why

What runs in the browser is moss-core, through the `moss-bindings` WebAssembly module that the Obsidian plugin already uses: the page parser, the reference resolver, the renderer and moss's page stylesheet, kept equal to the build's by a test. So each page's article (its title, headings, emphasis, links, embeds and pictures) is what `moss build` writes for the same file. The site bundle carries the folder's name (`root_name`), so the home page is titled after the folder as the build titles it.

What does not run is moss-build: the site's navigation and footer, the home page's list of pages, responsive pictures (`<picture>` with WebP, widths and blurred placeholders), feeds and the theme. moss-build needs threads, processes, the network, file watching and libwebp's C code, none of which compile to `wasm32-unknown-unknown`. Bringing them would mean a second, browser-only build path to keep in step with the real one; drawing them in the widget's own code would be a second renderer that drifts. So the line is the article, and the widget's folder list stands in for the site around it.

Two other limits:

- **HEIC.** moss does not read HEIC. A browser that can decode it (Safari) turns it into a JPEG named the same; anywhere else the visitor is told which pictures moss takes.
- **Nothing is kept between visits.** The folder lives in the page's memory until it is downloaded or saved. Keeping it in the browser's storage would need the renderer on page load for a returning visitor, which the first-load rule forbids.

## What "the same pages" means

The downloaded folder, built with `moss build`, gives the same set of pages, and for each page the same title, the same article text and the same pictures in the same order as the preview. The build adds the navigation, the home page's list of pages, the home's heading for screen readers (which stands in for the site header) and responsive picture sources; the comparison leaves those out. Language tags compare without case, since the build writes `zh-Hans` where the preview writes `zh-hans`. `scripts/check-landing-try-it.mjs` checks this end to end for every row it runs.

## The preview frame

Each page is drawn in an iframe sandboxed with scripts allowed but without its own origin, so whatever the visitor's own files contain runs at an opaque origin and cannot reach mosspub.com. A small script injected into the frame draws the visitor's pictures from files the page posts to it, and hands a click on a link between pages back to the page, which opens that page. A frame that kept the page's origin and had no scripts would let the page listen for clicks inside it in Chromium, but WebKit runs no listener in such a frame.

## How it is built and shipped

- `site/home/try-it/` holds the block: `try-it.js` (the interface), `folder.js` (the zip and folder writer, no DOM, tested in Node), `try-it.css`, and `samples/` (the first page's text in each language, and the sample picture).
- `scripts/build-site-try-it.mjs` compiles `moss-bindings` with wasm-pack into `site/home/try-it/wasm/` and renders each language's first page with it. Both are ignored by git and published by `moss build site`, which does not read `.gitignore`. A built copy is not checked in, because a 3 MB binary rebuilt whenever moss-core changes would add about a megabyte to the public history each time. `predeploy-landing.mjs` runs the script and refuses a deploy it fails.
- The renderer is about 0.9 MB with brotli and is requested on the first press or drop, never before.
