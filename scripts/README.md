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

These checks are timing-sensitive and must never run while another browser-heavy job runs on the same machine; concurrent runs on 2026-09-20 produced false failures (a settle spring moving the page under a drag, and a wheel gesture split into extra commits under load) that were not real bugs.

One command runs the whole regression net — every check below plus `check-landing-structure.mjs` — against a single served build, printing one script/engine/pass-or-fail table:

```bash
PLAYWRIGHT_MODULE=/absolute/path/to/playwright/index.mjs node scripts/check-landing-all.mjs
```

With no argument it builds nothing itself but serves `site/.moss/build/current` on a free local port (`scripts/landing-harness.mjs`); run `target/debug/moss-cli build site` first. Every script below is also independently runnable the same way — `node scripts/check-landing-pin.mjs` with no URL serves the local build; passing a URL (`https://scratch.mosspub.com/` or another `http://localhost:PORT/`) runs the same script against a deployed site instead, including the scratch deployment used before publication. The preview bridge can change harvested HTML; in September 2026 it masked a shipping transform that corrupted a quoted `data-moss-preview` attribute and broke the watercolor renderer only after deployment — native preview remains the editing workflow, the plain artifact check verifies exactly what gets published.

The scripts reuse an installed Playwright package (`playwright` by default); no new browser dependency is required. The transition check runs Chromium and WebKit; `ENGINE=chromium` or `ENGINE=webkit` narrows a diagnosis.

`check-landing-all.mjs --landing-only` skips the three doc-theme checks (`check-docs-media.mjs`, `check-docs-footer-icon.mjs`, `check-favicon-theme.mjs`) for a faster loop while iterating on the landing page alone; the URL argument still works in either position. `LANDING_GPU=1` (read by `scripts/landing-harness.mjs`'s `launchChromium`, used by the watercolor pigment checks — `check-watercolor-fidelity.mjs`, `check-watercolor-morph.mjs`) launches Chromium's own headless-new mode with hardware graphics on (`--enable-gpu --use-angle=metal --ignore-gpu-blocklist`) instead of its default software rasterizer, to measure whether GPU compositing changes wash timing; unset (or any value but `1`), Chromium launches exactly as it always has.

`check-landing-invariants.mjs <site-url>` states the page's rules directly rather than one incident at a time: native scrolling stays monotone through a real down/up trace and restores the opening hero (I-monotone); desktop exposes proximity snap geometry and reachable rests (I-snap); scene crossings show the exact scene in both directions (I-crossing); a dragged scene-1 plate or scene-3 card never leaves the print rectangle (I-rect); scene 3's cold load has only the video docked, with the shell and its nested preview sharing one transform (I-scene3); reduced motion reaches every scene's rest in at most one `scrollTo` write (I-reduced); a seeded fast-jump fuzz always ends at rest with no page errors (I-fuzz); and every check script's own default navigation is unflagged (I-default, a static scan, no browser). `check-landing-structure.mjs` is not a page check at all -- it ratchets four counts in the runtime script itself (window.__ globals, top-level lets, scene-index literals in the shared functions, byte size) and fails if any of them rises.

`check-landing-invariants.mjs` also takes `--only=<name>[,<name>...]` to run a subset by the I-* names above (e.g. `--only=I-rect,I-plate`; an unknown name is an error that lists the known ones), `--engine=chromium|webkit` to launch and check only that engine, and `--repeat=N` to run the selection N times against the same launched browsers and print one `passed/N` summary line plus every distinct failure message with its own count, exiting non-zero if any run failed -- a flake's pass rate, not just its first stack trace. With none of the three given the script behaves exactly as it always has.

`check-landing-cold-bottom.mjs <site-url>` delays the harvested UI and verifies immediate closing-scene display, reversal, and a jump during an active join on desktop and mobile. It accepts `PLAYWRIGHT_MODULE` like the other browser harnesses. Run it against plain compiled files or the scratch deployment as well as the native preview. (`check-landing-intent.mjs` was deleted; I-monotone now covers its three locales and two desktop viewports in both directions after a real native-wheel trace.)

`check-landing-pin.mjs <site-url>` verifies desktop artwork coordinates immediately after scrolling and again after animation frames in Chromium and WebKit. `LANDING_HTML_OVERRIDE` optionally supplies an older source document for regression verification; since the runtime moved to `site/landing.js` (phase 3), pair it with `LANDING_JS_OVERRIDE` for a real full-page regression test -- set both or neither, since setting exactly one throws rather than silently covering half the page. `check-landing-mobile.mjs` takes the same two variables (`LANDING_HTML_STDIN` in place of `LANDING_HTML_OVERRIDE` also works). Native desktop scrolling removed the wheel-tail model and its check: `check-landing-invariants.mjs` now owns I-monotone, I-snap, and I-crossing alongside the retained visual and state invariants.

`check-landing-mobile-handoff.mjs <url>` checks mobile source geometry and visibility at zero watercolor progress, then confirms gradual target reveal in Chromium and WebKit. Set `PLAYWRIGHT_MODULE` to an existing Playwright installation.

Before publishing the landing page, run `check-landing-subscription.mjs <url>` with `PLAYWRIGHT_MODULE` set. It intercepts API requests and exercises validation, focus, new and existing subscriber responses, and failure recovery in all three languages without creating subscribers. Compare the production form action with the proposed form: a successful HTTP response alone does not prove the correct subscriber list is being used. Keep an exact backup of the currently published file generation before a prebuilt deployment. Stop the native watcher before creating and freezing release output, and run browser checks against that plain frozen output.
