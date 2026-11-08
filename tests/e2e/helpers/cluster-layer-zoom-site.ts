/**
 * A places explorer whose every place sits on a historical layer, with three
 * works clustered at two nearby places (two share one place). The plate is
 * the one the build's own snapshot fixture carries.
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

function work(uid: string, title: string, location: string, date: string): string {
  return `---\ntitle: ${title}\nuid: "${uid}"\nlocation: "${location}"\ndate: ${date}\n---\n\n# ${title}\n\nA note from ${location}.\n`;
}

export const CLUSTER_LAYER_ZOOM_GATE: ScratchSiteSpec = {
  name: "cluster-layer-zoom-gate",
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
max_zoom = 17
budget = "2 MB"
`,
    ".moss/places.toml": `["Churchyard Gate"]
lat = 40.7081
lng = -74.0119
precision = "exact"

["Gate Lane"]
lat = 40.7089
lng = -74.0129
precision = "exact"

["Green Bench"]
lat = 40.7048
lng = -74.0140
precision = "exact"

["Harbor Pier"]
lat = 40.7200
lng = -74.0000
precision = "exact"
`,
    "index.md": "---\ntitle: Home\n---\n\nHi.\n",
    "places/index.md": "---\nuid: 6f2ab190\n---\n\n# Places\n\nWhere the photographs were taken.\n",
    "gate-morning.md": work("clz001aa", "Gate Morning", "Churchyard Gate", "2025-06-03"),
    "gate-evening.md": work("clz002bb", "Gate Evening", "Churchyard Gate", "2025-06-02"),
    "lane-walk.md": work("clz003cc", "Lane Walk", "Gate Lane", "2025-06-01"),
    "bench-rest.md": work("clz004dd", "Bench Rest", "Green Bench", "2025-05-20"),
    "pier-light.md": work("clz005ee", "Pier Light", "Harbor Pier", "2025-05-10"),
    ...plate,
  },
};
