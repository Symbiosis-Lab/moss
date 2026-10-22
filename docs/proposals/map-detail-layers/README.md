# Detail layers: a historical map or today's streets under a site's places

- **Status:** In progress. Designed in October 2026; the engine changes and the historical layer are being built first.
- **Where the work would land:** this repository: the build engine (`place_map`), the places explorer script, the CLI (`moss map bbox`), and a fetch plugin. Nothing in the desktop app is needed to use it.

## The problem

moss draws its maps from public-domain world data compiled into moss: Natural Earth for land, water and rivers, and the GEBCO grid for relief. That is enough to show where on Earth a place is, and nothing finer. A site whose places sit in one city sees an empty shape under its pins: the explorer stops zooming at about half a kilometre per pixel, and the data has no streets to show if it went further.

Collection sites need that detail most, and the detail they need is often historical. A collection of 1930s street photographs belongs on a 1930s city atlas, not on today's streets. Libraries hold those atlases, scanned and in the public domain, and volunteers have already placed many of them on the globe with open georeferencing data. Other sites, about the present, need today's streets.

Whatever detail a site adds, the site has to keep working the way every moss site does. Its folder can be downloaded and rebuilt with no network, no map service and no key, and it carries the licence of everything it shows.

## What a reader sees

*Harbor Street Photographs, 1936*, an invented example: 24 photographs of one city, each page naming where it was taken.

The home page shows the whole collection on a map. It opens on the city, with the moss landscape around it. Zooming in, the 1930 city atlas appears under the pins, plate by plate, at the resolution the library scanned it. Where the atlas ends, because the library holds plates for two boroughs and not the other two, the map returns to the moss landscape along the plates' own printed edges, not along a rectangle. A credit in the corner names the atlas and the library. Each photograph's page has a small map beside it, showing the same atlas around that one corner.

## What an author does

1. `moss map bbox` prints the frame the site's places need, with a margin and a suggested deepest zoom. It reads only the folder.
2. The author finds a map. For a historical layer, that is a library's IIIF image with a georeference annotation, the small open file that ties points on the scan to points on the Earth (Allmaps publishes these under CC0). For a streets layer, it is an OpenStreetMap extract cut to the frame, one PMTiles file.
3. A fetch plugin copies what the frame needs into the site's folder: for a historical layer, each plate's annotation and only the part of its image the frame covers, at the resolution the deepest zoom needs. Fetching is the only step that uses the network, and it runs once, when the author asks.
4. The author declares the layer in `.moss/config.toml`. Pages stay plain markdown.

```toml
[map.layers.atlas-1930]
kind = "historical"          # or "streets"
source = "maps/atlas-1930"   # annotations and images, inside the site folder
max_zoom = 17
budget = "40 MB"             # the build refuses to emit more tiles than this
```

The credit comes from each source's own metadata (an annotation's provider, a PMTiles file's attribution). A `credit` key overrides it.

## The design

### Three layers, one engine

| Layer | Data | Where it lives |
|---|---|---|
| 0, the world | Natural Earth and GEBCO | compiled into moss, unchanged |
| 1, today's streets | an OpenStreetMap PMTiles extract | the site folder |
| 2, a historical map | georeference annotations and image regions | the site folder |

moss draws layers 1 and 2 at build time into the same kind of static tiles it draws layer 0 into today, for the frames the site's places need and no further. The published site needs no map service, no vector renderer and no WebGL, so it stays offline and archival like the rest of the site.

### City scale needs two engine changes first

Before any data, the explorer has to reach city scale and draw it true.

- **Zoom.** The explorer's deepest zoom is fixed at 0.005° per pixel. Inside a detail layer's extent it may go as deep as the layer's `max_zoom`; outside, the ceiling stays where it is, since there is nothing finer to show.
- **Projection.** The explorer draws everything in one world projection (Patterson). At 40° north it stretches east–west against north–south by about 10%, which is invisible on a continent and obvious on a street grid. A frame small enough to show streets is drawn in a frame-local projection, an equirectangular projection with its standard parallel at the frame's centre, which is close to true at city scale. The world view keeps Patterson.

### Pixels in the folder, provenance in the annotation

A georeference annotation points at the library's image service. If the site's map read the image from there, the site would depend on that service staying up, and a downloaded folder would show no map. So the pixels are copied into the folder, and the annotation, kept beside them, records where they came from. The build reads only local files. If a layer's image is missing, the build prints a note and draws layer 0 there.

### Warping a plate

For each plate, the build fits the transformation from ground coordinates to image pixels directly from the annotation's control points by least squares: polynomial order 1, 2 or 3, as the annotation asks. For every output pixel it looks up the source pixel and samples it. The annotation's mask clips each plate to its printed area, so a mosaic of plates shows no margins. Plates are drawn in annotation order. Thin-plate splines come later.

### Bounded size

Tiles are emitted at overview zooms across the frame, and at the deepest zooms only within a radius of each place. The total is checked against the layer's `budget`; over budget is an error under `--strict` and a warning otherwise, naming the layer and its size.

### Saying where a layer ends

Coverage is often partial. The layer ends along its plates' mask edges, over layer 0. The build prints notes for:

- places that fall outside every layer's footprint;
- plates whose footprint falls outside every frame (often a sign of a bad control point: the plate lands in the wrong borough);
- plates whose control points disagree by more than a threshold after fitting.

### Credit on the map

Some sources ask for credit on the map itself: OpenStreetMap asks for it "in a corner of the map", and libraries give credit lines. The map figure gets a credit slot, filled from each drawn layer's metadata. The full licence texts go into the map folder's `NOTICE.txt`, where Natural Earth and GEBCO are already credited. Layer 0 shows no corner credit, because its sources don't ask for one.

### Streets come last

Reading PMTiles and decoding vector tiles is a small, well-specified job. Drawing today's streets in moss's own style (water, parks, streets and blocks, labels) is the real design work, and the first site to need detail needs the historical layer. So layer 2 is built first.

## Licences, in short

| Source | What a site's folder must carry |
|---|---|
| Natural Earth | nothing (public domain) |
| GEBCO | an acknowledgement, already shipped in `NOTICE.txt` |
| OpenStreetMap | a visible credit on the map, and the extract under the Open Database License with its licence text |
| Historical scans | the holder's rights statement and credit line. Public-domain and CC0 scans are free to use. Some collections are licensed non-commercially and cannot go on a commercial site |
| Georeference annotations from Allmaps | nothing (CC0) |

Licence wording changes. The recipe in the moss skill tells authors to read each source's terms on its live page.

## Order of work

1. Small fixes to today's maps.
2. This proposal.
3. Engine: `moss map bbox`, the `[map.layers]` schema, the credit slot, deeper zoom inside a layer's extent, the frame-local projection.
4. The historical layer: the fetch plugin, the warper and mosaic, raster tiles under the explorer's markers, a cropped image in each page's small map, and the notes.
5. The streets layer: the PMTiles reader, the vector-tile decoder and a street style.

## Open questions

- Dark theme: a scanned atlas is a paper object. Should it keep its own colours in the dark theme, dimmed, or be toned to the theme?
- More than one historical layer (two editions of the same atlas): one drawn at a time, with a switch, or stacked by date?
