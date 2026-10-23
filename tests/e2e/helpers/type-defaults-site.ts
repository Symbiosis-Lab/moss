/**
 * The scratch site the type-defaults render gate builds: a headerless site
 * (so the title keeps room for the theme controls) whose pages each isolate one
 * default that a hand-built design pinned down: the spacing of a captioned
 * picture in a grid card, the colour of a link, a year range in a heading, a
 * subtitle beside the title, a grid of equal rows, and a quote in Chinese or
 * Japanese in a collage. All names and text are invented.
 * Served by playwright/type-defaults.config.ts.
 */
import type { ScratchSiteSpec } from "./scratch-site";

const svg = (w: number, h: number, fill: string): string =>
  `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}"><rect width="${w}" height="${h}" fill="${fill}"/></svg>\n`;

export const RANGE_TITLE = "Maren Oduya of the Lantern Coast, 1626–1705";
export const LONG_NAME = "Bartholomew Wickersham-Okonkwo";
export const SUBTITLE = "A painter of invented birds";
export const LONG_TITLE =
  "The Complete Correspondence of the Harbour Board with the Lighthouse Keepers of the Northern Coast, Their Wives, Their Sons and the Ferrymen";
export const CJK_QUOTE = "海天一色，燈塔獨明。";
export const KANA_QUOTE = "朝の港に霧が降りて、船は静かに眠る。";
export const LONG_CJK_QUOTE = "潮聲入夢，漁火星沉。".repeat(16);
export const LATIN_QUOTE = "Communication is health; communication is truth.";

const card = (n: number, title: string, text: string, extra = ""): string =>
  `![Picture ${n}](p${n}.svg)\n*Caption ${n}*\n\n### ${title}\n\n${text}${extra}`;

/** Three cards of different shapes: a short one, one with a long title and text, one with a list too. */
const CARDS = [
  card(1, "Quays", "Short."),
  card(2, "The ferry that crossed the estuary at night", "A longer text that takes several lines where the others take one, so the cards differ in height.", "\n\n- a list\n- of two"),
  `### No picture\n\nA card without one.`,
].join("\n+++\n");

export const TYPE_DEFAULTS_GATE: ScratchSiteSpec = {
  name: "type-defaults-gate",
  files: {
    "p1.svg": svg(600, 400, "#4a7"),
    "p2.svg": svg(400, 600, "#a47"),
    "p3.svg": svg(500, 500, "#47a"),
    "p4.svg": svg(800, 300, "#a74"),
    "index.md": `---
title: Shelf
subtitle: ${SUBTITLE}
uid: "tdg00001"
---

A short welcome.
`,
    "cards.md": `---
title: Cards
uid: "tdg00002"
---

:::grid 2
${card(1, "One", "Text one.")}
+++
[${card(2, "Two", "Text two.")}](/links/)
:::
`,
    "links.md": `---
title: Links
uid: "tdg00003"
---

A paragraph with [a link](/cards/) in it.
`,
    ".moss/config.toml": `schema_version = 5

[site]
lang = "en"
header = "none"
`,
    // The subtitle stands beside the title.
    ".moss/theme/style.css": `.moss-folder-title, .moss-article-title, .moss-subtitle { display: inline; margin-block: 0; }
`,
  },
};
