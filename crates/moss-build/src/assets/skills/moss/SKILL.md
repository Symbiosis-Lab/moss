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

**Check your work.** `moss build` exits 0 even when it reported problems; add `--strict` to make warnings fail the build. `moss list [--json]` is the inventory of what actually got published — url, kind, date, and the LANG column that confirms a new language tree registered. Use `moss rename <old> <new>` for renames: it rewrites every `[[wikilink]]` and `[text](link)` project-wide, which hand-editing will not.

**Before a restructure, save a version.** Moving pages, rewriting the theme, or deleting sections is safer with a named snapshot first: `moss history --save "before restructuring the nav"`. Both `--save` and `--restore` build the whole site first — the same build `moss deploy` runs, minus the publish — so a build that fails refuses to save or restore, and a slow build makes either one just as slow. `moss history [--json]` lists what's saved, newest first — every landed publish gets one automatically, `--save` adds one on demand. `moss history <path> --restore --at <id> [--copy]` brings back one page (`--copy` writes it alongside the current file instead of overwriting it); `moss history --restore --at <id> --yes` brings back the whole site (`--yes` is required, since it can move files to the Trash). A restore saves its own version of the current state first, so a restore is itself undoable the same way. `<id>` is a version's id from the timeline (or an unambiguous prefix), and `--json` includes it. This history lives in `.moss/history/` inside the site folder itself, gitignored and not part of the published output — being inside the folder, it travels along with whatever sync the site already uses, the same as `.moss/config.toml`.

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

- **A folder is a site (or a section); its name is the title.** Name the
  folder as you want the title to appear — moss does not case it for you.
  Add `logo:` on a home page (the site root's, or a language edition's own)
  to put a mark before that name in the nav — on every page, every
  language; `logo:` anywhere else in the site is read by nothing.
- **The filename IS the title** (Obsidian-style). `My First Essay.md` → title
  "My First Essay". The only transform is hyphens and underscores becoming
  spaces — nothing is capitalized or reworded, so `about.md` titles "about"
  and `our-mission.md` titles "our mission", not "About" or "Our Mission" (a
  folder's own name goes through the same transform: `my-blog/` titles "my
  blog", not "My Blog"). Add `title:` whenever the title you want differs from
  that result even slightly; it is redundant, and safe to omit, only when it
  matches the filename's derived title character for character. Do **not**
  also write a body `# Heading` repeating the title — it renders as well, so
  the title would show twice. One exception: a `:::hero` block at the very
  top of the body takes over the title slot itself, so a heading written
  inside it does not duplicate anything. Nothing else does this — a
  `:::grid` or any other block opening the page still leaves the
  auto-injected title in place, so a heading after it shows the title twice
  the same as if the grid weren't there.
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
- **Reviewing or hiding a site's comments (spam) before publishing** → read
  `moss guide comments`.
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
- **Refused because the live site is newer:** `deploy` to moss hosting
  (`--prebuilt` included) refuses when the site was published from another
  copy of the folder after this copy's last publish. Bring this folder up to
  date with that copy (usually `git pull`) and deploy again. `--overwrite-newer`
  undoes the other publish — use it only when the human says to. A
  `[hooks] deploy` plugin's destination is not checked.
- **Before every real publish, dry-run it:** `moss deploy <folder> --dry-run`
  builds exactly as a deploy builds (the build may use the network as any
  build does: plugins the flags allow, link previews from third-party sites), prints the pages added, edited and deleted and
  the addresses that would go offline, and stops. It uploads nothing and
  records nothing, and exits 1 where a deploy would be refused by the checks it
  ran. It does not check what needs the server (whether another copy published
  since), so a passing dry run is not a promise the deploy goes ahead.
- **Refused because an address would go offline:** an address the site has
  served must not vanish unasked. When a build would stop serving one for a
  reason other than the human deleting its source (a page whose address
  changed, a generated file no longer produced), `deploy` and `--dry-run`
  list each address with its cause and refuse. Keep the address working
  (for a moved page, add the printed `"/old/" = "/new/"` line under
  `[redirects]` in `.moss/config.toml`), or pass `--accept-removals` — only when
  the human has said to lose those addresses.
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
