---
name: moss
description: Author, style, extend, or migrate a moss static site the canonical way. Use when changing a site's theme, style, or CSS — colors, palette, typography, fonts, dark mode, spacing, layout — when writing or restructuring its content, when building a moss plugin, or when converting an existing website into a moss project, faithfully or as a redesign. Applies in any folder containing .moss/.
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

**Never ship less than the design needs.** moss has a low floor and a high ceiling: the simple parts of a site are plain markdown and folders, and anything a hand-built site can put on the web a moss site can put there too. When markdown cannot say something — an interactive drawing, a scroll-driven scene, a custom component — keep the words in markdown and take an escape hatch for the rest: raw `<svg>` or `<canvas>` in a partial, `.moss/theme/script.js`, a plugin. Do not cut the design down to what is easy in markdown. If moss blocks something the web can do, or forces a workaround, tell the user it is a moss gap and name the reproduction, rather than quietly shipping less.

## A collection from a spreadsheet

For a folder of items with a metadata file (pictures plus a CSV, as an archive or library hands it over). Never edit the CSV, the pictures or the source note; a script that writes pages runs from outside the site folder, since every file left in the folder is published.

1. **One page per row**, in a folder of its own (`photographs/<slug>.md`). Its body is one picture line embedding the file where it already is: `![Title|](../images/x.jpg)` (the trailing `|` keeps the title as alt text without repeating it as a caption).
2. **Every other column into frontmatter**, header unchanged as the key, value exactly as the CSV has it. A name for readers goes in the field's `label`, never in the key. Prefix a header that is a moss key with another meaning (`url` becomes `source_url`; `moss describe --json | jq '.frontmatter'` lists them).
3. **Dates.** `date:` must be a calendar date (`1938`, `1938-05`, `1938-05-18`). If any row's date is not, put every row's value, as written, in one declared text field (`catalogue_date`).
4. **Facts.** In `.moss/config.toml`, declare each column a visitor should see as a field with `place = "facts"`: every column whose values differ between rows, the identifiers, and the rights statement (even when every row has the same one). Leave out the columns already used elsewhere: the file column is the picture line, and the title column is the page's `title:`.
5. **Credit and link back.** Declare the item-link column `type = "url"` with `text = "<the credit line the source asks for>"`.
6. **The creator, once.** If one creator made every item, set `author:` in `cascade:` on the items folder's home page (each page is cited to them) and show the name with a declared field set in the same `cascade:` (`photographer`, `place = "facts"`). `[site] author` is this edition's maker, not the items' creator.
7. **Citable.** `archival = true` under `[site]`: every page shows its edition and a citation, and the source travels with the site. The site's name is the home page's `title:`; `moss describe --json | jq .site_settings` lists every `[site]` setting. Until the site has an address, the build prints one note that citations name no address; leave it, since it clears once `[site] domain` is an https address.
8. **Browse.** Make the browse column a term kind (`[terms.boroughs] fields = ["borough"]`, with `count = "{n} photograph|{n} photographs"` so its cards name what they count). On the home page embed one listing per value (`![[/boroughs/brooklyn/|style:grid]]`) and set `children: false` (otherwise every item is listed again below) and `content_width: wide`. Since the home page already shows the boroughs, leave the kind out of the nav: no `nav = true`.
9. **Pictures whole.** For prints that must not be cropped, set `--moss-card-cover-fit: contain` and a `--moss-card-cover-ratio` close to the prints' own shape in `.moss/theme/style.css` under `:root` (an `aspect-ratio` value: the default is `4 / 3`, upright prints suit `4 / 5`); `contain` shows the whole picture inside that box, with bands where the shapes differ. Give the pictures folder an `index.md` with `listed: false`.
10. **The source note** becomes a page by adding frontmatter above its unchanged text.
11. `moss build --strict`, then read every warning and note.

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

**Check your work.** `moss build` exits 0 even when it reported problems; add `--strict` to make warnings fail the build. `--strict` fails on problems, which the author can fix in the folder; notes are advice and never fail it. A problem prints as `[WARN]` and counts; a note prints as `[NOTE]` (a dead root-relative link is one, since a proxy may serve the path; so is a file the build left out because it is still downloading from cloud storage, or a suspended process); environment, timing and cache notes log at INFO and are hidden unless `MOSS_LOG_LEVEL=info`. A build's last status line is `Built N pages (M skipped) · K problems`, plus ` · J notes` when there are any; `moss: K problems reported above` follows it only when K is not 0, and is the line to grep for. `moss list [--json]` is the inventory of what actually got published — url, kind, date (an event page with `start:` and no `date:` shows its start day), and the LANG column that confirms a new language tree registered. Use `moss rename <old> <new>` for renames: it rewrites every `[[wikilink]]` and `[text](link)` project-wide, which hand-editing will not.

**Before a restructure, save a version.** Moving pages, rewriting the theme, or deleting sections is safer with a named snapshot first: `moss history --save "before restructuring the nav"`. Both `--save` and `--restore` build the whole site first — the same build `moss deploy` runs, minus the publish — so a build that fails refuses to save or restore, and a slow build makes either one just as slow. `moss history [--json]` lists what's saved, newest first — every landed publish gets one automatically, `--save` adds one on demand. `moss history <path> --restore --at <id> [--copy]` brings back one page (`--copy` writes it alongside the current file instead of overwriting it); `moss history --restore --at <id> --yes` brings back the whole site (`--yes` is required, since it can move files to the Trash). A restore saves its own version of the current state first, so a restore is itself undoable the same way. `<id>` is a version's id from the timeline (or an unambiguous prefix), and `--json` includes it. This history lives in `.moss/history/` inside the site folder itself, gitignored and not part of the published output — being inside the folder, it travels along with whatever sync the site already uses, the same as `.moss/config.toml`.

## First, get the live vocabulary

Class names, tokens, shortcodes, and frontmatter fields change between moss
releases. Never hardcode them from memory. From the project root, run:

```
moss describe --json          # everything; the .moss/config.toml [site] keys are under site_settings
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
  inside it does not duplicate anything. A page in the navigation shows its title too. Nothing else does this — a
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

## Before styling: where a fact lives

A fact is a field, and moss places it. Before writing CSS, a classed div or HTML for something a page states (a date, a credit, a place, a role, a label), stop at the first of these that holds:

1. It is a fact about this page: declare a field with a `place` in `.moss/config.toml` (`moss guide authoring`, Declared fields), and write the value in frontmatter.
2. Every page in a folder shares the value: write it once in `cascade:` on the folder's home page; a page's own value wins.
3. Several facts read as one line: a group field, `type = "group"` with `of = [...]`, draws the parts that have a value, in order.
4. A picture needs its crop or framing: display words on the embed, `![[photo.jpg|focus:30%,20%]]` keeps that point in view, `|contain` shows it whole (`moss guide authoring`, images).
5. A section a link must reach: `## Opening hours {#hours}`, linked as `[[Page#hours]]`.

Only then style it, with the rungs below. Never write `{.class}` on an image, link or inline span: other markdown tools print it as text.

## The four styling rungs

When a design call needs a visual treatment, stop at the first rung that reaches it:

1. **Token override.** Set `--moss-*` tokens in `:root {}`. Run
   `moss describe --json` for every current token name, its light value, and its
   `dark_value`. To keep nav/buttons neutral while content links stay accented,
   set `--moss-color-ui-accent: var(--moss-color-text)`.
2. **Selector on semantic HTML.** Target the existing semantic structure (a
   heading, a blockquote, a list) with a plain CSS rule. No new markup.
3. **A declared field with a `place`, or a named-class fenced div.** First ask
   whether the content is a fact about the page (a year, a count, a role, a
   place). If it is, declare it as a field and give it a `place` (see `moss guide authoring`, Declared fields),
   so moss draws it from frontmatter instead of a classed block repeated on
   every page. Otherwise `::: {.your-class}` wraps markdown in a `<div>`
   with that class. Write the CSS under the class name. One descriptive class,
   not three stacked utilities.
4. **Structural override.** Re-lay-out a page against moss's defaults: scope to
   a declared attribute (`body[data-page="home"]`, `[data-theme="dark"]`), set
   the component's declared custom properties rather than re-declaring its
   layout, and read `moss describe --css <selector>` for the rules you are
   displacing. Reach here when a whole page has a different shape, not when one
   element has a different colour.

After moving a theme rule onto a new selector, build and run
`moss doctor --surface`: it names every rule in `.moss/theme/` that matches no
element on any built page, which is how a rule left on a class the pages no
longer carry shows up.

Rung 4 exists because rungs 1–3 restyle a component and some designs need to
re-lay-out a page. It is the last rung, not an escape hatch: `moss describe`
lists which properties a component exposes precisely so that "override the
layout" stays a supported operation rather than a fight with the cascade.

Never write `@layer` in your `.moss/theme/style.css` — moss handles cascade order
for you. Never write `!important`.

For shortcodes and partials (content reuse, not styling tiers), see
`moss guide authoring`.

## Hard rules

- Show a destination once. What the home page already shows (a set of listings, a map) is not also a nav item, and a nav item's page is not repeated on the home page; the same holds for a footer link that repeats the nav.
- Use `::: {.class}` for styling wrappers, never `<div class="...">`. Literal
  HTML earns its place only for semantics markdown cannot express — a `<nav>` or
  `<section>` landmark with an `aria-label`, or a stable hook where a positional
  selector like `p:first-of-type` would silently restyle the wrong element once
  the page grows. Say which of those applies, in a comment, at the point of use.
  A `data-*` hook for your own script is the stated exception: write it as
  `::: {.figure fig=teapot}` (which emits `data-fig="teapot"`), or as a raw
  `<svg data-figure="…">`/`<canvas>` where the element itself is the drawing.
  Drawings and your own JavaScript: `moss guide authoring`.
- Never write inline `style="..."` — put the rule in `.moss/theme/style.css`.
- Images and media use the standard form: `![](photo.jpg)`, written as a path relative to the page. Wikilinks (`![[photo.jpg]]`) work too and find the file by name. Either way moss resolves the file, so never write a site-absolute or `https://` address for a file in the folder.
- A deck (standfirst) is a `> blockquote` as the first thing in the body. moss
  injects the title itself, so the deck sits directly under it — do not write a
  `# Title` above the blockquote to position it.
- Repeated blocks become a partial, transcluded with `![[partial-name]]`; give the file `partial: true` so it is not also published as a page.
- In `footer.md`, each heading starts a column: the heading and what follows it, up to the next heading of the same or higher level, become one `<section>`, and the default theme lays the sections side by side. A `footer.md` with no headings stays one flow of paragraphs. See `moss guide authoring`.
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
- **Authoring or styling a site** → read `moss guide authoring` (hero, gallery, captions, definition lists and listings are there too).
- **Building a plugin** (deploy, syndicate, comments — anything needing the
  network or an external API) → read `moss guide plugins`.
- **Converting an existing site** (a URL, or local files) into moss → read
  `moss guide importing`.
- **Importing an existing site and redesigning it**, so the moss site looks better than the original while the folder stays easy to edit → read `moss guide import-and-redesign`.
- **Debugging a build, plugin, or shortcode** → read
  `moss guide debugging`.
- **Reviewing or hiding a site's comments (spam) before publishing** → read
  `moss guide comments`.
- **Building a site in more than one language** →
  `moss guide authoring`, Multilingual sites.
- **Putting the site online** → Publishing below. moss publishes to mosspub.com, to other built-in targets, or anywhere a deploy plugin reaches.

## Publishing

- **First publish (mosspub.com hosting, provided by Symbiosis Lab):** `moss env production <folder>` (or
  `staging`) **then** `moss deploy <folder> --site-id=<name>` → publishes to
  `<name>.mosspub.com`. The order matters: without a prior explicit `moss env`,
  `deploy` will not register a site, and `--site-id` alone silently does
  nothing. The hosting service can also refuse a new site with a site limit, including a limit of 0 when it is taking no new sites.
- **First publish refused as "Identity not yet verified":** the folder's key is
  not linked to an account yet. `moss deploy <folder> --site-id=<name> --email
  <address>` emails a sign-in link, waits up to 15 minutes for it to be opened,
  then registers. Ask the human for the address first, and use the account the
  site should belong to: a wrong one can leave the folder's key linked to the
  wrong account. Never pass `--email` unasked; it sends a real email.
- **Every publish after the first:** `moss deploy <folder>` — the site id
  already lives in `.moss/state.toml`.
- **Refused because the live site is newer:** `deploy` to mosspub.com hosting
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
- **Custom domain:** `moss domain link <folder> example.com` (`www.example.com`
  works too) links the bare domain and `www`, and for a domain registered elsewhere
  prints the DNS records to type at the registrar: leave MX and other TXT records
  alone. `moss domain status <folder> example.com` reads live DNS and the
  certificate and says what is left; it exits 0 when live, 3 while waiting, 4 if it
  failed. All three take `--json`. Only a registered domain of two labels is
  supported, not `blog.example.com`. `moss domain list <folder>` shows what is linked.
- **Site built by another generator:** `moss deploy <folder> --prebuilt=_site`
  skips `moss build` and uploads that directory as-is.
- **Any other host:** `moss build <folder> --site-url=https://example.com`,
  then upload `.moss/build.nosync/current/` yourself, after deleting the pages marked `draft: true`, which are built there for the preview (`moss list` shows which they are). `--site-url` is required there
  because canonical URLs, `og:image`, the sitemap, and RSS are all absolute and
  otherwise derive from mosspub.com deployment state, which any other host
  never sets. Flag details: `moss describe --json`.

## Harvest, don't unilaterally invent

If a pattern recurs across pages, tell the human — don't canonicalize a new
class yourself. Site-local classes with clear CSS are fine; minting a
contract-shaped name from one site's habit is not.
