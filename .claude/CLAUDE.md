# Agent instructions for this repository

Read [ARCHITECTURE.md](../ARCHITECTURE.md) before creating or moving any file — it names what each crate owns, what it must never depend on, and where new code belongs. A relocation is never scheduled for its own sake.

## This repository is public

This is the MIT-licensed public half of moss (`moss-core`, `moss-build`, `moss-cli`, `packages/`). Everything committed here is visible to the world. Before writing anything (code, comments, commit messages, docs):

- No paths from outside this repository, and no names of other repositories.
- No internal ADR ids or design-doc paths (`docs/decisions/…`, `docs/archive/…`) — they live in the private desktop repo and a stranger can't open them. Keep the claim and reword it to stand on its own. `25dc7f1f` removed about 560 of these, and a change prepared the same day re-added two from text it had copied before that commit.
- No client, site, or person names.
- No internal issue/tracker numbers — they resolve against the wrong tracker for a stranger reading this repo, or don't resolve at all.
- No AI attribution in commits or generated text.

## Rust conventions

- Never create `foo/mod.rs`. This tree uses the sibling-file pattern only (`foo.rs` + `foo/bar.rs`). Confirm before assuming otherwise: `find crates -name mod.rs` must be empty.
- Never run `rustfmt` or `cargo fmt` on an existing file. The tree isn't fmt-clean and nothing gates it, so reformatting one file for a small fix produces a diff many times the size of the actual change. Match the surrounding style by hand. `rustfmt` is fine on a brand-new file only.

## Testing

Always run `cargo test --workspace` from the repo root, not from inside a single crate — a plain `cargo test` in one crate's directory silently skips the others.

Build output is checked byte-for-byte by a snapshot suite: `cargo test -p moss-build --test snapshot_tests`. Regenerate its fixtures deliberately, never to silence a failure you haven't read, with `SNAPSHOTS=overwrite cargo test -p moss-build --test snapshot_tests`.

## Worktrees

One worktree per agent, under `.worktrees/` at the repo root. Never switch the branch of the root checkout. Before landing, resolve the base to a SHA once (`git rev-parse origin/develop`) rather than re-reading the remote-tracking ref later — it is shared across worktrees and can move under you while you work.

Land a finished branch on `develop` as one squashed commit, with a conventional-commit subject describing the feature, not the branch.

## The size gate

`scripts/ratchet.mjs` enforces shrink-only budgets on file size and directory-child counts (see ARCHITECTURE.md's "Size rules"). Run it yourself before committing:

```sh
node scripts/ratchet.mjs check
```

If it's red because your change grew something and that growth is deliberate, use `accept` with a real path and a real reason — never a bare sweep:

```sh
node scripts/ratchet.mjs accept prod_lines_per_file "<reason>" --path <file>
```

The reason must say why the growth is worth it, in at least 15 characters; `wip`, the row name or the path alone are refused. Include the printed `Ratchet-Accept: <row> <key> — <reason>` line(s) in your commit message; a `commit-msg` hook checks for it whenever the baseline file is staged. If your change only shrank something, run `tighten` instead — it lowers the baseline and never raises it.

Hooks live in `.githooks/` and install automatically via `pnpm install`'s `prepare` script. If they aren't active, `git config core.hooksPath .githooks` from the repo root wires them in.
