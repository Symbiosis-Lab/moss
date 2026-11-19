/**
 * The scratch site the header-over-band render gate builds: pages that open
 * with a full-width band of three pictures (the middle one a link) and the
 * page's h1, `header=over` on the band, over dark, pale and mixed pictures. The
 * theme lays the pictures side by side. Served by playwright/header-over-band.config.ts.
 */
import type { ScratchSiteSpec } from "./scratch-site";
import { lined } from "./header-over-ink-site";

/** Page path, and the ink the build should choose for it. */
export const BAND_PAGES = [
  { path: "/", name: "dark", ink: "light" },
  { path: "/pale/", name: "pale", ink: "dark" },
  { path: "/mixed/", name: "mixed", ink: "none" },
] as const;

const page = (title: string, uid: string, pictures: [string, string, string]) => `---
title: ${title}
uid: "${uid}"
---

::: {screen header=over}
![Harbour at dusk](${pictures[0]})

[![The old mill](${pictures[1]})
**The old mill**](/about/)

![The weir](${pictures[2]})

# A river town
:::

Body text below the band.
`;

export const HEADER_OVER_BAND_GATE: ScratchSiteSpec = {
  name: "header-over-band-gate",
  files: {
    "dark.png": lined(900, 600, 24, 44),
    "pale.png": lined(900, 600, 246, 228),
    "index.md": page("River town", "hob00001", ["dark.png", "dark.png", "dark.png"]),
    "pale.md": page("Pale", "hob00002", ["pale.png", "pale.png", "pale.png"]),
    "mixed.md": page("Mixed", "hob00003", ["dark.png", "pale.png", "dark.png"]),
    "later.md": `---
title: Later
uid: "hob00005"
---

Words first.

::: {screen header=over}
![Harbour at dusk](dark.png)
:::
`,
    "hero.md": `---
title: Hero
uid: "hob00006"
---

:::hero {image=dark.png header=over}
# A river town
:::

Body text below the hero.
`,
    "about.md": `---
title: About
uid: "hob00004"
---

About this invented site.
`,
    ".moss/config.toml": `schema_version = 5

[site]
lang = "en"
`,
    ".moss/theme/style.css": `article.container > div[data-width="screen"] {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  padding: 0;
}
article.container > div[data-width="screen"] > :is(p, figure, h1) {
  max-inline-size: none;
  margin: 0;
}
article.container > div[data-width="screen"] img {
  display: block;
  inline-size: 100%;
  block-size: 18rem;
  object-fit: cover;
}
article.container > div[data-width="screen"] > h1 {
  grid-column: 1 / -1;
  padding: 1rem;
}
`,
  },
};
