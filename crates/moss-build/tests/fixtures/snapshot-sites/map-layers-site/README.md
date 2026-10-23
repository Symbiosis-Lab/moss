# map-layers-site

Covers: `[map.layers.<name>]` in `.moss/config.toml` — one historical layer over one place.

What only a whole-build snapshot can see:

- `layers.json` lands in the same `_moss/map.<hash>/` directory as `world.svg` and `tiles.json`, with the layer's name, kind, extent, deepest zoom and credit, and `tiles` null;
- the places root figure carries `data-layers` beside `data-labels`, pointing at that file;
- every map figure, the article locator included, ends with the empty credit line the explorer fills in, and the `map-layers` stylesheet partial ships;
- the layer's `source` is an empty folder (it holds only a placeholder) and the build still succeeds.

Sites with no `[map.layers]` are byte-identical to before: the other fixtures prove it.

Regenerate this fixture alone with:

```sh
SNAPSHOTS=overwrite cargo test -p moss-build --test snapshot_tests -- snapshot_map_layers_site --exact
```
