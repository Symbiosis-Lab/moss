/**
 * The scratch site the subscribed-state render gate builds: one page with a
 * `:::subscribe` block, and a site id, so the form is wired rather than
 * pending. All content is invented. Served by
 * tests/render-gates/site/subscribed-state.spec.ts.
 */
import type { ScratchSiteSpec } from "./scratch-site";

export const SUBSCRIBED_STATE_GATE: ScratchSiteSpec = {
  name: "subscribed-state-gate",
  files: {
    ".moss/state.toml": `[deployment]
site_id = "field-notes"
`,
    "index.md": `---
title: Field Notes
---

Notes from the field, every other week.

Get the next one by email.

:::subscribe
:::
`,
  },
};
