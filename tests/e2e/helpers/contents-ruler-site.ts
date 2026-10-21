/**
 * The scratch site the contents-ruler render gate builds.
 *
 * One long essay, three folders deep, with everything the ruler has to cope
 * with: fourteen sections, a heading in CJK of more than forty characters, a
 * very short one, a long Latin one, one with an astral character, a cover
 * image above the article, and footnotes (which, on a wide window, move the
 * text column and so the room the ruler has). A second, short article in the
 * same folder is the target the morph test navigates to. All text is invented.
 * Served by playwright/contents-ruler.config.ts.
 */
import { deflateSync } from "node:zlib";
import type { ScratchSiteSpec } from "./scratch-site";

export const LONG_CJK_TITLE =
  "從港口到山腳的長路：關於一座小鎮在三個季節裡如何被遺忘、被記起、又被重新命名的漫長故事（上篇）";
export const LONG_LATIN_TITLE =
  "How a small harbour town was forgotten, remembered, and renamed again over three long seasons of fog";
export const SHORT_TITLE = "Notes";
export const ASTRAL_TITLE = "𠮷野橋邊的早市與𠮷祥話";

/** The long essay's section titles, in order. */
export const SECTIONS: string[] = [
  "Arrival",
  LONG_CJK_TITLE,
  "The ferry timetable",
  SHORT_TITLE,
  LONG_LATIN_TITLE,
  "Salt and rope",
  ASTRAL_TITLE,
  "An afternoon in the net loft",
  "What the fog hides",
  "The lamp keeper's ledger",
  "Weather and its names",
  "Winter quarters",
  "The last boat in",
  "Afterword",
];

/** The short article's, for the morph test. */
export const SHORT_SECTIONS = ["Alpha", "Beta", "Gamma"];

const FILLER =
  "The harbour wall holds the morning for a while before it lets the light " +
  "through, and the nets hung along it dry slowly in the damp. Nothing here " +
  "is in a hurry. People cross the quay with buckets and say very little, " +
  "and the gulls, who say a great deal, are mostly ignored.";

const paragraphs = (n: number): string =>
  Array.from({ length: n }, (_, i) => `${FILLER} ${"Tide and rope. ".repeat(i + 3)}`).join("\n\n");

const longBody = SECTIONS.map((title, i) => {
  const note = i === 2 ? "\n\nA remark with a note[^1]." : i === 6 ? "\n\nAnother remark[^2]." : "";
  return `## ${title}\n\n${paragraphs(4)}${note}`;
}).join("\n\n");

const COVER_SVG = `<svg xmlns="http://www.w3.org/2000/svg" width="1600" height="1100" viewBox="0 0 1600 1100"><rect width="1600" height="1100" fill="#456"/></svg>
`;

/** The opted-in page's section titles, in order. */
export const OPEN_SECTIONS = [
  "The quays at first light, 1820",
  "Lanterns and the lamp keepers",
  "Nets drying along the harbour",
  "The ropewalk through the town",
  "Afterword on the whole coast",
];
/** Which of them hold an image: the ropewalk's is inside a subsection. */
export const OPEN_WITH_IMAGE = [true, false, true, true, false];

const CRC = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
export function chunk(type: string, data: Buffer): Buffer {
  const body = Buffer.concat([Buffer.from(type), data]);
  let crc = 0xffffffff;
  for (const byte of body) crc = CRC[(crc ^ byte) & 0xff] ^ (crc >>> 8);
  const head = Buffer.alloc(4);
  head.writeUInt32BE(data.length);
  const tail = Buffer.alloc(4);
  tail.writeUInt32BE((crc ^ 0xffffffff) >>> 0);
  return Buffer.concat([head, body, tail]);
}

/** A flat-colour PNG wider than moss's first ladder rung (800), so the page
 *  carries a `srcset` and the choice of thumbnail has something to choose. */
function png(width: number, height: number, [r, g, b]: [number, number, number]): Buffer {
  const row = Buffer.alloc(1 + width * 3);
  for (let x = 0; x < width; x++) row.set([r, g, b], 1 + x * 3);
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr.set([8, 2, 0, 0, 0], 8);
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(Buffer.concat(Array.from({ length: height }, () => row)))),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

const openBody = [
  `## ${OPEN_SECTIONS[0]}\n\n![The quay at dawn](quay.png)\n\n${paragraphs(5)}`,
  `## ${OPEN_SECTIONS[1]}\n\n${paragraphs(5)}`,
  `## ${OPEN_SECTIONS[2]}\n\n${paragraphs(2)}\n\n![Nets drying](nets.png)\n\n${paragraphs(4)}`,
  `## ${OPEN_SECTIONS[3]}\n\n${paragraphs(2)}\n\n### The long walk\n\n![The ropewalk](rope.png)\n\n${paragraphs(4)}`,
  `## ${OPEN_SECTIONS[4]}\n\n${paragraphs(6)}`,
].join("\n\n");

export const CONTENTS_RULER_GATE: ScratchSiteSpec = {
  name: "contents-ruler-gate",
  files: {
    "cover.svg": COVER_SVG,
    "quay.png": png(1000, 700, [60, 90, 140]),
    "nets.png": png(1000, 700, [140, 90, 60]),
    "rope.png": png(1000, 700, [90, 140, 60]),
    "index.md": `---
title: Harbour Notes
uid: "crg00101"
breadcrumb: true
---

# Harbour Notes

Home page of an invented site for the contents-ruler render gate.

${["Harbour", "Market"].map((t) => `## ${t}\n\n${paragraphs(4)}`).join("\n\n")}
`,
    // Enough footer that the article can scroll above the middle of a window:
    // "once past the article" has to be a place a reader can actually reach.
    "footer.md": `${paragraphs(5)}\n`,
    "Essays/Essays.md": `---
title: Essays
uid: "crg00102"
---

# Essays
`,
    "Essays/Reading/Reading.md": `---
title: Reading the coast
uid: "crg00103"
---

# Reading the coast
`,
    "Essays/Reading/Long.md": `---
title: "Reading the coast slowly: field notes from a season of fog, ferries and nets drying in the damp"
uid: "crg00104"
---

:::hero {image=../../cover.svg}
:::

${paragraphs(6)}

${longBody}

[^1]: The first note, a line or two so that it has some width in the margin.
[^2]: The second note, likewise.
`,
    "Essays/Reading/Short.md": `---
title: A short one
uid: "crg00105"
---

${SHORT_SECTIONS.map((t) => `## ${t}\n\n${paragraphs(3)}`).join("\n\n")}
`,
    "Essays/Reading/Full.md": `---
title: Full width collections
uid: "crg00107"
contents: open
content_width: full
---

:::hero {image=../../cover.svg}
:::

${paragraphs(2)}

:::{.band screen}
A band as wide as the window.
:::

${openBody}
`,
    // Short titles: the column is the list's own width, so it is narrow.
    "Essays/Reading/Brief.md": `---
title: Brief
uid: "crg00108"
contents: open
content_width: full
---

${["Quays", "Nets", "Ropes"].map((t) => `## ${t}\n\n${paragraphs(4)}`).join("\n\n")}
`,
    // One title far longer than the column may be: it is cut at the cap.
    "Essays/Reading/Capped.md": `---
title: Capped
uid: "crg00109"
contents: open
---

${[LONG_LATIN_TITLE, "Nets", "Ropes"].map((t) => `## ${t}\n\n${paragraphs(4)}`).join("\n\n")}
`,
    "Essays/Reading/Boxed.md": `---
title: Boxed
uid: "crg00110"
contents: open
---

${OPEN_SECTIONS.slice(0, 3).map((t) => `::: {.boxed}\n## ${t}\n\n${paragraphs(3)}\n:::`).join("\n\n")}
`,
    "Essays/Reading/Open.md": `---
title: Collections
uid: "crg00106"
contents: open
---

${paragraphs(2)}

${openBody}
`,
    ".moss/config.toml": `schema_version = 5

[site]
lang = "en"
floating_nav = true
`,
    // A section in a panel that reaches past the text on both sides.
    ".moss/theme/style.css": ".boxed { margin-inline: -40px; padding: 24px 40px; background: #eee; }\n",
  },
};

/** A site whose HOME page asks for its contents open, each section opening
 *  with a grid of pictures in a wrapping div, as a collection page does. */
export const CONTENTS_HOME_GATE: ScratchSiteSpec = {
  name: "contents-home-gate",
  files: {
    "quay.png": png(1000, 700, [60, 90, 140]),
    "nets.png": png(1000, 700, [140, 90, 60]),
    "rope.png": png(1000, 700, [90, 140, 60]),
    "index.md": `---
title: Shelf
uid: "crg00201"
contents: open
---

# Shelf

${OPEN_SECTIONS.map(
  (title, i) =>
    `::: {.collection}\n## ${title}\n\n${
      OPEN_WITH_IMAGE[i] ? `:::grid 2\n![One](quay.png)\n+++\n![Two](nets.png)\n:::\n\n` : ""
    }${paragraphs(3)}\n:::`,
).join("\n\n")}
`,
    ".moss/config.toml": `schema_version = 5

[site]
lang = "en"
floating_nav = true
`,
    ".moss/theme/style.css": null,
  },
};
