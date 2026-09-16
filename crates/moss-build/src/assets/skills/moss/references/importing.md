# Converting an existing site into moss

## From a URL — `moss import`

```
moss import <url> [folder] [-r|--recursive]
moss import --list <urls.txt> [folder] [-r|--recursive]
```

- Accepts a remote **http/https URL**, a local **`.mhtml`/`.mht` web-archive**
  ("Save Page As"), a local **`.html`/`.htm` file**, or (via `--list`) a text
  file of one-per-line URLs/paths to import in batch.
- Each page becomes a `.md` file with YAML frontmatter (`title`, `date`,
  `author`, `publisher`, `lang`, `description`, `cover`, plus a `syndicated:`
  list holding the source URL). Images download to `assets/imported/`.
- `syndicated:` is the POSSE mirror field: an import is the user's own content
  republished here, so the local copy is canonical and the source URL is
  recorded as a syndication mirror. `moss import` does **not** set
  `external_url` — that is the manual linkblog field, which points cards,
  canonical and sitemap off-site. (Pages that fail to fetch are the one
  exception: they get a `title`/`external_url`/`scrape_error` stub.)
- `--recursive` crawls **same-domain, same-path-prefix** links (capped at 200
  pages) and rewrites in-scope links to relative `.md` paths.

**Important:** import extracts **content** and discards the original CSS and
design — you get raw markdown, not the original look. `moss import` does not
create a `.moss/` project and does not follow the folder-is-the-site
convention. After importing, your job is:

1. **Arrange files** into the canonical shape (a folder per section; a home
   file named `index.md` or after its folder).
2. **Clean up markup** — move any inline styling into `.moss/theme/style.css`;
   convert raw HTML to `::: {.class}` fenced divs; convert image links to
   `![[file.ext]]`.
3. **Design transfer (if you need the original look):** `moss import` discards
   CSS deliberately. To recreate the original visual style, do this as a
   separate step: inspect the original site's styling (computed CSS via browser
   devtools, or a screenshot), then recreate the relevant rules in
   `.moss/theme/style.css`. This is intentional design work, not part of the
   content scrape.
4. `moss preview <folder>` to build and check, then apply the authoring
   discipline in `moss guide authoring`. That opens moss desktop, which an
   agent cannot see — use `moss build <folder> --serve` instead.

## From local files

There is **no** local-import command. Two options:

- **Hand-convert (preferred for a few pages):** read the local HTML/files and
  write canonical moss markdown + `.moss/theme/` yourself, following
  `moss guide authoring`.
- **Temp-serve then import (for a whole local site):** serve the local files
  over HTTP (e.g. `python3 -m http.server`) and run
  `moss import http://localhost:8000/ <folder> --recursive`, then do the cleanup
  steps above.
