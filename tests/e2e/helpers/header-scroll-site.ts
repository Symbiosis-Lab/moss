/**
 * The scratch site the header-scroll render gate builds: `header_scroll =
 * "hide"` (and, in a twin, "sticky") over one long page with a heading to jump to and a link that jumps
 * there. A second page keeps a nav link in the header; a third opens with a hero the header is laid over. All text is invented.
 * Served by playwright/header-scroll.config.ts.
 */
import type { ScratchSiteSpec } from "./scratch-site";

const FILLER =
  "The harbour wall holds the morning for a while before it lets the light " +
  "through, and the nets hung along it dry slowly in the damp. Nothing here " +
  "is in a hurry. People cross the quay with buckets and say very little.";

const paragraphs = (n: number): string =>
  Array.from({ length: n }, (_, i) => `${FILLER} ${"Tide and rope. ".repeat(i + 3)}`).join("\n\n");

export const TARGET_ID = "salt";

export const COVER_PATH = "/cover/";

const gateSite = (name: string, mode: "hide" | "sticky"): ScratchSiteSpec => ({
  name,
  files: {
    "index.md": `---
title: Harbour Notes
uid: "hsg00001"
---

[Jump to salt](#${TARGET_ID})

## Quays

${paragraphs(12)}

## Salt

${paragraphs(12)}

## Lamps

${paragraphs(12)}
`,
    "Cover.md": `---
title: Cover
uid: "hsg00003"
listed: false
---

:::hero {header=over}
Cover words
:::

${paragraphs(14)}
`,
    "Ledger.md": `---
title: Ledger
uid: "hsg00002"
---

${paragraphs(3)}
`,
    ".moss/config.toml": `schema_version = 5

[site]
lang = "en"
header_scroll = "${mode}"
floating_nav = true
`,
  },
});

export const HEADER_SCROLL_GATE = gateSite("header-scroll-gate", "hide");
export const HEADER_SCROLL_STICKY_GATE = gateSite("header-scroll-sticky-gate", "sticky");
