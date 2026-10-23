# map-layers-plate-site

Covers: a historical layer whose folder holds a real, cropped plate (`<id>.annotation.json`, `<id>.jpg`, `<id>.region.json`).

What only a whole-build snapshot can see:

- `layers.json` carries the layer's extent from the plate's footprint, the credit from `region.json`, and `tiles` pointing at `layers/atlas-1930/index.json`;
- that index lists the tiles under `layers/atlas-1930/<level>/<i>/<j>.jpg` (JPEG bytes are compared for presence only) and the footprint polygon in world coordinates;
- `NOTICE.txt` gains the layer's credit, the plate's source address and its rights line.

Regenerate this fixture alone with:

```sh
SNAPSHOTS=overwrite cargo test -p moss-build --test snapshot_tests -- snapshot_map_layers_plate_site --exact
```
