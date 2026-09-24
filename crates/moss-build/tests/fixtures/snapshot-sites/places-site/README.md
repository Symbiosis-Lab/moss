# places-site

Covers: the places build slice — `[terms.places] type = "place" fields = ["location"]`, the `.moss/places.toml` gazetteer, and the roll-up.

Declares one place-typed kind. Kyoto (`kyoto.md`) claims its own term page; Osaka and Nara are unclaimed and get generated pages. Both Kyoto and Osaka name Japan as their gazetteer parent, and Japan has no gazetteer row of its own — the roll-up creates its page from the parent link alone. What only a whole-build snapshot can see:

- a claimed place page (`kyoto.md`) rendering a breadcrumb up to Japan and a children list with counts, with no coordinates anywhere in its output;
- a generated place page (Osaka) doing the same;
- Japan's own generated page, created purely by the roll-up, listing both Kyoto and Osaka as children;
- the automatic place line under the byline on every article that names a place, correctly linked;
- Osaka's missing `lng` and Nara's invalid `precision` each firing a diagnostic (visible via `cli_output_tests.rs`'s `PROBLEMS_TEST_LOCK` mechanism) without failing the build, and Osaka's row still getting its breadcrumb and roll-up despite having no coordinates;
- no `lat`/`lng` digit sequence anywhere in `expected/` — this slice renders no coordinates at all.
- the homepage body embeds Osaka's generated term page (`![[/places/osaka/]]`) and Japan's roll-up-only one (`![[/places/japan/]]`) as listings — both are pseudo-folders with no real directory behind them, reached only through `also_in` membership derived at build time, not through a real folder.

See `basic-site/README.md` for the regeneration procedure. Regenerate this fixture alone with:

```sh
SNAPSHOTS=overwrite cargo test -p moss-build --test snapshot_tests -- snapshot_places_site --exact
```
