# Landing shipping pass — 2026-09-18

The source remains `site/index.html` on `site/native-landing`. Documentation illustration studies are paused. This pass prioritizes the three real language routes, scene interaction correctness, and the closing layout.

## Language routes and branding

English is `/`; Simplified and Traditional Chinese are `/zh-hans/` and `/zh-hant/`. The Chinese HTML files contain translated copy before JavaScript runs and share assets through `<base href="/">`. After editing the source HTML or translation catalog, run `node scripts/generate-landing-locales.mjs`; `--check` rejects stale output. Native moss watch serves changes but does not invoke this generator. The language navigation uses the compact `EN / 繁 / 简` labels, with full accessible language names.

The favicon uses the current moss mark with tighter framing and embedded `prefers-color-scheme` rules. The browser controls the tab icon's outer size; reducing SVG padding increases the visible mark within it. Documentation navigation uses the slightly enlarged generic site-logo size, while GitHub footer styling stays in the site theme.

## Closing film

The six selected films retain their order, timings, silent monochrome grade, and dissolves. `scripts/build-closing-film.py` removes source borders and fills 16:9 without adding pillarboxes; viewport cover cropping handles other aspect ratios. `site/scene5-loop/selection.json` records source crops, timecodes, and provenance. The original downloads remain outside the repository.

The user selected kora and guqin construction for the next footage search. Existing performance excerpts and two instrument-making source leads are available at `/scene5-loop/candidates/`. The new making films have not been cut or incorporated: an exact excerpt and reuse basis remain to be established. No outreach was sent.

## Native validation and publishing

Run `cargo build -p moss-cli`, generate the locale entries, then `target/debug/moss-cli build site --serve --watch`. Validate the printed URL with `node scripts/check-site-preview.mjs <url>`; the checker follows each HTML document's asset base and covers all three landing routes, linked documentation, favicons, CSS, JavaScript, and images.

Production `mosspub.com` is the existing moss-hosted `landing` site. The owning checkout at `moss/site` holds its established identity and deployment state. Publishing a tested worktree output can use the native CLI's `deploy <owning-site> --prebuilt=<absolute-output>` path without switching the main checkout or rebuilding different sources. Prebuilt deployment updates the live generation and deployment record but does not create source history; `history --restore` is therefore not an immediate server rollback. Keep a known prior output when shipping through this path.

## Release readiness corrections

The intro stays at the document top until user input arms the scroll spring. Its title fade and animation expansion end at scene 1’s reading position; the stage then shares scene 2’s viewport anchor. Scene 1 owns a full viewport of space, preventing scene 2 from entering its resting position. The final scroll spring follows the geometric closing threshold, so a slow visual wash cannot delay the glide to the document bottom; new upward input cancels the glide.

Background print refreshes reuse existing neighbour prints instead of switching the visible stage’s scene and width. Scene 4’s transition guard is installed before its layout changes, and orbit startup/exit callbacks are invalidated on reversal. Repeated default-mode scene 3↔4 checks held a single scene attribute, width, and publish transform during each rest.

The linked editor, theme, media, and plugin introductions are task-focused in all three languages. The editor instructions are checked against the desktop implementation, including saved templates, scoped Versions, and filename versus title-property editing. The reusable harvested-interface bank is documented in `site/ui/README.md` and `site/ui/capture-manifest.json`; `scripts/capture-editor-guide-assets.mjs` refreshes its documented static views. Workflows absent from the harvested fixture are described in text rather than simulated.

The Zhuda snapshot comes from its restored canonical source, with all seven distinct homepage images. Never rebuild the live artist site from the older recording fixture.

## Scratch release review

The first remote review target is the standing disposable `scratch-pad.mosspub.com` site on seta. Production and Git pushes remain on hold until the scratch version has been reviewed. Its server already supplies `X-Robots-Tag: noindex, nofollow` and gzip; use those existing deployment features rather than rewriting the published HTML after a build.

The later scroll correction gives the intro its own resting position at zero while retaining scene 1 as the visible animation. The closing handoff now preserves carry velocity until the crossfade is strictly positive, eliminating a zero-progress seam that stalled on large screens. Real wheel checks covered 1440×900 and 1920×1200, including upward return and arrival at the final bottom.

Scene 1 uses separate 640px-long-edge, quality-70 JPEG derivatives in `site/blake/plates` and `site/zhuda/plates`; the captured homepages retain their original assets. With scene-3 sketch/poster/video deferred, a cold local 1440px startup measurement fell from 55 requests / 8,508,566 response bytes to 46 / 6,476,939. These are local uncompressed response-body measurements, not a claim about mobile or remote delivery time. Measure compressed wire transfer on the scratch site as well.

The three static landing documents include absolute production canonical and reciprocal language links, meaningful headings/copy, and ordinary links without requiring JavaScript. The no-script presentation remains readable. The docs demo is a site-level Web Component wrapping the harvested editor with a cancellable player, reader-controlled exploration, localized controls, and a static reduced-motion state. Component CSS is scoped so loading it cannot change the surrounding documentation layout.

Thermo review of this diff found no new structural blocker. Existing neighbour prints are reused, invariant art geometry is cached, and the component shares one implementation between its inline and standalone entry points. The inherited large landing lifecycle module remains a future refactoring concern; splitting it during this visual correction pass would relocate its shared state rather than simplify it.

Scratch publication completed through the native moss CLI using the prepared `scratch-d3189d0` output and the restored `scratch-pad` deployment owner. The publish reported 380 uploads and 26 removals. The public checker passed all 22 required pages and 23 assets at `https://scratch-pad.mosspub.com/`; production and Git remotes were not changed.

Live smoke checks confirmed HTTP 200, Zstandard compression at the delivery edge, `X-Robots-Tag: noindex, nofollow`, all three static language metadata sets, and working inline demo Play/Pause and interaction pause. One cold-browser Chromium run at 1440×900 on an unthrottled local network measured 232ms FCP/LCP. This is a lab observation, not a mobile or field guarantee; resource timing totals exclude the main document and should not be presented as a complete wire-byte budget.

## Scratch animation regression

The deployed page exposed a build defect that native preview masked: shipping stripped the name of `data-moss-preview=""` but left `=""`, producing `<body="">` in harvested HTML. Browser error recovery displayed the page, but the malformed element could not be serialized into the SVG image used by the watercolor renderer. Scene 2's missing print kept `primed()` false; the idle warmer retried every second by changing the live stage, causing both the flicker and blocked scene transitions. The native preview bridge repaired the body marker during serving, so checking only its wrapped responses missed the invalid shipped bytes.

The regression check in `scripts/check-landing-transitions.mjs` must run against plain compiled files or the deployed scratch URL. It verifies successful capture of all four scene prints, an unchanged idle stage, and visible forward/reverse watercolor joins in Chromium and WebKit. Page/asset status checks alone do not establish animation readiness. Mobile touch behavior has a separate check in `scripts/check-landing-mobile.mjs`: the artwork is visual-only, touch scrolling stays native, and closing progress follows forward/reverse scrolling without JavaScript repositioning.

The fixed ship transform preserves attribute boundaries, removes quoted/unquoted values completely, and increments `SHIP_TRANSFORM_REV` to invalidate cached transformed hashes. Its regression fails with the original stripper and passes with the corrected implementation. A failed neighbouring print now selects the existing cut fallback instead of retrying live scene swaps indefinitely. Plain compiled output passed all three languages in Chromium and WebKit, including stable idle scenes, four forward/reverse watercolor joins and the intentionally non-dissolving Publish return. Native mobile touch tests passed with no programmatic scroll writes; the original page fails that check because touch arms scroll snapping. The corrected native watcher was restarted with the rebuilt CLI.
