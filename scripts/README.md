# Docs tooling

## Build the public site

The landing page and its runtime assets live directly under `site/`. `site/index.html` owns its complete document head and body; `site/index.md` remains the moss homepage metadata source. The explicit `index.html` entry under `[build].passthrough` in `site/.moss/config.toml` makes the HTML source win the root output while moss generates every documentation route normally. Edit these sources directly, never `site/.moss/build/` or the retired overlay script.

```bash
cargo build -p moss-cli
target/debug/moss-cli build site
target/debug/moss-cli build site --serve --watch
# Use the URL printed by moss; do not assume port 8080.
node scripts/check-site-preview.mjs http://localhost:<printed-port>/
```

The checker validates the landing, all linked English and Chinese documentation routes, and same-origin CSS, JavaScript, and generated image response types. A route returning HTTP 200 is insufficient evidence: an HTML fallback at an asset URL also returns 200 while leaving the page unstyled or an image broken.

The two localized landing entry points are generated from `site/index.html` and the catalogs in `site/landing-i18n.js`. Regenerate them after changing either source; native watch rebuilds files but does not run this generator.

```bash
node scripts/generate-landing-locales.mjs
node scripts/generate-landing-locales.mjs --check
```

Keep images referenced by Markdown out of `[build].passthrough`. Passthrough subtrees are copied verbatim and excluded from moss's media index, so bare wikilinks, root-relative images, localized relative paths, and page covers cannot resolve files inside them even though the copied source URL itself returns 200.

`--strict` currently turns the unpublished-site diagnostic (“no usable record of what is live yet”) into exit 1 even when the local output is complete. Keep the diagnostic visible and use the ordinary build/preview command for this repository until the site has a live deployment record; do not suppress it in a wrapper.

`release-channels.json` records the release assets and install commands that were verified for the landing's download controls. Keep unavailable targets explicitly unavailable; update the record from the public GitHub release, npm, and Homebrew metadata whenever a new release changes platform coverage.

## The stage (site/ui/stage) — player tests and a browser checker

`node --test scripts/stage-player.test.mjs` (or `npm run test:stage-player`) runs player.js's own unit tests against a fake driver — no browser, no server, and fast enough to run on every change to that file.

`node scripts/check-docs-stage.mjs <preview-url>` drives the built site in a real Chromium and WebKit, the same PLAYWRIGHT_MODULE convention as `check-landing-mobile.mjs` describes above. One table-driven check plays every marker on every page that has a stage (en and zh-hant "Meet the editor" and "Get Started") to `done`, with no load error and no page error, within a pace-derived time bound computed from each scene's own steps — this is what proves a scene actually runs end to end, rather than each scene needing its own bespoke assertion. Alongside it, the checks named directly in `site/ui/stage/README.md` each guard one rule on their own: rest state, reader input interrupting playback, a superseded load not navigating the shared iframe, a failed-then-retried scene load, the sticky/toolbar/pinned/narrow layout rules, the page-width theme fix, and Play not shifting the page. Two checks are specific to the refreshed harvest: the framed editor's own context-menu text follows the page's locale rather than a hard-coded translation, and en/zh-hant frame two different real projects (compared structurally — differing, non-empty, Latin vs. CJK — never against a typed "Blake"/"朱耷" literal). A last check plays "versions" and confirms its mid-scene diff view shows a snippet of the piece's own live text, not a fabricated or empty one.

## sync-reference.mjs — keep the reference docs in sync with moss

The pages under `site/docs/reference/` (CSS tokens, component classes, HTML
structure, the plugin contract, the CLI) and the frontmatter table in
`site/docs/writing/frontmatter.md` are **generated from the moss binary**, not
hand-maintained. The single source of truth is `moss describe --json`; this
script fills the `<!-- auto:start:NAME -->…<!-- auto:end:NAME -->` regions of
those pages from it, so the published contract can never silently drift from the
shipped binary.

```bash
MOSS_BIN=/path/to/moss node scripts/sync-reference.mjs --check   # exit 1 if stale
MOSS_BIN=/path/to/moss node scripts/sync-reference.mjs --write   # rewrite in place
```

`MOSS_BIN` defaults to `moss` on `PATH`. Edit the source (tokens.json /
components.rs / the describe command), never the generated region.

### Region → source mapping

| Page | Regions (auto:NAME) | describe section |
|------|---------------------|------------------|
| `reference/css-tokens.md` | `tokens-<category>` | `tokens` |
| `reference/components.md` | `components` | `components` |
| `writing/frontmatter.md` | `frontmatter-<group>` | `frontmatter` |
| `reference/hooks.md` | `plugin-hooks` | `plugin_hooks` |
| `reference/manifest.md` | `manifest-fields` | `manifest_fields` |
| `reference/slots.md` | `slots` | `slots` |
| `reference/cli.md` | `cli-commands` | `cli_commands` |

## Activation (gated on a moss release)

**Prerequisite:** the plugin/CLI sections (`plugin_hooks`, `manifest_fields`,
`slots`, `cli_commands`) require `describe_schema_version >= 5`, added on the
`feat/describe-plugin-contract` branch. The currently *released* moss has no
`describe` command, so auto-sync activates only after that branch lands in a
moss release. Until then, run the generator against a locally-built binary from
that branch.

**One-time wiring** (when the release is out): in each reference page, wrap the
table the generator owns in `<!-- auto:start:NAME -->` / `<!-- auto:end:NAME -->`
markers (names per the table above; the `css-tokens`/`components` pages already
have start markers — give them matching named end markers), then:

```bash
MOSS_BIN=$(which moss) node scripts/sync-reference.mjs --write
git diff   # review the populated tables, commit
```

After that the pages stay in sync via the gates below.

### CI diff-gate (drop into `site/.github/workflows/` once activated)

```yaml
# sync-reference.yml — fail the build if reference docs lag the pinned moss
name: sync-reference
on: [pull_request]
jobs:
  check:
    runs-on: macos-latest   # the published binary is moss-darwin-universal
    steps:
      - uses: actions/checkout@v4
      # Pin to the release whose contract these docs describe:
      - run: curl -L -o moss https://github.com/Symbiosis-Lab/moss-releases/releases/download/${MOSS_VERSION}/moss-darwin-universal && chmod +x moss
        env: { MOSS_VERSION: v0.7.12 }   # bump in lockstep with releases
      - uses: actions/setup-node@v4
        with: { node-version: 20 }
      - run: MOSS_BIN=./moss node scripts/sync-reference.mjs --check
```

This mirrors the discipline moss already uses for `bindings.ts` / `reference.md`
(`git diff --exit-code` against a generated artifact).

### Pre-commit hook (local convenience)

`.git/hooks/pre-commit` (or wire via `core.hooksPath`):

```bash
#!/bin/sh
# regenerate reference docs when a source/reference page is staged
if git diff --cached --name-only | grep -qE 'site/docs/(reference|writing/frontmatter)'; then
  MOSS_BIN=$(command -v moss) node scripts/sync-reference.mjs --check || {
    echo "Reference docs are stale — run: node scripts/sync-reference.mjs --write"; exit 1; }
fi
```

### Release-refresh (run in THIS repo, no cross-repo token needed)

A `workflow_dispatch` job here (triggered after a moss release) bumps the pinned
`MOSS_VERSION`, runs `--write`, and opens a PR — so each released contract is
captured and the docs never lag a version. Running it *in* moss-releases avoids
the cross-repo PAT a moss→moss-releases push would need.

```yaml
# refresh-reference.yml
name: refresh-reference
on: { workflow_dispatch: { inputs: { version: { required: true } } } }
jobs:
  refresh:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v4
      - run: curl -L -o moss https://github.com/Symbiosis-Lab/moss-releases/releases/download/${{ inputs.version }}/moss-darwin-universal && chmod +x moss
      - uses: actions/setup-node@v4
        with: { node-version: 20 }
      - run: MOSS_BIN=./moss node scripts/sync-reference.mjs --write
      - uses: peter-evans/create-pull-request@v6
        with: { title: "docs(reference): sync to moss ${{ inputs.version }}", branch: refresh-reference }
```

## check-sc-demos.sh

Pre-commit consistency check for the shortcode demo blocks under
`site/docs/writing/shortcodes/`. See [../CONTRIBUTING-DOCS.md](../CONTRIBUTING-DOCS.md).

## Landing animation checks

These checks are timing-sensitive and must never run while another browser-heavy job runs on the same machine; concurrent runs on 2026-09-20 produced false failures (a settle spring moving the page under a drag, and a wheel gesture split into extra commits under load) that were not real bugs.

One command runs the whole regression net — every check below plus `check-landing-structure.mjs` — against a single served build, printing one script/engine/pass-or-fail table:

```bash
PLAYWRIGHT_MODULE=/absolute/path/to/playwright/index.mjs node scripts/check-landing-all.mjs
```

With no argument it builds nothing itself but serves `site/.moss/build/current` on a free local port (`scripts/landing-harness.mjs`); run `target/debug/moss-cli build site` first. Every script below is also independently runnable the same way — `node scripts/check-landing-pin.mjs` with no URL serves the local build; passing a URL (`https://scratch.mosspub.com/` or another `http://localhost:PORT/`) runs the same script against a deployed site instead, including the scratch deployment used before publication. The preview bridge can change harvested HTML; in September 2026 it masked a shipping transform that corrupted a quoted `data-moss-preview` attribute and broke the watercolor renderer only after deployment — native preview remains the editing workflow, the plain artifact check verifies exactly what gets published.

The scripts reuse an installed Playwright package (`playwright` by default); no new browser dependency is required. The transition check runs Chromium and WebKit; `ENGINE=chromium` or `ENGINE=webkit` narrows a diagnosis.

`check-landing-all.mjs --landing-only` skips the three doc-theme checks (`check-docs-media.mjs`, `check-docs-footer-icon.mjs`, `check-favicon-theme.mjs`) for a faster loop while iterating on the landing page alone; the URL argument still works in either position. `LANDING_GPU=1` (read by `scripts/landing-harness.mjs`'s `launchChromium`, used by the watercolor pigment checks — `check-watercolor-fidelity.mjs`, `check-watercolor-morph.mjs`) launches Chromium's own headless-new mode with hardware graphics on (`--enable-gpu --use-angle=metal --ignore-gpu-blocklist`) instead of its default software rasterizer, to measure whether GPU compositing changes wash timing; unset (or any value but `1`), Chromium launches exactly as it always has.

`check-landing-invariants.mjs <site-url>` states the page's rules directly (design doc R2/R3/R6/R7) rather than one incident at a time: every scene boundary commits both directions with a light gesture (I-commit); a dragged scene-1 plate or scene-3 card never leaves the print rectangle (I-rect); scene 3's cold load has only the video docked, with the shell and its nested preview sharing one transform (I-scene3); reduced motion reaches every scene's rest in at most one `scrollTo` write (I-reduced); a seeded fast-jump fuzz always ends at rest with no page errors (I-fuzz); and every check script's own default navigation is unflagged (I-default, a static scan, no browser). `check-landing-structure.mjs` is not a page check at all -- it ratchets four counts in the runtime script itself (window.__ globals, top-level lets, scene-index literals in the shared functions, byte size) and fails if any of them rises.

`check-landing-invariants.mjs` also takes `--only=<name>[,<name>...]` to run a subset by the I-* names above (e.g. `--only=I-rect,I-plate`; an unknown name is an error that lists the known ones), `--engine=chromium|webkit` to launch and check only that engine, and `--repeat=N` to run the selection N times against the same launched browsers and print one `passed/N` summary line plus every distinct failure message with its own count, exiting non-zero if any run failed -- a flake's pass rate, not just its first stack trace. With none of the three given the script behaves exactly as it always has.

`check-landing-cold-bottom.mjs <site-url>` delays the harvested UI and verifies immediate closing-scene display, reversal, and a jump during an active join on desktop and mobile. It accepts `PLAYWRIGHT_MODULE` like the other browser harnesses. Run it against plain compiled files or the scratch deployment as well as the native preview. (`check-landing-intent.mjs` was deleted; I-monotone now covers its three locales and two desktop viewports in both directions after a real native-wheel trace.)

`check-landing-pin.mjs <site-url>` verifies desktop artwork coordinates immediately after scrolling and again after animation frames in Chromium and WebKit. `LANDING_HTML_OVERRIDE` optionally supplies an older source document for regression verification; since the runtime moved to `site/landing.js` (phase 3), pair it with `LANDING_JS_OVERRIDE` for a real full-page regression test -- set both or neither, since setting exactly one throws rather than silently covering half the page. `check-landing-mobile.mjs` takes the same two variables (`LANDING_HTML_STDIN` in place of `LANDING_HTML_OVERRIDE` also works). Native desktop scrolling removed the wheel-tail model and its check: `check-landing-invariants.mjs` now owns I-monotone, I-snap, and I-crossing alongside the retained visual and state invariants.

`check-landing-mobile-handoff.mjs <url>` checks mobile source geometry and visibility at zero watercolor progress, then confirms gradual target reveal in Chromium and WebKit. Set `PLAYWRIGHT_MODULE` to an existing Playwright installation.

Before publishing the landing page, run `check-landing-subscription.mjs <url>` with `PLAYWRIGHT_MODULE` set. It intercepts API requests and exercises validation, focus, new and existing subscriber responses, and failure recovery in all three languages without creating subscribers. Compare the production form action with the proposed form: a successful HTTP response alone does not prove the correct subscriber list is being used. Keep an exact backup of the currently published file generation before a prebuilt deployment. Stop the native watcher before creating and freezing release output, and run browser checks against that plain frozen output.
