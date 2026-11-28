/**
 * The scratch site the subscribe-bodyless render gate builds: the one-line
 * `:::subscribe` form on an English page, a German page and a members page,
 * with a site id so each form is wired. All content is invented. Served by
 * tests/render-gates/site/subscribe-bodyless.spec.ts.
 */
import type { ScratchSiteSpec } from "./scratch-site";

export const SUBSCRIBE_BODYLESS_GATE: ScratchSiteSpec = {
  name: "subscribe-bodyless-gate",
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
    "de/index.md": `---
title: Feldnotizen
lang: de
---

Notizen aus dem Feld, alle zwei Wochen.

:::subscribe
:::
`,
    "letters/index.md": `---
title: Letters
members: subscribers
---

The long letters, for readers on the list.

:::subscribe
:::
`,
  },
};
