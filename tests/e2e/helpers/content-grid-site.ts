/**
 * The scratch sites the content-grid render gate builds: a page with a
 * full-width band and a footer.md that closes with a picture, once with the
 * default content width and once with `content_width: full`. The text column,
 * the band's text and the closing picture's words are measured against each
 * other. All content is invented. Served by playwright/content-grid.config.ts.
 */
import { CREDIT, HEADING, LINK_TEXT, landscapePng, SKY, GROUND, SKY_NIGHT } from "./closing-picture-site";
import type { ScratchSiteSpec } from "./scratch-site";

export const BAND_TEXT = "A band as wide as the window, with a line of its own.";
export const BODY_TEXT = "The harbour wall holds the morning for a while before it lets the light through.";

const page = (title: string, uid: string, frontmatter: string): string => `---
title: ${title}
uid: "${uid}"
${frontmatter}---

# ${title}

${BODY_TEXT}[^1]

::: {.band screen}
${BAND_TEXT}
:::

${BODY_TEXT}

[^1]: A note, so the page reserves room for sidenotes.
`;

const site = (name: string, uid: string, frontmatter: string, align = "top"): ScratchSiteSpec => ({
  name,
  files: {
    ".moss/config.toml": `schema_version = 5\n\n[site]\nlang = "en"\n`,
    "closing.png": landscapePng(1600, 700, SKY, GROUND, 0.7),
    "closing-dark.png": landscapePng(1600, 700, SKY_NIGHT, [8, 10, 14], 0.7),
    "index.md": page("Grid Shelf", uid, frontmatter),
    // Not the home page: only an inner page reserves room for sidenotes.
    "Notes.md": page("Notes", `${uid}n`, frontmatter),
    "footer.md": `:::hero {image=closing.png dark=closing-dark.png align=${align} caption="${CREDIT}"}\n## ${HEADING}\n\n[${LINK_TEXT}](https://example.org/new)\n:::\n`,
  },
});

export const DEFAULT_WIDTH_GATE = site("content-grid-default-gate", "cgd00101", "");
export const FULL_WIDTH_GATE = site("content-grid-full-gate", "cgf00101", "content_width: full\n");
export const END_ALIGN_GATE = site("content-grid-end-gate", "cge00101", "", "end");
export const CENTER_ALIGN_GATE = site("content-grid-center-gate", "cgc00101", "", "center");
