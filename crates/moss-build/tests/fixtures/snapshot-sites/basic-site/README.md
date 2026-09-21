# basic-site

Covers: docs/author/frontmatter.md, docs/author/wikilinks-and-embeds.md

A minimal working moss site: three pages (`index.md`, `about.md`, `contact.md`) with
standard frontmatter and a wikilink or two. Exercises the happy-path build.

## Layout

- `input/` — source site, as a user would author it.
- `expected/` — build output from `moss build input` with `--no-plugins`. Canonical
  snapshot; tests diff against this.

## Regenerating `expected/`

Run from the repo root:

```bash
cargo build -p moss-cli
./target/debug/moss-cli build crates/moss-build/tests/fixtures/snapshot-sites/basic-site/input --no-plugins
# then diff .moss/build/staging vs expected/; if the diff is justified, replace expected/.
```
