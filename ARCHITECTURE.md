# Architecture

What each crate in this repository is for, what it may depend on, and where new code goes. Read it before creating or moving a file.

## Crates

| Crate | What it's for | What it must never depend on |
|---|---|---|
| `moss-core` | pure Rust: markdown parsing, frontmatter typing, schema validation, wikilink/embed resolution, the design-token and HTML-class contract | filesystem I/O, network I/O, any async runtime |
| `moss-build` | everything that runs without a window: the build pipeline (scan → render → emit), the preview server, the publish path, the plugin engine, the HTTP client for the hosting service | any GUI toolkit, direct or transitive (no Tauri, no wry, no webview) |
| `moss-cli` | the `moss` binary | anything beyond `moss-build` — it links `moss-build` only, never `moss-core` directly |

## Dependency direction

`moss-core ← moss-build ← moss-cli`. Each crate depends only on the one(s) to its left; nothing here depends back on the CLI or on any consumer.

A closed desktop app consumes these crates as a git dependency — pinned to a branch and locked by its own `Cargo.lock`/`pnpm-lock.yaml`, not a submodule. That app is downstream of this repository, not the other way around: nothing here may depend on it, reference its internal paths, or assume it exists.

## `moss-build/src` module map

One line each, matching `crates/moss-build/src/lib.rs` as it stands today:

- `advisory` — the recovery/advisory vocabulary shared by every long-running job producer (build, domain, deploy, plugins); pure values, no I/O.
- `config` — the `.moss/config.toml` reader, the pure migration transform, and the one write primitive shared by the CLI and the compiler.
- `moss_paths` — owns the on-disk build layout: the paths every build generation writes to.
- `nested_roots` — detects and classifies nested moss roots, so a build never treats a cloud mount, a home directory, or a folder that is itself a site as safe to descend into.
- `vault_root` — the compiler's root identity: which folder is the site being built.
- `build` — the compiler itself: scan → render → emit, the staging/generations build-directory model, file watching.
- `editor` — the editor backend: path resolution, file tree, content parse/persist, source-asset bytes; shared by the desktop app's commands and the preview server's HTTP carrier.
- `engine` — the QuickJS plugin execution engine.
- `i18n` — language detection, UI string translation, and translation linking for multilingual sites.
- `ops` — long-running processes over a built site: the preview server (`ops/serve`) and the file-watch debouncer (`ops/watch`).
- `tasks` — the task/progress primitive: the registry, the plugin-task router, and the wire snapshot sent to any frontend.
- `types` — data types shared across the build tree.
- `cli` — CLI-facing pieces that ride the build itself: agent-guidance sync, the `describe`/`doctor`/`list` commands, the shared command table.
- `deploy` — the windowless half of publishing: the upload latch, the upload window, the mass-removal safety gate, progress reporting, the landed-publish record.
- `identity` — vault identity: the keypair vocabulary, the on-disk identity service, the file-based keystore.
- `infra` — shared infrastructure the build tree and the plugin runtime both reach: atomic file writers, the advisory display vocabulary, the app-data directory resolver, the surgical `config.toml` editor.
- `platform` — cloud-file materialization primitives (today: macOS file-provider I/O policy and file coordination).
- `seta` — the HTTP client for the hosting service: sites, domains, DNS, CDN, TLS, request signing.
- `plugins` — the plugin vocabulary, the bundled-plugin installer and registry client, and the plugin manager runtime.
- `system` — small OS-facing utilities the build tree needs: per-folder session state, proxy resolution, a resumable verified download, "reveal in file manager".
- `vault` — the vault family's crossed members: path aliases and the plugin-facing filesystem sandbox.

## Size rules

A Rust or TypeScript source file (tests excluded) whose production line count exceeds 800 needs a baseline entry in `scripts/ratchet-baseline.open.json`: a ceiling that is a multiple of 100. The ceiling can only shrink, except through an explicit, reasoned `accept`. A directory with 30 or more direct children needs a written disposition in the baseline. `scripts/ratchet.mjs check` enforces both; see that script's own header for the exact rules and commands.

## Where new code goes

New code lands in the module that already owns the concern it touches. A relocation — moving existing code to a different module — is never scheduled for its own sake; it happens only alongside a change that already has a reason to touch that code.

## Before adding an abstraction

Three questions, answered before building: how many real callers exist today (two or more)? What does undoing it cost? What observable event would prove it wrong?

## Known debt

The largest files in this crate, in current production lines (tests excluded, counted the same way `scripts/ratchet.mjs` counts them):

- `build/render/blocking.rs` — 3613
- `build/media/image.rs` — 3008
- `build/pipeline.rs` — 2041
- `build/media/pipeline.rs` — 1804

None of these are a waiver by design; they are unfinished decomposition, tracked (and only allowed to shrink) through the baseline referenced above.

Build output is checked byte-for-byte by a snapshot suite: `crates/moss-build/tests/snapshot_tests.rs`, with fixtures under `crates/moss-build/tests/fixtures/snapshot-sites/`. Run it with `cargo test -p moss-build --test snapshot_tests`; regenerate the fixtures with `SNAPSHOTS=overwrite cargo test -p moss-build --test snapshot_tests`.
