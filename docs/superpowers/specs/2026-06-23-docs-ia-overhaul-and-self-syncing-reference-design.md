# Design: moss docs IA overhaul + self-syncing reference

**Date:** 2026-06-23
**Status:** Reviewed (4 specialist reviews → approve-with-changes, incorporated below)
**Repos:** `moss-releases` (docs site), `moss` (app — Phase 0 only)

## Goal

Reorganize the moss documentation so **Get Started is the docs landing page**, the
overall IA follows a clear writer-track / builder-track split, the two builder
guides (theme, plugin) **point at a contract reference instead of duplicating it**,
and that reference **cannot drift** from the codebase because it is generated from
`moss describe --json` and gated in CI.

This supersedes the current state where `/docs/` is a "Documentation" concept page,
Get Started is buried at `/docs/start/`, the theme/plugin guides duplicate (and
conflict on) contract material, and the `/contract/reference/` link is dead.

## Non-goals

- Translating the new/rewritten English pages into zh-hans/zh-hant (deferred; the
  *structure* is mirrored in all three languages, the deeper English content is not
  translated in this effort).
- Rewriting already-calm existing pages (callouts, navigation, shortcodes, etc.) —
  they move unchanged.
- A visual redesign of the docs theme.

## Target information architecture

Two tracks. **Writer track** (Get Started → Writing) serves the majority audience
of non-technical writers. **Builder track** (Design → Reference → Extend) serves
theme/plugin authors and AI agents.

```
docs/   →  GET STARTED  (the landing: install → write → publish)
├── how-moss-works.md         concept / mental model
├── from-matters.md  newsletter.md  syndicate.md  domain.md  deploy.md
│                             getting-online next steps (promoted out of start/)
│
├── writing/    WRITING — author track
│     index, frontmatter, structure, callouts, navigation,
│     wikilinks-and-embeds, media, multilingual, editor, editors, shortcodes/*
│
├── design/     DESIGN — "Write a theme" (one calm narrative → reference)
│     index.md
│
├── reference/  REFERENCE — the contract (look-up; AI-facing; auto-synced)
│     index, css-tokens, components, html-structure, cli, manifest, hooks, slots
│
└── extend/     EXTEND — "Write a plugin" (one calm narrative → reference + moss-api/moss-core)
      index.md
```

**Framing rationale:** Design and Extend are the two AI-facing "how to build" guides.
Each is a single calm narrative that points into **Reference** (the machine contract)
rather than reproducing tables. This removes the current duplication/drift (the
`manifest.md` vs `plugin-development.md` hook-name conflict; inline token tables in
`css.md`) and gives the dead `/contract/reference/` link a real, generated home.

## File mapping — English (full reorg + content work)

| Source | Target | Action |
|---|---|---|
| `docs/start/index.md` | `docs/index.md` | move + **calm rewrite** (Get Started landing) |
| `docs/index.md` (old "Documentation") | `docs/how-moss-works.md` | **new** concept page, absorbing the old intro |
| `docs/start/from-matters.md` | `docs/from-matters.md` | move |
| `docs/start/newsletter.md` | `docs/newsletter.md` | move |
| `docs/start/syndicate.md` | `docs/syndicate.md` | move |
| `docs/start/domain.md` | `docs/domain.md` | move |
| `docs/deploy.md` | `docs/deploy.md` | stays (getting-online step) |
| `docs/author/index.md` | `docs/writing/index.md` | move + reframe |
| `docs/author/frontmatter.md` | `docs/writing/frontmatter.md` | move |
| `docs/author/callouts.md` | `docs/writing/callouts.md` | move |
| `docs/author/navigation.md` | `docs/writing/navigation.md` | move |
| `docs/author/wikilinks-and-embeds.md` | `docs/writing/wikilinks-and-embeds.md` | move |
| `docs/author/editor.md` | `docs/writing/editor.md` | move |
| `docs/author/shortcodes/*` | `docs/writing/shortcodes/*` | move (whole folder) |
| `docs/structure.md` | `docs/writing/structure.md` | move |
| `docs/media.md` | `docs/writing/media.md` | move |
| `docs/multilingual.md` | `docs/writing/multilingual.md` | move |
| `docs/editors.md` | `docs/writing/editors.md` | move |
| `docs/design/index.md` + `css.md` + `javascript.md` | `docs/design/index.md` | **consolidate** into "Write a theme" (calm); token/component/DOM tables move to reference |
| `docs/extend/cli.md` | `docs/reference/cli.md` | move |
| `docs/extend/manifest.md` | `docs/reference/manifest.md` | move + dedupe |
| `docs/extend/hooks.md` | `docs/reference/hooks.md` | move + dedupe (canonical hook names) |
| `docs/extend/slots.md` | `docs/reference/slots.md` | move |
| `docs/extend/index.md` + `plugin-development.md` | `docs/extend/index.md` | **consolidate** into "Write a plugin" (calm) |
| — | `docs/reference/index.md` | **new** reference hub |
| — | `docs/reference/css-tokens.md` | **new** (generated from `moss describe`) |
| — | `docs/reference/components.md` | **new** (generated from `moss describe`) |
| — | `docs/reference/html-structure.md` | **new** (shell/article DOM) |

**Deleted after consolidation:** `design/css.md`, `design/javascript.md`,
`extend/plugin-development.md`, the old `docs/index.md` content (becomes
how-moss-works).

## File mapping — zh-hans / zh-hant (structure only)

Mirror the section layout and the moves; do **not** translate new/consolidated
English content. Accepted, documented divergences:

- `start/index.md` → `docs/index.md`; old `docs/index.md` (概览) → `docs/how-moss-works.md`
  (rename of existing content, no new prose).
- `start/{from-matters,newsletter,syndicate,domain}.md` → `docs/*` (promote).
- `author/*` + `structure,media,multilingual,editors` → `writing/*`. (zh has no
  `author/editor.md` and no `author/shortcodes/` — those leaves simply don't exist
  there.)
- `design/` keeps `index` + `css` + `javascript` as separate pages (English
  consolidated them; zh keeps the finer-grained set until translated).
- `extend/{cli,hooks,manifest,slots}.md` → `reference/*`; `extend/index.md` stays as
  the plugin section page. zh `reference/` does **not** get the new
  `css-tokens`/`components`/`html-structure` pages (English-only) and is **not**
  auto-synced (manual translation until tooling exists).

## Children-listing design (every folder page)

Each folder/section page is a guided index, not a raw file dump:

1. **Frame:** open with a one-line description of what the section is for (and who
   it's for, when the writer/builder split matters).
2. **Order:** children sorted by reading sequence via `weight` (lower = first); fix
   the current weight collisions. Onboarding/next-step pages get the lowest weights;
   advanced/reference material sorts last.
3. **Describe:** every child page carries a `description:` so the auto-listing shows
   a meaningful subtitle.
4. **Style:** pick `children_style` deliberately per section (e.g. `list` for
   prose-heavy sections; `minimal` only where a dense index is wanted).

Apply to: the docs root (Get Started landing — next-steps weighted before the four
sections), `writing/`, `writing/shortcodes/`, `reference/`, and the zh mirrors.

## Voice & scope

- **Calm rewrite** (the privacy-rewrite voice: plain prose, minimal bold, em-dashes
  split into sentences) applies to the new/consolidated English pages only: Get
  Started, How moss works, the theme guide, the plugin guide, the Reference hub +
  generated pages' framing.
- Existing already-calm pages move unchanged.
- zh-hans/zh-hant: structural moves + weight/wikilink fixes + children-listing
  framing only.

## Reference section + self-syncing architecture

**Principle:** the running moss binary is the single source of truth. Reference pages
are generated from `moss describe --json`; the contract regions are never hand-edited.

### Phase 0 — extend `moss describe --json` (moss app repo)

`moss describe --json` already emits tokens (light+dark), components (`authorable`),
and frontmatter fields. Extend it to also emit:

- **manifest** schema (plugin `manifest.json` fields + types + defaults)
- **hooks** contract (the five capabilities + their context fields + canonical names)
- **slots** list (named template injection points)
- **cli** command list (name, args, description)
- **html-structure** the shell/article DOM skeleton (the stable element/class
  scaffold theme JS hooks into), derived from the `shell.html` / `article-content.html`
  templates — `describe` does not expose this today, so either surface it here or have
  the generator read the templates directly. (Token + component data already come
  from `describe`.)

Sources already exist in `crates/moss-core/src/contract/`, the CLI definition, and
the desktop app's template assets; this surfaces them through the existing describe
command (or, for the DOM skeleton, the generator reading the template) so the whole
contract has a generated source. Follows moss-app conventions (worktree → develop → release); gated by the
existing describe totality tests.

### Phase 2 — generator + gates (moss-releases)

1. **`scripts/sync-reference.mjs`** runs `moss describe --json` (pinned moss version)
   and rewrites the `<!-- auto:start:<section> -->…<!-- auto:end -->` regions in
   `site/docs/reference/*` and the field-table region in `writing/frontmatter.md`.
   (Gives the currently-orphaned markers a real generator.) The theme guide
   (`design/index.md`) holds **no** auto-region tables — it links to
   `reference/css-tokens` and `reference/components` instead, so the old `css.md`
   token/component regions are retired, not migrated.
2. **CI diff-gate:** a moss-releases workflow runs the generator then
   `git diff --exit-code`; stale or hand-edited regions fail the build. Mirrors moss's
   `bindings.ts` / `reference.md` discipline.
3. **Pre-commit hook:** regenerate locally on commit, extending the existing
   `check-sc-demos.sh` pre-commit pattern.
4. **Release-triggered refresh:** when moss publishes a version, a workflow bumps the
   pinned moss version, re-runs the generator, and opens a PR to moss-releases. Each
   released contract is captured; the docs never silently lag.

**Pinned binary:** the generator runs against a specific released moss version (the
one the docs describe), obtained from the moss-releases GitHub releases. The pin lives
in the workflow/config and is bumped by the release-refresh PR.

**Coverage:** auto-sync targets the **English** reference pages. zh reference pages
are manually maintained translations (out of scope here).

## URLs & wikilinks

- **Wikilinks largely survive moves.** moss resolves bare `[[filename]]` to the
  nearest match, so `[[structure]]`, `[[newsletter]]`, `[[slots]]` keep resolving
  after a move. **Path-qualified** links (`[[author/frontmatter#Children]]`,
  `[[start|Get Started]]`) must be rewritten. The reorg includes a sweep: grep every
  `[[…/…]]` and `[[start…]]` and update the path segment.
- **URLs change:** `/docs/start/`→`/docs/`, `/docs/start/x`→`/docs/x`,
  `/docs/author/*`→`/docs/writing/*`, `/docs/{structure,media,multilingual,editors}`→
  `/docs/writing/*`, `/docs/extend/{cli,hooks,manifest,slots}`→`/docs/reference/*`,
  `/docs/design/{css,javascript}`→folded into `/docs/design/`.
- **In-app deep-links:** the moss app may link to specific doc URLs (e.g.
  `/docs/author/...`, `/docs/start`). The plan must enumerate changed URLs and check
  the app for references; for any the app depends on, either update the app or pin
  `url:` on that page. Footer `/privacy` links are unaffected.

## Phasing & sequencing

| Phase | Repo | Depends on | Summary |
|---|---|---|---|
| 0 | moss | — | Extend `moss describe --json` (manifest/hooks/slots/cli) |
| 1 | moss-releases | — | Docs IA reorg (3 langs) + English rewrites + children-listing |
| 2 | moss-releases | 0 | Reference pages + `sync-reference.mjs` + CI gate + pre-commit + release refresh |

Phases 0 and 1 are independent and can proceed in parallel. Phase 2 needs Phase 0
for the manifest/hooks/slots/cli reference content (tokens/components/frontmatter are
already in `describe`, so those reference pages could land earlier).

## Risks & mitigations

- **URL churn breaks inbound/app links** → enumerate changed URLs; audit the app;
  pin `url:` where needed.
- **Path-qualified wikilinks break** → grep-and-rewrite sweep + a build that surfaces
  unresolved wikilinks.
- **en/zh structural divergence** (design/extend leaf sets differ) → documented and
  bounded; structure parallel at the section level.
- **Generator pins the wrong binary** → version pin is explicit in the workflow;
  release-refresh keeps it current; CI diff-gate catches mismatch.
- **Auto-region edited by hand** → CI `git diff --exit-code` fails the build.

## Verification

- Build all three languages; confirm `/docs/`, `/docs/zh-hans/`, `/docs/zh-hant/`
  render Get Started as the landing.
- No unresolved wikilinks in the build; no 404s in internal doc links.
- Reference pages match `moss describe --json` (the diff-gate is green).
- Spot-check each folder listing: framing present, ordered, every child described.
- Confirm footer `/privacy` links and the privacy pages are untouched.

## Revisions from expert review (2026-06-23)

Four specialist reviews (IA/UX, technical writing, technical-editor/code-verified,
i18n) returned approve-with-changes. Incorporated decisions:

### IA & landing
- **The docs root authors its own hierarchy.** `docs/index.md` (Get Started) sets
  `children: false`; the body curates the path: the quickstart, then a "What's next"
  list (from-matters, newsletter, syndicate, domain, deploy) inline, then an explicit
  **Sections** block linking the four section hubs. No raw auto-listing at the root.
- **`how-moss-works.md` is kept** (a rename of the old `docs/index.md`), retitled to a
  real explanation ("How moss works" / "moss 如何运作" / "moss 如何運作"), weighted
  immediately after Get Started, and linked from the Get Started body ("New to moss?
  Here's how it works"). It explains the folder→site model, the build pipeline, the
  pre-paint `data-theme`, and why there is no config file. Keeping it as a rename
  preserves `translationKey: docs` across all three languages with no zh rewrite.

### Voice (executable rules, not just "match Privacy.md")
1. Sentences end a thought; don't use em-dashes to continue a clause — split into two.
2. Bold marks something the reader will click or type, not emphasis; ≤2 per section.
3. No hedges ("you might want to", "it's worth noting", "feel free to") — state it.
4. Lead with what the user does, not what moss does ("Drag moss to Applications").
5. A code block for anything the reader copies; inline code only for a name.

### Diátaxis discipline
- **Get Started is a tutorial:** single goal → numbered steps → "you're live" → next
  links. No background explanation (that lives in How moss works).
- **Builder guides keep a minimum inline:** (a) one minimal working example, (b) the
  directory/invocation context (where `style.css` / the plugin folder goes), (c) the
  one sentence explaining *why* it works. Everything else (full token tables, hook
  signatures, manifest field types) → Reference, linked. Guides must not be hollow
  link-lists.
- Section page titles are **"Write a theme"** and **"Write a plugin"** (verb-object,
  unambiguous) rather than the bare nouns "Design"/"Extend". URL slugs stay
  `design/` and `extend/` for stability.

### Wikilink & URL sweep (expanded — applies to ALL three languages)
- Grep `\[\[start` (catches bare `[[start]]`, which breaks: the `start/` folder-note
  resolution disappears), every `[[…/…]]` path-qualified link, AND hardcoded absolute
  `/docs/…/` URLs (several zh `callouts.md`/`navigation.md` carry `/docs/author/…`
  literals; these are not wikilinks and won't auto-resolve).
- `translationKey` atomicity: move both language sides together; the final key on
  `docs/index.md` is `docs-start` in all langs, `how-moss-works.md` keeps `docs`.
- Verified: the moss app does **not** deep-link any reorganized `/docs/` path (grep of
  the app's source tree), so URL-churn risk is LOW. The one stale doc URL in
  the binary (`describe.rs` → `landing.mosspub.com/contract/v1/reference.md`) is
  noted for a follow-up app update, not a blocker.

### zh handling
- **Strip the `auto:start` markers from zh `design/css.md` (+ zh-hant)** and make the
  content static. The English generator won't maintain zh regions, and the diff-gate
  won't cover them, so leaving the markers in place would silently drift while looking
  auto-maintained.
- New `docs/index.md` (Get Started) zh titles: 开始使用 / 開始使用 (inherited from the
  promoted start page).
- Add a one-line "English is authoritative; 中文译文陆续更新" notice to zh `reference/`
  pages (manually maintained, may lag).
- **Structural-drift convention:** every new English page must either have a matching
  zh stub (placeholder + correct `translationKey`) or be listed in a root
  `UNTRANSLATED.md` manifest, so the trees don't silently diverge.

### Phase 0 source-location corrections (verified against code)
- Hooks (`Capability`/`PluginHook`) and manifest (`PluginManifest`) live in the desktop
  app's plugin types module; slots (`SLOT_NAMES`) in the app's plugin-enhance and
  build-slots modules — **Tauri layer, not `moss-core/contract/`.** They are surfaced
  through the app's existing describe entry point (which runs in Tauri context).
- CLI commands are a bare match block in the app's startup run-mode module with no
  registry — the fuzzy part of Phase 0 (hand-register a table or refactor dispatch).
- html-structure: include `page-content.html` alongside `shell.html` +
  `article-content.html`; the generator can read these templates directly (no Phase 0
  dependency for html-structure specifically).

### Phase 2 operational corrections
- Cross-repo refresh PR needs a provisioned PAT/fine-grained token with write to
  moss-releases, OR (simpler) run the refresh workflow **in moss-releases** on
  `workflow_dispatch`/schedule that pulls the pinned release binary. Prefer the latter.
- CI must use `.moss/build/current/` (the stale `site/.github/workflows/deploy-docs.yml`
  references `site/.moss/site`, an obsolete path — fix as part of Phase 2).
- The generator runs on a **macOS** runner (the published binary is
  `moss-darwin-universal`; no Linux CLI build exists). `describe` output is
  deterministic (BTreeMap), so the `git diff --exit-code` gate is stable.

## Out of scope / follow-ups

- zh-hans/zh-hant translation of the new/consolidated English content.
- zh reference-page auto-sync.
- Any docs theme/visual changes.
