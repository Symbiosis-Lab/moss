/**
 * The scratch site the subscribe-questions render gate builds: a page whose
 * `:::subscribe` asks questions, an `en/` edition whose form asks others, and a
 * site id, so the form is wired. All
 * content is invented. Served by tests/render-gates/site/subscribe-questions.spec.ts.
 */
import type { ScratchSiteSpec } from "./scratch-site";

export const SUBSCRIBE_QUESTIONS_GATE: ScratchSiteSpec = {
  name: "subscribe-questions-gate",
  files: {
    ".moss/state.toml": `[deployment]
site_id = "field-notes"
`,
    "index.md": `---
title: Subscribe
---

Notes from the field, every other week.

:::subscribe {button="Subscribe"}
- Your name * {autocomplete=name}
- Where did you hear about us *
  - [ ] A friend
  - [ ] A talk or workshop
  - [ ] Social media
  - [ ] Other
    - Where else
- Anything you want to tell us {type=textarea}
:::
`,
    "en/index.md": `---
title: Subscribe
---

The English letter, once a month.

:::subscribe {button="Subscribe"}
- Your name *
- Topics
  - [ ] Field notes
  - [ ] Events
:::
`,
  },
};
