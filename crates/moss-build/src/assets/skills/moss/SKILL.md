---
name: moss
description: Author, style, extend, or migrate a moss static site the canonical way. Use when changing a site's theme, style, or CSS — colors, palette, typography, fonts, dark mode, spacing, layout — when writing or restructuring its content, when building a moss plugin, or when converting an existing website into a moss project. Applies in any folder containing .moss/.
uid: "87da8da7"
---

# Working in moss

moss turns a folder of markdown into a portable, semantic website. Keep three
things legible forever: the **prose** (`.md` files), the **visual voice**
(`.moss/theme/`), and the **shipped HTML**. If any becomes illegible, you have
failed the user. Prefer the cheapest representation that works: prose over
markup, markup over code, shared rules over duplicated ones.

**Personalize, don't generate.** Find the closest existing pattern and adapt
it. Generation from scratch scatters one-off HTML and gradient-soup themes;
both erode legibility.

## Start from a blank folder

1. Create a folder. Its name becomes the site title — choose it deliberately.
2. Add `.md` files (see naming convention below and the example in
   `moss guide example`).
3. `moss build <folder>` — auto-creates `.moss/` on first run. No init step.
   No wizard.
4. `moss preview <folder>` — opens the live preview in moss desktop for you to watch. An agent has no display: use `moss build --serve` below instead.

**Headless / agent self-check:** `moss build <folder> --serve` builds and
serves, printing the URL it bound — read that, do not assume a port. (8080 is
only the first one tried; a foreign dev server holding it means moss lands
elsewhere, and a hardcoded `localhost:8080` would audit someone else's site.)
Inspect built HTML under `.moss/build.nosync/current/` (the active frozen generation);
the `.css` files under its `_moss/` are minified build output — use
`moss describe --css <selector>` instead of reading them.

**Check your work.** `moss build` exits 0 even when it reported problems; add
`--strict` to make warnings fail the build. `moss list [--json]` is the
inventory of what actually got published — url, kind, date, and the LANG column
that confirms a new language tree registered. Use `moss rename <old> <new>` for
renames: it rewrites every `[[wikilink]]` and `[text](link)` project-wide, which
hand-editing will not.

## First, get the live vocabulary

Class names, tokens, shortcodes, and frontmatter fields change between moss
releases. Never hardcode them from memory. From the project root, run:

```
moss describe --json          # everything
moss describe --json | jq 'keys'   # what "everything" covers, right now
```

That key list is itself generated, so it is always complete and this file cannot
fall behind it. Treat the output as the source of truth for any `--moss-*` name,
`moss-*` class, built-in frontmatter key, language code, plugin hook, slot, or
CLI command.

Two of those keys answer questions whose failure mode is silence, so reach for
them *before* you act rather than after:

- `.languages` — the allowlist for a language directory or filename suffix. An
  unrecognized name is not an error; it is treated as ordinary content, so the
  build succeeds and nothing tells you the edition does not exist.
- `.cli_commands` — the commands and flags this binary has. `moss <cmd> --help`
  works too.

The project's `.moss/AGENTS.md` (if present) carries site-specific notes.

Before you override a rule, read the one you are displacing:

```
moss describe --css <selector>
```

It prints every shipped rule that mentions that selector, across all of moss's
stylesheets. Overriding a rule you have not read is how a stylesheet acquires
declarations that fight each other.

## Canonical project shape and naming convention

- **A folder is a site (or a section); its name is the title.** `my-blog/` →
  "My Blog". Name the folder as you want the title to appear.
- **The filename IS the title** (Obsidian-style). `My First Essay.md` → title
  "My First Essay". Do **not** also write a body `# Heading` repeating the
  title, and do **not** add a `title:` frontmatter field — either causes a
  duplicated or overridden title.
- **Name files in their own language; pin a non-ASCII URL.** The
  filename-as-title rule lets a localized name title the page for you —
  `隐私.md` → "隐私". When the name isn't ASCII, add `url:` to keep the public
  path clean and stable: `url: privacy` publishes `隐私.md` at `/privacy`.
- **A folder's home page is a markdown file** whose stem is `index`, `readme`,
  `_index`, or `main`, **or** that is named after its folder (`recipes/recipes.md`
  is the home of `recipes/`). Every other `.md` file is an article.
- moss classifies every page as **Article** or **Folder**.

## Styling: the one-sentence model

Edit `.moss/theme/style.css`: override any `--moss-*` token in `:root {}` (light)
and `:root[data-theme="dark"] {}` (dark), then write ordinary CSS selectors for
the rest. No `!important`, no `@layer`.

moss's default rules ship **compiled into the binary** — there is no readable
default stylesheet on disk anywhere in a site folder. Everything under
`.moss/build.nosync/` is regenerated output, not source. A themed site has at least two
stylesheets there: `_moss/style.<hash>.css` (the built-in defaults, minified to
one line with comments stripped) and `_moss/theme/style.<hash>.css` (a
hashed copy of your own `.moss/theme/style.css`), plus one
`_moss/css/<name>.<hash>.css` for each enabled feature (comments, email,
review). Never read any of them — the defaults alone are ~20k tokens of
unskimmable minified text — and never edit them: the next build overwrites all
of them. Read the defaults with `moss describe --css <selector>`; write
overrides in `.moss/theme/style.css`.

## The four styling rungs

When a design call needs a visual treatment, stop at the first rung that reaches it:

1. **Token override.** Set `--moss-*` tokens in `:root {}`. Run
   `moss describe --json` for every current token name, its light value, and its
   `dark_value`. To keep nav/buttons neutral while content links stay accented,
   set `--moss-color-ui-accent: var(--moss-color-text)`.
2. **Selector on semantic HTML.** Target the existing semantic structure (a
   heading, a blockquote, a list) with a plain CSS rule. No new markup.
3. **Named-class fenced div.** `::: {.your-class}` wraps markdown in a `<div>`
   with that class. Write the CSS under the class name. One descriptive class,
   not three stacked utilities.
4. **Structural override.** Re-lay-out a page against moss's defaults: scope to
   a declared attribute (`body[data-page="home"]`, `[data-theme="dark"]`), set
   the component's declared custom properties rather than re-declaring its
   layout, and read `moss describe --css <selector>` for the rules you are
   displacing. Reach here when a whole page has a different shape, not when one
   element has a different colour.

Rung 4 exists because rungs 1–3 restyle a component and some designs need to
re-lay-out a page. It is the last rung, not an escape hatch: `moss describe`
lists which properties a component exposes precisely so that "override the
layout" stays a supported operation rather than a fight with the cascade.

Never write `@layer` in your `.moss/theme/style.css` — moss handles cascade order
for you. Never write `!important`.

For shortcodes and partials (content reuse, not styling tiers), see
`moss guide authoring`.

## Hard rules

- Use `::: {.class}` for styling wrappers, never `<div class="...">`. Literal
  HTML earns its place only for semantics markdown cannot express — a `<nav>` or
  `<section>` landmark with an `aria-label`, or a stable hook where a positional
  selector like `p:first-of-type` would silently restyle the wrong element once
  the page grows. Say which of those applies, in a comment, at the point of use.
- Never write inline `style="..."` — put the rule in `.moss/theme/style.css`.
- Images use wikilinks: `![[filename.ext]]`. Never hardcode paths.
- A deck (standfirst) is a `> blockquote` as the first thing in the body. moss
  injects the title itself, so the deck sits directly under it — do not write a
  `# Title` above the blockquote to position it.
- Repeated blocks become a partial, transcluded with `![[partial-name]]`.
- Frontmatter is for metadata and declared controls, never prose. Some of those
  controls *are* visual — layout, cover, children rendering, content width — so
  check `.frontmatter[]` before reaching for CSS to do something a declared
  field already does.

These are the essentials. Everything else is in the binary you are already
running: `moss describe --json` for any name, `moss guide --list` for the rest.
Prefer both over anything you remember, and over any documentation URL — you are likely
offline, and a URL is a second source of truth that goes stale between
releases, which is the whole reason `describe` exists. For the full styling
model (dark mode, quiet chrome, `@layer` rules), see
`moss guide authoring`.

## What are you doing?

- **Starting a new site** → read `moss guide example`
  for the canonical folder shape and file content to copy.
- **Authoring or styling a site** → read `moss guide authoring`.
- **Building a plugin** (deploy, syndicate, comments — anything needing the
  network or an external API) → read `moss guide plugins`.
- **Converting an existing site** (a URL, or local files) into moss → read
  `moss guide importing`.
- **Debugging a build, plugin, or shortcode** → read
  `moss guide debugging`.
- **Building a site in more than one language** →
  `moss guide authoring`, Multilingual sites.
- **Putting the site online** → Publishing below.

## Publishing

- **First publish (moss hosting):** `moss env production <folder>` (or
  `staging`) **then** `moss deploy <folder> --site-id=<name>` → publishes to
  `<name>.mosspub.com`. The order matters: without a prior explicit `moss env`,
  `deploy` will not register a site, and `--site-id` alone silently does
  nothing. The invite allowlist can also refuse registration.
- **Every publish after the first:** `moss deploy <folder>` — the site id
  already lives in `.moss/state.toml`.
- **Custom domain:** `moss domain list <folder>` / `moss domain link <folder>
  example.com`.
- **Site built by another generator:** `moss deploy <folder> --prebuilt=_site`
  skips `moss build` and uploads that directory as-is.
- **Any non-moss host:** `moss build <folder> --site-url=https://example.com`,
  then upload `.moss/build.nosync/current/` yourself. `--site-url` is required there
  because canonical URLs, `og:image`, the sitemap, and RSS are all absolute and
  otherwise derive from moss hosting deployment state, which a non-moss host
  never sets. Flag details: `moss describe --json`.

## Harvest, don't unilaterally invent

If a pattern recurs across pages, tell the human — don't canonicalize a new
class yourself. Site-local classes with clear CSS are fine; minting a
contract-shaped name from one site's habit is not.
