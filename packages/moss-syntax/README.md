# @symbiosis-lab/moss-syntax

moss's markdown syntax layer, extracted so **one implementation** serves every
host (the moss editor today, the Obsidian plugin next). Design:
`docs/archive/2026-08-11-cm6-extraction-design.md` (#1020).

Workspace-private until the Obsidian plugin ships publicly.

## What the `.` export carries (L1 — the core layer)

Zero CM6 at runtime; the only peer is `@lezer/markdown`, imported type-only.
Works anywhere a string works (Obsidian mobile included).

- `shortcode` — `:::shortcode` fence grammar (`shortcodeBlockConfig`), the
  open/close line predicates (`SHORTCODE_OPEN_RE`, `isOpenMatch`,
  `isCloseFence`), the attr parser (`parseAttrKvSpans`, a mirror of moss-core
  `ast::attrs`), and `shortcodeAssetRef`.
- `scan` — `scanShortcodeBlocks(text)`, a pure text-driven scanner over the
  same predicates, for hosts that cannot register a `@lezer/markdown`
  extension (Obsidian's markdown language is closed to grammar extensions).
  Also `KNOWN_SHORTCODES`, the authorable-name vocabulary.
- `wikilink-grammar` / `math-grammar` — `[[wikilink]]` / `$math$` inline
  grammars (ADR-041; math contract-tested against the shared Rust vectors).
- `wikilink-syntax` — `[[…]]` strip/wrap helpers.
- `completion-core` — pure completion-context line parsers and DTO types.
- `contract/shortcodes.generated` — the shortcode catalog, generated from
  Rust (`crates/moss-core/src/contract/shortcodes.rs`). Do not edit.

## `./cm6`

The CM6-importing, host-free extensions: `cm-active-lines`, `cm-link-extract`,
`cm-image-extract`, `cm-criticmarkup`, `cm-highlight`, `cm-shortcode-block`
(styles at `./cm6/cm-shortcode-block.css`) and `cm-link-resolver`.

Host-free means what it says. Anything only a host can answer arrives as an
option: user-visible strings as **thunks** (called at `toDOM` time, so a locale
switch is picked up without rebuilding the extension), and reference resolution
as a structural type the host's own model satisfies — no import back into moss.

## Constraints

Three facts that bind code outside this package. They were the surviving half of
the Obsidian hub issue (#897) when it closed.

**A second copy of `@codemirror/state` breaks dispatch, silently.** Obsidian
supplies `@codemirror/*` and `@lezer/{common,lr,highlight}` as externals, and
CM6 dispatches facets by `instanceof`. Two copies in one process means facets
registered against one are invisible to the other — no error, just extensions
that do nothing. That is why every CM6 dependency here is a `peerDependency`,
why the Obsidian plugin's bundler marks the same modules external, and why
moss's own versions must stay compatible with what Obsidian ships (they align
today: moss `@codemirror/state ^6.7.1` / `@codemirror/view ^6.43.6` against
Obsidian's `6.7.0` / `6.43.5`). Check before bumping either side.

**Adding a `moss-core` AST variant is a one-way door.**
[ADR-030](../../../docs/decisions/ADR-030-latex-math-rendering.md) §4: the `Inline`
and `Block` enums are published, serialized and not `#[non_exhaustive]`, so a
new variant is a semver break and needs its own ADR. A syntax feature that
seems to want one should first be tried as a transform over the existing
`Other` passthrough — which is how math itself ships.

**The Obsidian plugin needs a public repo.** Obsidian stopped accepting new closed-source plugins in May 2026. This package stays `"private": true` with no changesets entry until the mirror pipeline publishes it.

## Fixtures

`fixtures/` holds copies of the cross-language golden vectors whose source of
truth is Rust (`crates/moss-core/tests/fixtures/` + `tests/fixtures/`). They
are written by the same codegen run that emits `contract/shortcodes.generated.ts`
(`cargo run --bin generate-artifacts --features dev-tools -- shortcode-catalog`)
and CI fails on a diff, so a fixture edited here and not in Rust cannot survive.
