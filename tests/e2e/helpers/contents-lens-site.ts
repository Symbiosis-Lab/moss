/**
 * The scratch site the contents-lens render gate builds.
 *
 * One page with `contents: pictures`: six sections that each open with a
 * picture and a seventh with none. A control page has the same seven titles and
 * no field, to compare the ruler at rest against. A third page has forty
 * sections, a few with pictures. All text is invented.
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
export const MANY = 40;
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

const images = ["quay.png", "nets.png", "rope.png"];

const body = (withPictures: boolean): string =>
  LENS_SECTIONS.map(
    (title, i) =>
      `## ${title}\n\n${withPictures && LENS_WITH_IMAGE[i] ? `![${title}](${images[i % 3]})\n\n` : ""}${paragraphs(5)}`,
  ).join("\n\n");

const many = Array.from({ length: MANY }, (_, i) => {
  const title = `Chapter ${i + 1}`;
  return `## ${title}\n\n${i % 5 === 0 ? `![${title}](${images[i % 3]})\n\n` : ""}${paragraphs(3)}`;
}).join("\n\n");

const full = FULL_SECTIONS.map(
  (title, i) => `## ${title}\n\n${i < 5 ? `![${title}](${images[i % 3]})\n\n` : ""}${paragraphs(5)}`,
).join("\n\n");

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
    ".moss/config.toml": `schema_version = 5

[site]
lang = "en"
floating_nav = true
`,
  },
};
