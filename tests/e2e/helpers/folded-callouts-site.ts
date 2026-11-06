/**
 * The scratch site the folded-callouts render gate builds: one page with a
 * group of folded callouts (`::: {accordion}`) and one folded callout outside
 * it. All text is invented. Served by playwright/folded-callouts.config.ts.
 */
import type { ScratchSiteSpec } from "./scratch-site";

export const FOLDED_CALLOUTS_GATE: ScratchSiteSpec = {
  name: "folded-callouts-gate",
  files: {
    "index.md": `---
title: Ferry questions
uid: "fcg00001"
---

::: {accordion}

> [!faq]- How long is the crossing?
> About twenty minutes, tide permitting. The ferry leaves from the north quay every half hour through the morning, and the last boat waits for no one.

> [!faq]- Can I bring a bicycle?
> Yes. Bicycles ride on the lower deck, and the crew will help you lift one over the sill if the tide is low.

:::

> [!faq]- Is there a cafe on board?
> A kettle and a tin of biscuits, mostly.
`,
    ".moss/config.toml": `schema_version = 5

[site]
lang = "en"
`,
  },
};
