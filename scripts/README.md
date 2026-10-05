# Docs tooling

## Build the docs with the custom landing page

`build-site-with-landing.mjs` runs the real moss compiler in a temporary copy
of `site/`, then installs the landing page and its asset trees at the compiled
site root. The source vault stays clean and the route contract in
`landing-routes.json` is checked before the build succeeds.

```bash
cargo build -p moss-cli
node scripts/build-site-with-landing.mjs \
  --landing /path/to/moss-landing-lab \
  --moss-bin target/debug/moss-cli \
  --out dist/site
```

The result serves the custom home at `/`, the compiled documentation at
`/get-started/` and `/docs/`, and the landing's nested scene assets from their
root-relative paths. The script also verifies the English and Chinese editor,
media, design, and extension routes consumed by the landing page.

`release-channels.json` records the release assets and install commands that
were verified for the landing's download controls. Keep unavailable targets
explicitly unavailable; update the record from the public GitHub release, npm,
and Homebrew metadata whenever a new release changes platform coverage.

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

## The docs demo (site/ui) — check-demo-scenes.mjs, check-docs-demo.mjs, record-scene.mjs

A documentation page can pin a real moss interface beside the text and let a reader play a short scripted scene against it, or take over at any moment — see [../site/ui/demo/README.md](../site/ui/demo/README.md) for the authoring contract and module map.

`npm run test:demo-player` is the no-browser gate: player unit tests plus `check-demo-scenes.mjs`, which walks every page's `#scene=` links against `site/ui/demo/scenes/*.json` and each scene's surface adapter, in seconds, before a browser is ever involved.

`check-docs-demo.mjs` is the real-browser check, in both Chromium and WebKit, against a served build:

```bash
node scripts/check-docs-demo.mjs http://127.0.0.1:PORT/
```

`record-scene.mjs` records an author's own clicks against the real harvested document and writes the scene JSON a Markdown link plays back:

```bash
node scripts/record-scene.mjs my-scene
```

Both launch Playwright through `site-check-harness.mjs`'s `loadPlaywright()` (`PLAYWRIGHT_MODULE` to point at an existing install) and, for `record-scene.mjs`'s unscripted server mode, `resolveBaseURL()`.

## Landing animation checks

These checks are timing-sensitive and must never run while another browser-heavy job runs on the same machine; concurrent runs on 2026-09-20 produced false failures (a script moving the page under a drag, and a wheel gesture split into extra commits under load) that were not real bugs.

One command runs the whole regression net — every check below — against a single served build, printing one script/engine/pass-or-fail table:

```bash
PLAYWRIGHT_MODULE=/absolute/path/to/playwright/index.mjs node scripts/check-landing-all.mjs
```

With no argument it builds nothing itself but serves `site/.moss/build.nosync/current` on a free local port (`scripts/landing-harness.mjs`); run `target/debug/moss-cli build site` first. Every script below is also independently runnable the same way — `node scripts/check-landing-pin.mjs` with no URL serves the local build; passing a URL (`https://scratch.mosspub.com/` or another `http://localhost:PORT/`) runs the same script against a deployed site instead, including the scratch deployment used before publication. The preview bridge can change harvested HTML; in September 2026 it masked a shipping transform that corrupted a quoted `data-moss-preview` attribute and broke the watercolor renderer only after deployment — native preview remains the editing workflow, the plain artifact check verifies exactly what gets published.

The scripts reuse an installed Playwright package (`playwright` by default); no new browser dependency is required. The transition check runs Chromium and WebKit; `ENGINE=chromium` or `ENGINE=webkit` narrows a diagnosis.

`check-landing-all.mjs --landing-only` skips the three doc-theme checks (`check-docs-media.mjs`, `check-docs-footer-icon.mjs`, `check-favicon-theme.mjs`) for a faster loop while iterating on the landing page alone; the URL argument still works in either position. `LANDING_GPU=1` (read by `scripts/landing-harness.mjs`'s `launchChromium`) launches Chromium's own headless-new mode with hardware graphics on (`--enable-gpu --use-angle=metal --ignore-gpu-blocklist`) instead of its default software rasterizer, to measure whether GPU compositing changes wash timing; unset (or any value but `1`), Chromium launches exactly as it always has.

`check-landing-cold-bottom.mjs <site-url>` delays the harvested UI and verifies closing-scene display before capture readiness (on desktop once the picture has followed the page there, in WebKit), reversal, and a jump during an active join on desktop and mobile. It accepts `PLAYWRIGHT_MODULE` like the other browser harnesses. Run it against plain compiled files or the scratch deployment as well as the native preview.

`check-landing-pin.mjs <site-url>` verifies the pinned visual's top and that the stage stays where it was at every scroll offset, immediately after scrolling and again after animation frames, in Chromium and WebKit. `LANDING_HTML_OVERRIDE` optionally supplies an older source document for regression verification; `check-landing-mobile.mjs` and `check-landing-readiness.mjs` take it too (`LANDING_HTML_STDIN` also works for the mobile check). Desktop scrolling is native, so its checks drive it with input a browser really handles. `check-landing-desktop-scroll.mjs` (Chromium and WebKit, pointer in the left margin) requires nothing to scroll before input, a wheel notch and PageDown or PageUp to step one scene and rest without rebound (also while the editor demo holds focus), two quick notches to advance one scene, End to rest on the close and a notch up from the footer to rest on it again, the snap to be back after a carried notch, a key step to land at once under reduced motion with the snap kept and the picture not trailing the page, and the page never to cancel a wheel event; in WebKit only it also times the wash (running within 300 ms of a notch, scene 2 whole about two seconds later, four notches ending whole on scene 4, down-then-up ending whole on the scene it left). Synthetic wheel events dispatched from inside the page cannot move a natively scrolled page, so no check uses them; the page's own scroll position, scrolled by the browser, is read instead. `check-landing-desktop-pace.mjs` (WebKit, whose headless desktop simulation runs at full rate) requires the picture to follow the page at no more than its pace, the wash to follow the picture between the two rests and be whole only on arrival, the title to dissolve as ink, a resting scene to have the screen to itself, scene 2's Publish cue to send one ring at a time from the button while nothing else in the resting scene is redrawn for nothing, scene 3's artifacts to keep running through the retakes of its print, keys to stay with an editor demo the reader has used, and the close to play its recording with the picture all the way to scene 5 without stopping. Because the page snaps to its rests, a check that must hold the page between two of them turns the snap off with a style rule.

`check-landing-wash-recovery.mjs <url>` injects three failures on mobile in Chromium and WebKit — a wash frame that throws, a lost WebGL context, and a next scene whose print has not been taken — and requires the shown scene to keep following the reader across that boundary and the next; a lost context must not raise the cover, and a missing print must not delay the wash past the first fifth of the copy's passage.

`check-landing-slow-network.mjs <url>` loads each language with nothing but the HTML document and requires the copy to be readable end to end and the signup to say it needs the runtime instead of offering a form that cannot submit.

`check-landing-mobile-handoff.mjs <url>` checks mobile source geometry and visibility at zero watercolor progress, then confirms gradual target reveal in Chromium and WebKit. Set `PLAYWRIGHT_MODULE` to an existing Playwright installation.

Before publishing the landing page, run `check-landing-subscription.mjs <url>` with `PLAYWRIGHT_MODULE` set. It intercepts API requests and exercises validation, focus, new and existing subscriber responses, and failure recovery in all three languages without creating subscribers. Compare the production form action with the proposed form: a successful HTTP response alone does not prove the correct subscriber list is being used. Keep an exact backup of the currently published file generation before a prebuilt deployment. Stop the native watcher before creating and freezing release output, and run browser checks against that plain frozen output.
