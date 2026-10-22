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

## ${BAND_SECTIONS[BAND_SECTIONS.length - 1]} {contents=end}

${paragraphs(8)}
`,
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
