# Landing shipping pass — 2026-09-18

The source remains `site/index.html` on `site/native-landing`. Documentation illustration studies are paused. This pass prioritizes the three real language routes, scene interaction correctness, and the closing layout.

## Language routes and branding

English is `/`; Simplified and Traditional Chinese are `/zh-hans/` and `/zh-hant/`. The Chinese HTML files contain translated copy before JavaScript runs and share assets through `<base href="/">`. After editing the source HTML or translation catalog, run `node scripts/generate-landing-locales.mjs`; `--check` rejects stale output. Native moss watch serves changes but does not invoke this generator. The language navigation follows the documentation's named-link pattern.

The favicon uses the current moss mark with tighter framing and embedded `prefers-color-scheme` rules. The browser controls the tab icon's outer size; reducing SVG padding increases the visible mark within it. Documentation navigation uses the slightly enlarged generic site-logo size, while GitHub footer styling stays in the site theme.

## Closing film

The six selected films retain their order, timings, silent monochrome grade, and dissolves. `scripts/build-closing-film.py` removes source borders and fills 16:9 without adding pillarboxes; viewport cover cropping handles other aspect ratios. `site/scene5-loop/selection.json` records source crops, timecodes, and provenance. The original downloads remain outside the repository.

The user selected kora and guqin construction for the next footage search. Existing performance excerpts and two instrument-making source leads are available at `/scene5-loop/candidates/`. The new making films have not been cut or incorporated: an exact excerpt and reuse basis remain to be established. No outreach was sent.

## Native validation and publishing

Run `cargo build -p moss-cli`, generate the locale entries, then `target/debug/moss-cli build site --serve --watch`. Validate the printed URL with `node scripts/check-site-preview.mjs <url>`; the checker follows each HTML document's asset base and covers all three landing routes, linked documentation, favicons, CSS, JavaScript, and images.

Production `mosspub.com` is the existing moss-hosted `landing` site. The owning checkout at `moss/site` holds its established identity and deployment state. Publishing a tested worktree output can use the native CLI's `deploy <owning-site> --prebuilt=<absolute-output>` path without switching the main checkout or rebuilding different sources. Prebuilt deployment updates the live generation and deployment record but does not create source history; `history --restore` is therefore not an immediate server rollback. Keep a known prior output when shipping through this path.
