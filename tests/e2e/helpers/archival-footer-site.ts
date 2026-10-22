/**
 * The scratch site the archival-footer render gate builds: `[site] archival`
 * with a site author and a licence, a home page, and a page with an author of
 * its own. No footer.md and no footer links, so the edition block is the
 * footer's only content. All content is invented. Served by
 * playwright/archival-footer.config.ts.
 */
import type { ScratchSiteSpec } from "./scratch-site";

export const ARCHIVAL_FOOTER_GATE: ScratchSiteSpec = {
  name: "archival-footer-gate",
  files: {
    ".moss/config.toml": `schema_version = 5

[site]
lang = "en"
archival = true
author = "Site Keeper"
license = "No known copyright restrictions."
`,
    "index.md": `---
title: Harbour Prints
uid: "afg00101"
---

A small collection of harbour photographs.
`,
    "prints/pier.md": `---
title: The Pier at Dawn
uid: "afg00102"
author: Ada Example
---

A photograph of a pier.
`,
  },
};
