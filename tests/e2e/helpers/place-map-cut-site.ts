/**
 * A site whose historical layer does not fill the street-scale frame of a
 * map drawn over it: a place page and its locator take the cut shape, the
 * places root keeps filling the window. The plate is the one the build's
 * own snapshot fixture carries.
 */
import * as fs from "node:fs";
import * as path from "node:path";
import { openCrateDir } from "../../support/crate-paths";
import type { ScratchSiteSpec } from "./scratch-site";

const PLATE_DIR = path.join(openCrateDir("moss-build"), "tests/fixtures/snapshot-sites/map-layers-plate-site/input/maps/atlas-1930");

const plate: Record<string, Buffer> = {};
for (const name of fs.readdirSync(PLATE_DIR)) {
  plate[`maps/atlas-1930/${name}`] = fs.readFileSync(path.join(PLATE_DIR, name));
}

export const PLACE_MAP_CUT_GATE: ScratchSiteSpec = {
  name: "place-map-cut-gate",
  files: {
    ".moss/config.toml": `schema_version = 6

[site]
lang = "en"
locator = "align-right"

[terms.places]
type = "place"
fields = ["location"]
title = "Places"

[map.layers.atlas-1930]
kind = "historical"
source = "maps/atlas-1930"
max_zoom = 15
budget = "2 MB"
`,
    ".moss/places.toml": `["Churchyard Gate"]
lat = 40.7081
lng = -74.0119
precision = "exact"

["Green Bench"]
lat = 40.7048
lng = -74.0140
precision = "exact"
`,
    "index.md": "---\ntitle: Home\n---\n\nHi.\n",
    "places/index.md": "---\nuid: 6f2ab190\n---\n\n# Places\n\nWhere the photographs were taken.\n",
    "posts/gate.md": "---\ndate: 2025-06-01\nuid: 2e90b4d1\nlocation: Churchyard Gate\n---\n\n# Gate\n\nA photograph at the gate, with enough prose to sit beside the locator.\n",
    ...plate,
  },
};
