# Importing and redesigning a site

Use this when an existing website should become a moss site that looks better than the original. It is not a faithful port (that is `moss guide importing`) and not a new site from nothing (`moss guide example`). The result has two halves, and both are the deliverable:

- **The vision.** The site as it should look: the original's own aesthetic direction, pushed further, with today's best practice underneath it.
- **The authoring surface.** A folder the owner can keep editing without a developer. Pages are plain markdown named for what they are, sections are folders, facts are frontmatter, and every visual decision lives in `.moss/theme/`.

A redesign that reaches the vision by turning pages into HTML has failed the second half. A clean folder that looks like moss's defaults has failed the first. Every round of work moves toward the vision without spending the surface.

## Work as a team of small agents

If your harness can dispatch agents (subagents, tasks, workers), split every step into tasks small enough for the smallest, fastest model, and dispatch those. Run independent ones in parallel. Keep the judgment in your own session, and read what agents report rather than everything they read. A small model does well when the task is narrow and fully stated, and badly when it must decide what the task is. So shape each task until it is:

- **One output.** One file, one table, or one set of edits, with its exact path.
- **Fully specified.** The inputs by path, numbered steps, and the commands to run. Nothing is left to find out except what the task exists to find.
- **Checked by a gate.** A command that passes or fails (a build with `--strict`, a script, a grep), or a checklist of named yes/no questions, never "review this".
- **Reported in a fixed shape.** A short table or a few lines in a stated format, so that reports from parallel agents line up.

| Tier | Use it for | With Claude |
|---|---|---|
| Small (the default) | Every task that fits the shape above: fetching and screenshots, inventories, reading computed styles, scoring one reference against a rubric, applying one section of a build spec, answering one review checklist, running the checks | Haiku |
| Mid | A task a small agent has already failed at once, re-sent with what went wrong; or one whose output cannot be specified ahead of time | Sonnet |
| Strongest | The design direction and the specs everything else is cut from: the vision, the move table, the content schema, the theme plan, and deciding what each gap means | Opus, usually your own session |

Move a task up one tier only after the smaller model's result has failed, and record why. When a small agent fails, first try making its task smaller or better specified; that usually works, and it keeps the next run cheap. Every dispatch states the goal, the exact input and output paths, what it must not touch (the original site's servers deserve gentle pacing; most hosted builders rate-limit), and that a denied tool call means stop and report, not find another way. Agents that run at the same time never share a browser: one browser session driven by two agents mixes their pages, and a screenshot named for one site shows another. Give each agent its own, such as a headless browser script per agent, and check every screenshot against the address it claims. Without dispatch, do the same steps yourself, in order.

Keep three places apart:

- `<Site>/`: the moss site, holding only what the published site needs. Any `.md` in it is published as a page.
- `<Site> design/`: beside it, never inside it. The review, research, vision, prototype, screenshots and gap list live here.
- The original site, which you read and never write.

## 1. Import

`moss import <url> "<Site>" --recursive`, as `moss guide importing` describes. Read the summary: pages written, duplicates skipped, failed pages, widgets carried as links or with no static form, and the site chrome written. Build it once with `moss build "<Site>" --strict` and take `moss list --json` as the page inventory. An imported site often fails that first strict build on dead links to addresses the old platform served as aliases (`/home`, a renamed page); repoint each link, or redirect the old address, before building further, and record which you did. Then put the folder under version control (`git init`, commit) with nothing else changed: the import is the content baseline, and every later change becomes a reviewable diff against it. Build before the first commit, not after: the first build writes `.moss/.gitignore`, which keeps build output, caches and the site's identity out of the baseline.

Write what the import lost into `<Site> design/import.md`: failed pages, widgets with no static form, anything that came across wrong. Each is a decision for the vision, never something to patch silently. Prefer a re-import over hand edits to fix what came across wrong, because a re-import discards hand edits. An import defect moss should fix is worth reporting to moss with the URL and the page.

## 2. Understand the original

Dispatch these small tasks in parallel, each writing one file into `<Site> design/review/`:

| Task | Output |
|---|---|
| Screenshots of the home page and five representative pages at 1440 and 390 px | `shots/<page>-<width>.png`. Hosted builders reveal content as it scrolls into view, so a capture straight after load shows blank bands: scroll to the bottom and back first, and check each image against its address |
| The sitemap, grouped by kind of page, with counts | `inventory.md` |
| For each kind of page, the facts one example carries (an event: date, time, venue, programme, performers, tickets) | `content-model.md` |
| Every dynamic or third-party element (ticketing, donations, newsletter, calendar, video) and what it needs on a static site: a link out, an embed, a hosted form, or nothing | `widgets.md` |
| Computed font family, size and weight of the h1, h2, body text and nav on three pages, and the hex colours of background, text, links and buttons, read from computed styles of real elements (a stylesheet often declares faces a page never uses, so a declared font is not evidence) | `style.md` |
| Three short quotes of the copy's voice, and the logo and favicon files | `voice.md` |

Then, in your own session, read the six files and the screenshots and write two lines into `vision.md`'s notes: the current aesthetic direction in one sentence, and what is genuinely good and what is weak.

## 3. Research

Choose the candidates yourself: eight to twelve of the best sites of the same kind, five to eight in the same aesthetic from any field, and the relevant entries from [the list below](#design-references). Then dispatch one small task per candidate with the same rubric, so the answers line up in one table: the URL; whether it loaded; the one pattern it does best; how that pattern would apply here; and its aesthetic in a phrase. Design references are searched locally first and fetched otherwise. Take patterns and name where each came from. Never copy one site's design.

## 4. The vision

Strongest tier, with everything above in hand. Write `<Site> design/vision.md`:

- **The direction in one sentence:** the original's own aesthetic, doubled down. The owner chose that direction, and the redesign makes it more itself rather than swapping it for a fashionable one.
- **Three to five moves that amplify it**, and what to drop.
- **The best practice it keeps whatever the direction:** content before decoration; every top task one step from the home page; real text, never text in images; WCAG 2.2 AA contrast and visible keyboard focus; readable at phone width; light pages (few font files, images sized for their slot); and dark mode, if the design has one, designed rather than inverted.
- **The fate of every lost widget:** a link to the hosted page, an embed, a contact route, or nothing, with the reason.

Put the system (type scale, palette for light and dark, spacing, image treatment, and the layout of each kind of page) in `<Site> design/DESIGN.md`, in the DESIGN.md format listed under [Design references](#design-references): tokens in its front matter, rationale in prose. Its tokens map onto moss's `--moss-*` tokens in step 5, and its linter checks contrast.

Then build the vision as a hand-written static prototype in `<Site> design/vision/`: the home page, one listing and one item page, at full fidelity, with real content. It is the target and is not constrained by moss. Keep it to plain HTML and CSS so its rules can move into the theme. First write its shared stylesheet from DESIGN.md yourself, or have one task do it from DESIGN.md alone. Then give each page to its own small task, with the stylesheet, the page's structure from `vision.md` and the content file it draws from. Look at every screenshot yourself before the prototype becomes the target.

## 5. Build it in moss

Write three specs yourself, from the vision, the prototype, `moss guide authoring` and `moss describe --json`, then let small tasks apply them:

1. **The move table**, `build/moves.md`: one row per page that moves, old path to new path. A folder per section, a home file per folder, filenames that are the titles. Small tasks apply it a section at a time with `moss rename`, which rewrites every link to a moved page, and add a `[redirects]` entry for each old address that changes. The gate is `moss build --strict` and `moss list --json` showing every page.
2. **The content schema**, `build/schema.md`: for each kind of page, which facts go in which frontmatter field moss declares (`moss describe --json | jq '.frontmatter'`), such as an event's `start`, `end`, `timezone` and `tickets`, and what stays prose. A block repeated on several pages becomes a partial. One small task per section of the site applies it to that section's pages, then lists any page it could not fit.
3. **The theme plan**, `build/theme.md`: the token values first, then one entry per kind of page naming the prototype's rules that page needs and the moss selector each lands on, found with `moss describe --css <selector>`. These follow the styling rungs in `moss guide authoring`. One small task writes the tokens, then one task per kind of page writes its rules, each rule with its design reason in a comment.

Each task ends by running `moss build "<Site>" --strict` and reports pass or fail with the first problem.

What keeps the surface friendly:

- A page's source reads like the page: no raw HTML where markdown or a moss block does the job, no inline styles, no class on every paragraph.
- A fact lives in one place. A value several pages share is frontmatter or a partial, never pasted.
- The owner's own sentences stay as written, even when one repeats a fact moss now shows from frontmatter ("Join us Sunday at 2:00 PM"). Remove builder leftovers that repeat facts (date lines, venue blocks, ticket buttons), never the owner's prose.
- Every theme rule carries a short comment with its design reason, or it goes.
- A source-owned HTML homepage (`moss guide authoring`) gives up the surface for that page. Use one only when the owner accepts that, and record why.

## 6. Iterate toward the vision

Each round, one small task screenshots the moss build and the prototype at the same widths. Then one small task per page answers a checklist you wrote from `vision.md`: named yes/no questions such as "does the next concert show its date in large numerals?" and "is the nav one line at 390 px?", each answered with the screenshot region as evidence. Collect every "no" in `<Site> design/gaps.md`, and give each gap one home yourself:

| The gap | Its fix |
|---|---|
| The theme can express it | A theme rule |
| A moss feature does it | The field or block `moss describe` names |
| The content is shaped wrong | Restructure the folder |
| moss cannot express it without spending the surface | Keep the gap. Record what it is, on which page, and what a workaround would cost, and report it to moss |

Stop when the vision is reached or every remaining gap sits in the last row. Check the surface every round against the list in step 5. A round that closes a gap by putting HTML into pages is rolled back.

## 7. Before handing over

- `moss build --strict` passes. `moss list --json` holds every imported page except deliberate removals, and every old address still resolves or redirects.
- Check phone and desktop widths, light and dark, keyboard focus, and AA contrast.
- Open three page sources at random. A non-developer could edit each without touching the theme.
- Hand the owner the folder, plus from `<Site> design/` the vision, the gap list, and what each lost widget became.

## Design references

Search for a local copy first, and fetch the URL only when there is none. A local copy is an agent skill installed under the name given (in your harness's skills or plugins folder), a checkout of the repository, or the npm package. Grep its text for the question at hand rather than reading it whole. Prefer the machine-readable entries (a skill, a guideline in markdown, an `llms.txt`) over galleries. Galleries are for direction, structure and type; take patterns from them and never copy a site. A gallery that refuses automated requests is skipped, not routed around. Fonts must carry a licence that allows self-hosting (OFL, or terms you have read).

**Agent design skills and guidelines**

| Reference | Local name | Use it for |
|---|---|---|
| [frontend-design](https://github.com/anthropics/skills/tree/main/skills/frontend-design) | skill `frontend-design` | Committing to a bold, specific direction instead of the generic template look |
| [Impeccable](https://github.com/pbakaus/impeccable) ([docs](https://impeccable.style)) | skill `impeccable` | Critique and audit passes on a draft (`critique`, `audit`, `distill`, `bolder`, `quieter`), with detectors for machine-made tells |
| [Web Interface Guidelines](https://github.com/vercel-labs/web-interface-guidelines) | repo `web-interface-guidelines` | A checklist for focus, forms, motion and content, to review finished pages against |
| [UI Skills](https://github.com/ibelick/ui-skills) ([registry](https://www.ui-skills.com)) | skill `baseline-ui` | Small focused skills for baseline polish and motion |
| [DESIGN.md](https://github.com/google-labs-code/design.md) | npm `@google/design.md` | The format for the vision's system: tokens in front matter, rationale in prose. `npx @google/design.md lint DESIGN.md` checks contrast |
| [awesome-design-md](https://github.com/VoltAgent/awesome-design-md) | repo `awesome-design-md` | DESIGN.md write-ups of real brand sites, to learn how a direction becomes tokens |
| [Anthropic skills](https://github.com/anthropics/skills) | skill `theme-factory` | Palette and type presets to start a system from |

**AI design tools with something to browse**

| Reference | Use it for |
|---|---|
| [Google Stitch](https://stitch.withgoogle.com) | Generating alternative directions and exporting their tokens as DESIGN.md |
| [v0 templates](https://v0.app/templates) | Layout patterns for common page kinds |
| [Framer templates](https://www.framer.com/marketplace/templates/) | Well-made marketing and portfolio layouts to study |
| [Relume components](https://www.relume.ai/components) | A taxonomy of page sections, to check nothing a visitor expects is missing |

**Galleries**

| Reference | Use it for |
|---|---|
| [Minimal Gallery](https://minimal.gallery) | Restrained, typographic sites |
| [Typewolf](https://www.typewolf.com) | Real font pairings, named per site |
| [Fonts In Use](https://fontsinuse.com) | Type in context, in print and on the web |
| [Awwwards](https://www.awwwards.com/websites/) | The current edge of motion and interaction |
| [Hoverstat.es](https://hoverstat.es), [Httpster](https://httpster.net) | Interaction ideas and a broad current sample |

**Type and colour**

| Reference | Use it for |
|---|---|
| [Google Fonts](https://fonts.google.com) | Open-licence families to download and self-host |
| [Velvetyne](https://www.velvetyne.fr), [Collletttivo](https://www.collletttivo.it) | Open-source display faces with character; confirm each family's licence |
| [Fontshare](https://www.fontshare.com) | Free commercial faces under their own licence, which you read first |
| [OKLCH picker](https://oklch.com) | Picking colours in a perceptually even space for light and dark pairs |
| [Radix Colors](https://www.radix-ui.com/colors) | Twelve-step scales with matched light and dark values |

**Quality**

| Reference | Use it for |
|---|---|
| [WCAG 2.2 quick reference](https://www.w3.org/WAI/WCAG22/quickref/) | The accessibility criteria the handover checks |
| [APCA in a nutshell](https://git.apcacontrast.com/documentation/APCA_in_a_Nutshell.html) | A second opinion on contrast, especially for large display type |
| [Core Web Vitals](https://web.dev/articles/vitals) | Performance targets |
