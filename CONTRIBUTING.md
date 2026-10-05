# Contributing to moss

All work lands on `develop`, the default branch. Open pull requests against it.

## Ways to help

- **Report a bug** in the CLI, the build engine, a package or the desktop app. The desktop app's source is private, and its bug reports belong here all the same.
- **Comment on a proposal.** Designs for things moss does not do yet are in [docs/proposals](docs/proposals/), each with an issue.
- **Fix something.** Issues labelled `good first issue` are sized for a first contribution. Say in a comment that you are taking one, so two people do not do the same work.

For anything larger than a fix, open an issue before you write code. A feature that arrives as a pull request with no discussion behind it will usually be closed, however good the code is.

## Build and check

```sh
cargo build -p moss-cli      # the CLI, at target/debug/moss-cli
pnpm install                 # JS tooling, and the git hooks
cargo test --workspace       # from the repo root, never from inside one crate
node scripts/ratchet.mjs check
```

Read [ARCHITECTURE.md](ARCHITECTURE.md) before adding or moving a file. Do not run `cargo fmt` on an existing file: the tree is not fmt-clean, and the reformatting would bury your change.

Anything that changes a published page's CSS is also checked by the render gates, which lay out real pages in Chromium and WebKit: `bash scripts/render-gates.sh`. The script's header lists its flags.

## Written by you, built with any tool

Use whatever tools you like to write code, AI coding agents included. Two things have to come from you.

- **The words.** Issues, pull request descriptions and replies in review are written by you, in your own words. Write in English or in Chinese, whichever is yours.
- **The responsibility.** You are the author of everything you submit. You have read every line, you can explain it, and you have run the checks above.

An agent acting on its own may not open issues or pull requests here.

## What to expect

moss is maintained by a very small team. Everything gets read, and a first reply can take a few days. Changes that do not fit where moss is going are declined, and we try to say so early.

## Licence

moss is MIT-licensed. By contributing you agree that your contribution is licensed the same way.
