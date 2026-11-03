# Country maps: colour whole countries from a table

**Status:** Slice 1 is built; the later slices wait for a site build to show the gap.

A site that rates or measures countries (a travel-risk desk, an NGO's country programmes, a lab's field sites by country) wants a world map with each country coloured by a value it owns and updates often, a legend, and the same rows as text for phones, screen readers and search. moss draws landscape maps of places today; it has no country shapes and no way to colour by a value. A country map is drawn equal-area in Equal Earth, on flat land made from the country shapes themselves, unlike moss's place maps (Patterson, over the landscape); see "Why its own drawing, not the landscape map" below for the reasons.

## The author's side

The data is a page whose body is a markdown table, the form an author already reads and edits:

```markdown
---
title: Risk levels
date: 2026-11-03
country_map:
  by: Level
  levels: [Low, Guarded, Elevated, High]
---
| Country | Level | Note |
|---|---|---|
| Mexico | Elevated | Avoid border crossings at night. |
| Japan | Low | |
```

- `country_map:` marks the page as a country map. `by` names the column whose value colours the country; `levels` lists that column's values in order, lowest first, which is both the colour ramp and the legend. The first column names the country. Any other column (here `Note`) is shown when a reader chooses the country.
- Each item in `levels` is a plain string or `{name, text}`. `text` is that level's default advice ("Take normal precautions."): when a country's row has nothing in its other columns, the panel shows its level's `text` instead.
- An optional `unrated: "<text>"` is what the panel shows for a country that has no row. Choosing such a country (uncoloured land; disputed land stays inert) opens the panel with its name, a localised "Not rated" in place of the level, the `unrated` text if set, and the `action` link if set.
- `date:` is the day the values were last set. The map's stamp shows it ("As of 3 Nov 2026"), never the build day: a site rebuilt for another reason must not claim its data is new.
- The page renders as the map with its legend and stamp, followed by its own table. Any other page embeds it like any map: `![[Risk levels|style:map]]`, which draws the map, legend and stamp, followed by the rows grouped by level as a list.
- A cell may be blank: that country is listed in the text but not coloured. A country with no row is not coloured and not listed.

## What the build guarantees

- **Matching fails loudly.** The country column matches, ignoring case and accents, the ISO 3166 alpha-3 code, Natural Earth's `NAME`, `NAME_LONG`, `ADMIN` and `FORMAL_EN`, or a short alias list (UK, USA, UAE, Ivory Coast, Turkey, Czech Republic, Burma, Swaziland, Macedonia, Cape Verde, East Timor, Vatican). An unmatched name, a value not in `levels`, a country listed twice, and a name that is disputed or breakaway land are each a build problem naming the page, row and value: a warning, and a failure under `--strict`. A country silently left blank is the worst failure this feature can have.
- **No borders, no claims.** No boundary line is drawn. Land Natural Earth marks as disputed or breakaway is cut out of every country's shape and drawn as plain land, so colouring never assigns contested ground to anyone and moss needs no point-of-view setting. Neighbouring countries at the same level read as one area, as on any choropleth without outlines.
- **Offline, with its licence.** The shapes ship inside moss. Natural Earth is public domain; the map's `NOTICE.txt` names it as the other map assets do.

## Why its own drawing, not the landscape map

moss's place maps draw a landscape (relief, rivers, sea floor, city labels) in Patterson's projection. A country map is a thematic map, and three reasons give it its own drawing:

1. **Equal area.** A choropleth compares areas by colour, so the projection must not inflate them. Patterson enlarges high latitudes, where Canada, Russia and Greenland would carry more colour than their share. The country map uses Equal Earth (Šavrič, Patterson and Jenny, 2018), the equal-area world projection made for this use. The landscape maps keep Patterson.
2. **One coastline.** Colours drawn from country shapes over land drawn from the pack's separately simplified rings leave slivers along every coast. The country map draws its land as the countries themselves, so colour and coast share vertices.
3. **Legible colour.** Relief shading under a colour ramp changes how each level reads from place to place. The country map's land is one flat token.

Antarctica is not drawn and the frame runs from 84°N to 57°S, which removes a fifth of the height no country map of people's activity uses; a row naming Antarctica is a problem like any unmatched name. A faint graticule (every 30°) gives the projection its shape.

## Look and tokens

Inline SVG, one `<path>` per country, so light and dark themes follow live. Tokens, each with a light and dark default in moss's stylesheet:

- `--moss-country-land`: land with no value, and disputed land;
- `--moss-country-fill-low`, `--moss-country-fill-high`: the ends of the ramp; level *i* of *n* is mixed between them in OKLCH;
- `--moss-country-fill-1` … `--moss-country-fill-<n>`: set any one to override that step;
- `--moss-country-graticule`, and `--moss-country-chosen` for the chosen country's outline.

The tokens are read with their light default and a dark default under `[data-theme="dark"]`, so a site sets any of them on `:root`; `moss describe --json` lists them under `.custom_properties`. Every step must keep 3:1 against the land token in both themes (WCAG 1.4.11 non-text contrast; a test computes it for two to seven levels), and the legend names each level in text, so colour is never the only carrier. A map has at most seven levels, and a build problem names a page with more: more ordered steps than that cannot be told apart by colour. The default ramp is one warm hue, light to dark on the light theme and dark to light on the dark one, so order reads as magnitude; a test also holds adjacent steps apart in lightness.

## Choosing a country

A tap or click on a country, a click on a list row, and the address `#<country-slug>` all do the same thing: the country is outlined and a panel shows its name, level and other columns, with the as-of date. Esc, the panel's close button or a click on empty sea clears it. Keyboard users reach every country through the list, never through two hundred tab stops on the map. Without script, the list is the content and every row has its own anchor.

An optional `action` adds one link to the panel, with `{country}` replaced by the country's name, URL-encoded:

```yaml
country_map:
  by: Level
  levels: [Low, Guarded, Elevated, High]
  action: { label: "Request protection for this trip", link: "/book/?to={country}" }
```

## Slice 1

Data and matching, the shapes asset, Equal Earth, the tokens, the map with legend and stamp, the list, the panel with its optional action, the hash, problems and `--strict`, `moss describe` and the guide.

The shapes: Natural Earth 1:50m admin-0 countries and breakaway/disputed areas, pinned with their SHA-256 in `crates/moss-build/data/place-map/countries-source-manifest.toml` (a sibling of the landscape pack's manifest, so that pack's manifest digest is unchanged) and verified by `scripts/place-map/generate.py --check --countries` like the other sources, without needing the elevation grid. The generator subtracts disputed land, drops Antarctica, simplifies for a map about 1,300 px wide, quantises and writes `crates/moss-build/data/place-map/countries-v1.bin.br`, a sibling of the pack with its own budget and size report, so the pack's budget is untouched. Measured on a prototype: all 235 countries at this detail are about 119 KB as SVG paths and 33 KB brotli; the asset's budget is set from the generator's first measured run plus 5%, and only moves down. The first run measured 32,117 brotli bytes for 241 countries, so the budget is 33,723. Boolean operations use `polygon-clipping`, pinned in the generator's `package.json`; shared borders are simplified once so neighbours keep one common edge.

Done means: a fixture table of about fifty countries builds strict with no problem; a misspelt name, a value outside `levels`, a duplicate row and a disputed name each fail `--strict` naming the row; the figure, legend and list agree; both themes pass the contrast rule; the page works at 390 px and in WebKit; the stamp shows the page's `date:`.

## Later slices

Each waits for a site build to show the gap with a reproduction, and lands on its own:

- **Hover.** On devices that hover, a label beside the pointer ("NIGERIA · 3 ELEVATED") and a one-step lift of the colour; hovering a legend level lifts every country at it.
- **Framing.** Choosing a country frames its region, at most 2.5 times the world view, with the panel beside it, or as a bottom sheet on a phone.
- **Search.** A combobox over every country, its common names and major cities (Natural Earth populated places), with the level in each result, written by the build from the same sources.
- **Change email.** When a published table's values change, subscribers get the changed rows. This crosses into hosting and is designed with it.
