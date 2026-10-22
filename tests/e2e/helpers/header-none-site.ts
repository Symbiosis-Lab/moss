/**
 * The scratch sites the header-none render gate builds.
 *
 * ONE_PAGE_GATE: a site with `[site] header = "none"` whose footer ends on a
 * closing picture — a home page of five `##` collections, a full-width band
 * marked `contents=end`, and one more section after it, and a second long page
 * with no marker, whose contents ruler runs to the closing picture. Search is
 * on and the home page has a translation, so the controls are all three. BAND_GATE: the default header, with a long home page that
 * has a full-width band in the middle of its sections and a second page, so
 * the header, the island and the ruler's step-aside all have a normal site to
 * be checked on. All text is invented.
 * Served by playwright/header-none.config.ts.
 */
import type { ScratchSiteSpec } from "./scratch-site";
import { png } from "./contents-ruler-site";

/** The last title is longer than the island's old 16rem label box, and fits the bar. */
export const LONG_TITLE = "Landings of the Lantern Ferry, 1861–62";
export const COLLECTIONS = ["Quays", "Lanterns", "Nets", "Ropewalk", LONG_TITLE];
export const SITE_SUBTITLE = "An index of invented collections";
export const AFTER_END = "Colophon";
/** A page with no `contents=end`: its ruler ends at the closing picture. */
export const CLOSING_PATH = "/ledger/";
export const CLOSING_SECTIONS = ["Spring", "Summer", "Autumn", "Winter"];
/** A full-width page with `contents: pictures`: a tinted band whose words keep
 *  the text column after its second section, and a picture as wide as the
 *  window after its third. */
export const PICTURES_PATH = "/pictures/";
export const PICTURE_SECTIONS = ["Quays", "Lanterns", "Nets", "Ropewalk", "Salt"];
export const BAND_TINT = "rgb(40, 60, 90)";
/** The same field on a page set vertically, under a title longer than one
 *  column. */
export const UPRIGHT_PATH = "/upright/";

/** A page whose title is long enough to reach the controls on a phone. */
export const LONG_TITLE_PATH = "/landings/";

const FILLER =
  "The harbour wall holds the morning for a while before it lets the light " +
  "through, and the nets hung along it dry slowly in the damp. Nothing here " +
  "is in a hurry. People cross the quay with buckets and say very little.";

const paragraphs = (n: number): string =>
  Array.from({ length: n }, (_, i) => `${FILLER} ${"Tide and rope. ".repeat(i + 3)}`).join("\n\n");

const sections = (titles: string[]): string =>
  titles.map((title) => `## ${title}\n\n${paragraphs(4)}`).join("\n\n");

export const ONE_PAGE_GATE: ScratchSiteSpec = {
  name: "header-none-gate",
  files: {
    "index.md": `---
title: Shelf
subtitle: ${SITE_SUBTITLE}
uid: "hng00001"
---

A short welcome to the shelf.

${sections(COLLECTIONS)}

::: {.band screen contents=end}
Everything below is the shelf's own housekeeping.
:::

${sections([AFTER_END])}
`,
    "index.zh-hans.md": `---
title: 书架
uid: "hng00004"
---

书架的简短欢迎。
`,
    "Ledger.md": `---
title: Ledger
uid: "hng00003"
---

# Ledger

${sections(CLOSING_SECTIONS)}
`,
    "Landings.md": `---
title: The Landings of the Lantern Ferry at Every Harbour Light
uid: "hng00005"
---

${sections(["Morning", "Evening"])}
`,
    "Pictures.md": `---
title: Pictures
uid: "hng00006"
contents: pictures
content_width: full
---

${PICTURE_SECTIONS.map(
  (title, i) =>
    `## ${title}\n\n![${title}](closing.png)\n\n${paragraphs(4)}${
      i === 1 ? "\n\n::: {.tinted screen}\nA band as wide as the window, its words on the text column.\n:::" : ""
    }${i === 2 ? "\n\n![[closing.png|screen]]" : ""}`,
).join("\n\n")}
`,
    "Upright.md": `---
title: 書架上的每一件藏品都有它自己的來歷從港口的晨光到燈塔的守夜人再到繩索工坊裡的長廊
uid: "hng00007"
contents: pictures
typesetting: vertical
---

${PICTURE_SECTIONS.slice(0, 3).map((title) => `## ${title}\n\n![${title}](closing.png)\n\n${paragraphs(1)}`).join("\n\n")}
`,
    ".moss/theme/style.css": `.tinted { background: ${BAND_TINT}; color: white; padding-block: 6rem; }
`,
    "closing.png": png(1200, 600, [70, 110, 150]),
    "footer.md": `:::hero {image=closing.png align=top}
## The shelf is open.
:::
`,
    // One section only: no contents table, so the island is controls alone.
    "short.md": `---
title: Short
uid: "hng00002"
---

# Short

${sections(["Only"])}
`,
    ".moss/config.toml": `schema_version = 5

[site]
lang = "en"
header = "none"
search = true
`,
  },
};

export const BAND_SECTIONS = ["Arrival", "Ferry", "Salt", "Lamp", "Ledger", "Winter", "Afterword"];

export const BAND_PATH = "/essays/reading/long/";

export const BAND_GATE: ScratchSiteSpec = {
  name: "header-none-band-gate",
  files: {
    "index.md": `---
title: Harbour Notes
uid: "hng00101"
breadcrumb: true
---

# Harbour Notes

${paragraphs(2)}
`,
    "Essays/Essays.md": `---
title: Essays
uid: "hng00102"
---

# Essays
`,
    "Essays/Reading/Reading.md": `---
title: Reading
uid: "hng00105"
---

# Reading
`,
    "Essays/Reading/Long.md": `---
title: Reading the coast
uid: "hng00103"
---

# Reading the coast

${sections(BAND_SECTIONS.slice(0, 3))}

::: {.band screen}
A band as wide as the window.
:::

${sections(BAND_SECTIONS.slice(3, -1))}

![[quay.png|screen]]

## ${BAND_SECTIONS[BAND_SECTIONS.length - 1]} {contents=end}

${paragraphs(8)}
`,
    "quay.png": png(1200, 600, [60, 90, 140]),
    "about.md": `---
title: About
uid: "hng00104"
nav: true
---

# About

${paragraphs(2)}
`,
    ".moss/config.toml": `schema_version = 5

[site]
lang = "en"
floating_nav = true
`,
  },
};
