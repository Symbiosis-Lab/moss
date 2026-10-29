/**
 * The scratch site the contents-lens render gate builds.
 *
 * One page with `contents: pictures`: six sections that each open with a
 * picture (one cropped to a focal point) and a seventh with none. A control
 * page has the same seven titles and no field, to compare the ruler at rest
 * against. A third page has forty sections, a few with pictures. All text is
 * invented.
 * Served by playwright/contents-lens.config.ts.
 */
import type { ScratchSiteSpec } from "./scratch-site";
import { paragraphs, png } from "./contents-ruler-site";

export const LENS_SECTIONS = [
  "Quays at first light",
  "Lanterns and their keepers",
  "Nets along the harbour",
  "The ropewalk",
  "Salt flats in winter",
  "The ferry crossing",
  "Afterword",
];
/** The last section has no picture. */
export const LENS_WITH_IMAGE = [true, true, true, true, true, true, false];
/** The section whose picture is cropped to a focal point, and that point. */
export const FOCAL_ROW = 2;
export const FOCUS_POINT = "20% 70%";
export const MANY = 40;
/** A band under the open list, light in the light theme and dark in the dark one. */
export const BAND_LIGHT = "rgb(214, 226, 240)";
export const BAND_DARK = "rgb(30, 50, 80)";
/** The same bands in a colour syntax a pattern match would miss. */
export const BAND_OKLCH_LIGHT = "oklch(0.9 0.04 250)";
export const BAND_OKLCH_DARK = "oklch(0.3 0.05 250)";
/** A page that fills the window (`content_width: full`), so its left margin is
 *  as narrow as a page's gets, with titles that do not fit it on one line. */
export const FULL_SECTIONS = [
  "Forty-one Landings of the Lantern Ferry, 1861–62",
  "Quays at first light",
  "Nets along the harbour and the ropewalk beyond it",
  "The long road of the lantern ferry from the old harbour wall to the last light on the far shore",
  "Salt flats in winter",
  "Afterword",
];

/** A page whose second section's picture will not load, and whose first
 *  section has a short name of its own. */
export const BROKEN_SECTIONS = ["Opening hours of the reading room", "In the works", "Afterword"];
export const SHORT_NAME = "Opening hours";

const images = ["quay.png", "nets.png", "rope.png"];

const picture = (title: string, i: number): string =>
  i === FOCAL_ROW ? `![[${images[i % 3]}|focus:${FOCUS_POINT.replace(" ", ",")}]]` : `![${title}](${images[i % 3]})`;

const body = (withPictures: boolean): string =>
  LENS_SECTIONS.map(
    (title, i) =>
      `## ${title}\n\n${withPictures && LENS_WITH_IMAGE[i] ? `${picture(title, i)}\n\n` : ""}${paragraphs(5)}`,
  ).join("\n\n");

const many = Array.from({ length: MANY }, (_, i) => {
  const title = `Chapter ${i + 1}`;
  return `## ${title}\n\n${i % 5 === 0 ? `![${title}](${images[i % 3]})\n\n` : ""}${paragraphs(3)}`;
}).join("\n\n");

const full = FULL_SECTIONS.map(
  (title, i) => `## ${title}\n\n${i < 5 ? `![${title}](${images[i % 3]})\n\n` : ""}${paragraphs(5)}`,
).join("\n\n");

const TINTED_CSS = `.tinted { background: ${BAND_LIGHT}; padding-block: 60vh; }\n:root[data-theme="dark"] .tinted { background: ${BAND_DARK}; }\n.tinted-oklch { background: ${BAND_OKLCH_LIGHT}; padding-block: 60vh; }\n:root[data-theme="dark"] .tinted-oklch { background: ${BAND_OKLCH_DARK}; }\n`;

export const CONTENTS_LENS_GATE: ScratchSiteSpec = {
  name: "contents-lens-gate",
  files: {
    "quay.png": png(1000, 700, [60, 90, 140]),
    "nets.png": png(1000, 700, [140, 90, 60]),
    "rope.png": png(1000, 700, [90, 140, 60]),
    "index.md": `---
title: Harbour Notes
uid: "ctl00101"
---

# Harbour Notes

An invented site for the contents-lens render gate.
`,
    "Essays/Essays.md": `---
title: Essays
uid: "ctl00102"
---

# Essays
`,
    "Essays/Lens.md": `---
title: Collections
uid: "ctl00103"
contents: pictures
---

${body(true)}
`,
    "Essays/Plain.md": `---
title: Plain
uid: "ctl00104"
---

${body(false)}
`,
    "Essays/Band.md": `---
title: Band
uid: "ctl00108"
contents: pictures
---

${LENS_SECTIONS.map(
  (title, i) =>
    `## ${title}\n\n${paragraphs(4)}${i === 1 ? "\n\n::: {.tinted screen}\nA band as wide as the window, its words on the text column.\n:::" : ""}`,
).join("\n\n")}
`,
    "Essays/BandOklch.md": `---
title: Band in oklch
uid: "ctl00109"
contents: pictures
---

${LENS_SECTIONS.map(
  (title, i) =>
    `## ${title}\n\n${paragraphs(4)}${i === 1 ? "\n\n::: {.tinted-oklch screen}\nA band as wide as the window, its words on the text column.\n:::" : ""}`,
).join("\n\n")}
`,
    ".moss/theme/style.css": TINTED_CSS,
    "Essays/Many.md": `---
title: Many
uid: "ctl00105"
contents: pictures
---

${many}
`,
    "Essays/Full.md": `---
title: Full
uid: "ctl00106"
contents: pictures
content_width: full
---

${full}
`,
    "Essays/Broken.md": `---
title: Broken
uid: "ctl00107"
contents: pictures
---

## ${BROKEN_SECTIONS[0]} {#hours contents-label="${SHORT_NAME}"}

![Quay](quay.png)

${paragraphs(4)}

## ${BROKEN_SECTIONS[1]}

<img src="gone.png" alt="">

${paragraphs(4)}

## ${BROKEN_SECTIONS[2]}

${paragraphs(4)}
`,
    ".moss/config.toml": `schema_version = 5

[site]
lang = "en"
floating_nav = true
`,
  },
};

/** The same site under a theme whose dark text colour is too faint on the dark
 *  page to hold 4.6:1, so the dark theme's names keep the secondary text colour
 *  while the light theme gets a floor of its own. */
export const CONTENTS_LENS_FALLBACK_GATE: ScratchSiteSpec = {
  name: "contents-lens-fallback-gate",
  files: {
    ...CONTENTS_LENS_GATE.files,
    ".moss/theme/style.css": `:root[data-theme="dark"] { --moss-color-text: #5a5650; }\n${TINTED_CSS}`,
  },
};
