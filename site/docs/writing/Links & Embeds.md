---
title: Links & Embeds
url: wikilinks-and-embeds
uid: 5555db30
weight: 4
description: Wikilinks, embeds, cross-references, and bilingual content.
---

## Wikilinks

Link to any page in your site using double brackets:

```markdown
Read the [[structure]] page for details.
```

moss resolves the link by matching the text against filenames, titles, and slugs. No full path needed. `[[from-matters]]` finds `docs/start/from-matters.md` automatically.

**Display text:**

```markdown
See the [[structure|content structure docs]] for more.
```

**Link to a heading:**

```markdown
Check [[structure#Stems]] for the definition.
```

**Link to a block reference:**

```markdown
As defined in [[structure#^intro]].
```

### Resolution rules

When multiple files could match, moss picks the best one:

1. Exact filename match
2. Filename without extension
3. Folder note (self-named file)
4. Same language tree as the source page (e.g. `![[footer]]` from `zh-hans/about.md` prefers `zh-hans/footer.md` over root `footer.md`)
5. Closest directory first (files nearer to the linking page win remaining ties)

Explicit paths like `[[zh-hans/footer]]` always win over the language-tree preference.

## Image and media embeds

Embed an image with standard Markdown, or with a wikilink by putting `!` before the brackets. Both render the same:

```markdown
![](photos/photo.jpg)
![[photo.jpg]]
```

The standard form is the default: write the path relative to the page, and spell a space as `%20` (`![](my%20photo.jpg)`). If nothing is at that path, moss tries the same path from the site root, then falls back to the nearest file with that name, so a path that has gone stale still finds its file. The wikilink needs only the file name and searches the entire content tree, so no path is needed at all. The same two spellings work for video, audio, PDF and 3D model files (`![](clip.mp4)` or `![[clip.mp4]]`).

Put images in any folder, such as `assets/` or `images/`, and reference them from any markdown file.

```
work/
├── index.md         ← uses ![](assets/poster-farewell.webp)
└── assets/
    └── poster-farewell.webp   ← not a page; served as a static file
```

The language-tree preference (rule 4 above) applies to image resolution too: `![[photo.jpg]]` inside `zh-hans/about.md` prefers `zh-hans/photo.jpg` if it exists.

Every embed has both spellings, and they render the same: pages, sections, tables, notebooks and folders (`![](notes/essay.md)`, `![](notes/essay.md#Methods)`, `![](data/ledger.csv)`, `![](lab/analysis.ipynb)`, `![](journal/)`), the provider players for YouTube, Vimeo and CodePen URLs (`![](https://www.youtube.com/watch?v=…)`), and the `style:` keys. The text in the standard brackets is the text after the pipe, read by the same rule: `![640x360 loop](clip.mp4)` is `![[clip.mp4|640x360 loop]]`, and `![limit:2](journal/)` is `![[journal/|limit:2]]`. Where the pipe text is only a caption (an image, a page) the brackets can hold it too; on a page, table or notebook transclusion the text is ignored.

Three things stay wiki-only. A web address that no provider claims, such as `![[https://example.com/page]]`, becomes a frame around that page, while `![](https://example.com/page)` stays an image. An image whose brackets hold a display keyword (`![[photo.jpg|cover]]` crops the picture) captions it "cover" in the standard form, because the word is also a valid caption. And a reference-style image (`![text][ref]`) pointing at a page, table, notebook or folder stays an image.

**Pipe syntax** controls display — a fit/position keyword (`![[photo.jpg|contain top]]`), a size (`![[photo.jpg|400]]`), or a caption (`![[photo.jpg|Morning light]]`). See [[media#Display control]] and [[media#Captions]] for the full rules.

## Embeds

Pull content from another page inline using `!` before the brackets:

```markdown
![[structure#^intro]]
```

This inserts the referenced paragraph directly into the current page. Embeds work with:

- **Full page**: `![[media]]`: embeds the entire page content
- **Section**: `![[structure#URLs]]`: embeds everything under that heading
- **Block**: `![[structure#^intro]]`: embeds a single paragraph marked with a block ID
- **Folder listing**: `![[journal/]]`: embeds another folder's children as cards (see below)

moss detects circular embeds and stops before an infinite loop.

## Text from a file

Some plugins can read a kind of file you keep in the folder (a PDF, a slide deck, a spreadsheet) and hand back its text. Embed that text with the fragment name the plugin declares:

```markdown
![[annual-report.pdf#text]]
```

The text appears in the page as ordinary markdown, so it is searchable and can be linked to. While moss is still reading the file the page shows a placeholder, and if the plugin could not read it the page says so instead of failing the build. The text comes from a file someone else wrote, so moss shows HTML tags, `$` amounts and `[[brackets]]` in it as plain text.

To show every file in a folder as its own page, run `moss import reports/ --allow-plugins` (add `--recursive` to cover the folders beneath it too). moss writes a short page beside each file (`annual-report.md` next to `annual-report.pdf`) that embeds the file and takes its title and description from the file, and remembers the choice for that folder. It never overwrites a page you already have, and a file that another page already embeds gets none. `moss import --status` lists each folder, how many files it holds, and any file the plugin declined to read, with its reason. To keep the text and drop the file, run `moss expand reports/annual-report.md`: each line holding only an embed is replaced by the text it stands for.

A page can take its title and description from the first file it embeds instead of repeating them: write `inferred: [title, description]` in the frontmatter, and a `title:` or `description:` you write yourself still wins.

## HTML and interactive embeds

Embed an interactive or animated `.html` file (an animation, a small demo, a widget) with the same `![[...]]` syntax (a standard `![](folder-to-site.html)` that points at an HTML file in your site embeds it the same way):

```markdown
![[folder-to-site.html]]
```

moss serves the file as-is inside an iframe, so its own CSS and animation run normally. Keep the file in a folder such as `assets/` and reference it by bare filename. moss resolves the path and keeps the URL correct wherever the page is deployed. A query string passes straight through: `![[chart.html?series=a,b]]`.

This is the preferred way to embed interactive content. Do not hand-write a raw `<iframe>` or an absolute `/assets/...` URL. The wikilink computes the right relative path. The same `![[folder-to-site.html]]` resolves to a per-language copy via the language-tree rule: put the default file at `assets/animations/folder-to-site.html` and a translation at `zh-hant/assets/animations/folder-to-site.html`, and each page picks its own.

The embed fills the page width in a 16:9 frame, so design the file to read well at that shape.

## Maps

Place a map in the body with `style:map`. The embed stays where you write it, with the same width, alignment, and caption controls as other media:

```markdown
![[#|style:map|align-right 50%|Places in this article]]
![[Travel diary|style:map|wide]]
![[/places/kyoto/|style:map|align-left 40%|Kyoto]]
![[/places/|style:map|wide]]
```

`#` selects the current document. A named document selects that document's `location:` values; neither form transcludes its body. A place target ending in `/` keeps the existing place-map behavior: `/places/kyoto/` is that place's map, and the place root `/places/` is the whole site's map, so a home page can show every place. On any page but the places root itself, the root embed loads the explorer in a frame, with the static map as the fallback. A destination shows once, so when the home page embeds a place root's map and that root is also a nav item (`nav = true` on its `[terms.<key>]`, or its page has `nav: true`), the build prints a note naming both; nothing is removed. Article maps use all coordinate-bearing locations from the target's frontmatter, with the same precision and `route:` rules as its own map. A heading or block fragment does not select separate location data.

An explicit map of the current document replaces its automatic locator. The `map:` frontmatter switch and `[site].locator` setting still control automatic maps; they do not hide embeds you place in the body. If the target cannot be resolved or has no coordinate-bearing locations, moss reports an unavailable map instead of embedding the article text.

### Detail layers

Detail layers show a historical map or today's streets under a site's places. A historical layer is drawn at build time from georeferenced scans kept in the site's folder, into static raster tiles, so the published site needs no map service. Streets layers are not drawn yet: moss reads and checks the declaration and tells the places explorer about it.

Declare a layer in `.moss/config.toml`:

```toml
[map.layers.atlas-1930]
kind = "historical"          # or "streets"
source = "maps/atlas-1930"   # a folder inside the site; for a historical layer, its plates
bbox = [-74.03, 40.68, -73.90, 40.88]   # west, south, east, north, in degrees
max_zoom = 17                # 10 to 19, default 16
budget = "40 MB"             # a warning when the tiles total more; an error under --strict
credit = "Atlas of the City, 1930. Example Public Library"   # else the plates' own credit
annotations = ["https://annotations.allmaps.org/maps/<id>"]  # optional: the plates' order, first drawn first
```

`kind` and `source` are required. `bbox` is optional: without it the frame is taken from the places in `.moss/places.toml` that have coordinates. A wrong value, a `source` that is missing or outside the site, or a `max_zoom` outside 10 to 19 stops the build and names the table and key. When a site declares a layer, every map figure gets an empty credit line in its corner, and the build writes a small `layers.json` beside the map data for the explorer.

#### What a historical layer's folder holds

Each plate (one georeferenced map sheet) is three files in the layer's `source` folder, named by one id, usually an Allmaps map id. The build reads only these files and never the network, so it makes no difference how they got there: a fetch plugin, or the recipe below.

- `<id>.annotation.json` is the georeference annotation as Allmaps publishes it (IIIF Georeference Annotation). The build uses `target.selector`, an SvgSelector whose `<polygon points="x,y x,y ...">` outlines the plate's printed area in full-image pixels; `body.features`, the control points, each with `properties.resourceCoords` as `[x, y]` in full-image pixels and `geometry.coordinates` as `[longitude, latitude]`; and `body.transformation`, a polynomial of order 1, 2 or 3 (a `thinPlateSpline` is drawn as a first-order polynomial, with a note). `target.source.id` is the image service's address, and `target.source.provider` may name the holder.
- `<id>.jpg` is the image, whole or a region of it, at any size.
- `<id>.region.json` says which part of the full image the jpg shows. Its fields: `region` (required) is `[x, y, w, h]`, the part the jpg shows, in full-image pixels, where the full image is the size `target.source` gives, so the whole image is `[0, 0, width, height]`; `scale` (required) is the jpg's pixels per full-image pixel, so the jpg is `w * scale` pixels wide; `credit` is the holder's name, shown in the map's corner (when empty, the annotation's provider is used); `rights` is the rights statement or its address, copied into the site's notice; `source` is the image service's address (default: `target.source.id`); `annotation` and `image` name the other two files (default: `<id>.annotation.json` and `<id>.jpg`); `version` is `1`; `fetched` is a date for your own records.

A full-image pixel `(u, v)` is the pixel `((u - x) * scale, (v - y) * scale)` of the jpg. The control points, the outline and `region` are all in full-image pixels, so cropping or shrinking the jpg only means changing `region` and `scale` to match.

A plate that cannot be read (a file missing, invalid JSON, an outline or fewer than three control points that cannot be used) is a build warning naming the plate, and the other plates still draw. Plates are drawn in the order of `annotations` (matched by the id at the end of each address), then by id; where two overlap, the later one covers the earlier.

To make the three files by hand with only a shell and `curl`:

1. Find the plate's Allmaps annotation address, `https://annotations.allmaps.org/maps/<id>`, on the map's page at allmaps.org.
2. `curl -o <id>.annotation.json https://annotations.allmaps.org/maps/<id>`
3. Read `target.source.id` (the image service), `width` and `height` from it.
4. Choose a region (the whole image is fine: `[0, 0, width, height]`) and a scale. Pick the scale so the jpg is no larger than the deepest zoom needs; about 0.25 is a good start for a sheet 9000 pixels wide.
5. `curl -o <id>.jpg "<service>/<x>,<y>,<w>,<h>/<round(w*scale)>,/0/default.jpg"` (for a IIIF Image API 3 service; use `full` in place of the region for the whole image, and `/0/default.jpg` in the path for version 2 as well).
6. Write `<id>.region.json` yourself. The values come from the steps above: `source` and the sizes from the annotation, `region` and `scale` from the choice you made, and `credit` and `rights` from the holder's catalogue page.

```json
{
  "version": 1,
  "annotation": "95abed283977b2cc.annotation.json",
  "image": "95abed283977b2cc.jpg",
  "source": "https://iiif.example.org/iiif/3/5222333",
  "region": [0, 0, 8873, 6224],
  "scale": 0.25,
  "credit": "Example Public Library",
  "rights": "No known copyright restrictions",
  "fetched": "2026-10-22"
}
```

The rights statement belongs to the holder, not to Allmaps. Copy it from the holder's catalogue page for the sheet, and read the holder's terms on that live page before publishing: moss records what you give it and cannot check it.

What the build writes: tiles of 256 by 256 pixels as JPEG files under `_moss/map.<hash>/layers/<name>/<level>/<i>/<j>.jpg`, a `layers/<name>/index.json` listing the tiles and each plate's outline, and in `layers.json` the layer's `tiles` address, an extent that is the plates' outlines within `bbox`, and the credit (from `credit`, else the plates). The two deepest levels are drawn only within 300 m of a place, so `max_zoom` costs space only where places are. The credit, each plate's source address and its rights statement are added to `NOTICE.txt`. When the tiles total more than `budget`, the build warns, naming the layer, its size and the budget; with `--strict` that fails the build. A rebuild draws nothing again until a plate file, the layer's settings or a place's coordinates change. Notes (never failures) mention a place that lies outside every plate, a plate outside `bbox`, and a plate whose control points disagree with their fit by more than 30 m.

`moss map bbox [<folder>]` prints the frame your places need, as `west,south,east,north` with a margin of 10% of each side (at least 0.01 degree), and a suggested `max_zoom`: the zoom at which the frame's longer side spans about 1024 px, plus 3. It reads only the places that pages name in the place kind's fields, writes nothing, and takes `--json` for `{"bbox": [...], "max_zoom": n, "places": k}`.

## Folder listings

A wikilink whose path ends with `/` embeds a folder's children inline:

```markdown
# A Reader's Notebooks

A personal record of my reading and writing.

## Recent journal

![[journal/|limit:5,more]]

## Books
```

The trailing slash signals a folder embed, not a page. The cards inherit the target folder's `sort` (see [[frontmatter#Sort]]), so a `journal/` folder with dated entries renders date-sorted cards, and a `projects/` folder of dateless entries renders alphabetically.

**Pipe parameters** (comma-separated):

| Param | Meaning |
|---|---|
| `limit:N` | cap at N items |
| `more` | append a "More →" link to the source folder when truncated |
| `sort:date` / `sort:date-asc` / `sort:weight` / `sort:title` | override the source folder's sort axis for this embed only |

Examples:

```markdown
![[journal/]]                       # full listing
![[journal/|limit:5]]               # 5 most recent (or whatever the journal's sort picks)
![[journal/|limit:5,more]]          # 5 newest + "More →" link
![[news/|sort:date,limit:3]]        # override sort axis per-embed
```

Paths follow the same resolution as other wikilinks: relative paths anchor at the current page's folder, absolute paths (`/journal/`) anchor at the site root.

### When to use folder listings vs `children_source`

Two ways to render another folder's children on a page:

| Mechanism | When to use |
|---|---|
| Inline `![[folder/]]` wikilink | Multiple embeds per page, or you want the listing between other markdown sections. The author chooses where each listing appears. |
| `children_source: "[[folder]]"` in frontmatter | Page-scope: the page IS a listing of another folder. One transclusion per page. Renders at the slot determined by your theme. |

For a page with a single primary listing, `children_source` is the right choice. Use `![[folder/]]` when you want the listing inline among other content.

## Block references

Mark any paragraph with a `^block-id` at the end of the line:

```markdown
moss turns a folder of markdown files into a website. ^intro
```

Other pages can then link to or embed this paragraph using `[[page#^intro]]` or `![[page#^intro]]`. Block IDs are stable — they don't change when you reword the paragraph or rename the heading above it.

## Markdown links

Standard markdown links work as you'd expect:

```markdown
[Visit Example](https://example.com)
[About page](about.md)
[Section link](/docs/writing/structure/)
```

Both relative and absolute paths work. External links open in a new tab.

## Bilingual content

Create translations by appending a language suffix to the filename:

| File | Language |
|------|----------|
| `about.md` | site default |
| `about.zh-hans.md` | Simplified Chinese |
| `about.zh-hant.md` | Traditional Chinese |
| `about.en.md` | English (explicit) |

When translations exist, a language toggle appears on the page automatically.

### Language detection

moss resolves a page's language in this order:

1. `lang` frontmatter field
2. Filename suffix (`.zh-hans.md`)
3. Content auto-detection
4. Site default language

### Linking translations with different names

When translations have different filenames, use `translationKey` to connect them:

```yaml
# posts/hello.md
---
translationKey: hello-post
---

# posts/ni-hao.md
---
translationKey: hello-post
lang: zh-hans
---
```

Both files show a language toggle linking to each other.
