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

The checker validates the landing, all linked English and Chinese documentation routes, and same-origin CSS and JavaScript response types. A route returning HTTP 200 is insufficient evidence: an HTML fallback at a stylesheet URL also returns 200 while leaving the page unstyled.

`--strict` currently turns the unpublished-site diagnostic (“no usable record of what is live yet”) into exit 1 even when the local output is complete. Keep the diagnostic visible and use the ordinary build/preview command for this repository until the site has a live deployment record; do not suppress it in a wrapper.

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
