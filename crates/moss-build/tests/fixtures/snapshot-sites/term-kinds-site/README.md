# term-kinds-site

Covers: declared term kinds.

One declared kind — `[terms.people] fields = ["author", "editor", "jury"]` — so three frontmatter fields feed one namespace and one person can be reached through more than one of them. What only a whole-build snapshot can see:

- a claimed term page (`ada-lin.md` sets `author_page: true`) rendering its listing split into per-field sections, each under an `<h2 class="moss-term-role">` heading;
- a generated term page for an unclaimed person doing the same;
- a person reached through exactly one field, and the `tags/craft/` page, rendering with no heading at all — the same markup any folder index emits;
- `people/` as the namespace root, titled from the kind's `title`.

See `basic-site/README.md` for the regeneration procedure. Regenerate this fixture alone with:

```sh
SNAPSHOTS=overwrite cargo test -p moss-build --test snapshot_tests -- snapshot_term_kinds_site --exact
```
