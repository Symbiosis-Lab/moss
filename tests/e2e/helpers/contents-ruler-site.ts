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

export const CONTENTS_RULER_GATE: ScratchSiteSpec = {
  name: "contents-ruler-gate",
  files: {
    "cover.svg": COVER_SVG,
    "index.md": `---
title: Harbour Notes
uid: "crg00101"
breadcrumb: true
---

# Harbour Notes

Home page of an invented site for the contents-ruler render gate.
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
    ".moss/config.toml": `schema_version = 5

[site]
lang = "en"
floating_nav = true
`,
    ".moss/theme/style.css": null,
  },
};
