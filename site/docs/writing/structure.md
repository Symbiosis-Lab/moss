---
title: Structure
uid: d37e0621
weight: 2
description: How files and folders become pages, sections, and navigation.
translationKey: docs-structure
---

Every `.md` file in your folder becomes a page. Every subfolder becomes a section.

## Folder tree = site tree

moss maps your folder tree directly to your URL tree. A file at `work/farewell.md` publishes at `/work/farewell/`. Folders become site sections; navigation, breadcrumbs, and permalinks follow the filesystem. To add a new section, create a folder. To nest pages, nest folders.

**Example site tree and resulting URLs:**

```
work/
├── index.md            →  /work/
├── farewell.md         →  /work/farewell/
└── orchard/
    ├── index.md        →  /work/orchard/
    └── gallery.md      →  /work/orchard/gallery/
about.md                →  /about/
index.md                →  /
```

Set `breadcrumb: true` on the homepage and every page gets an automatic trail (`Home / Work / Farewell`). No hand-rolled breadcrumb blocks needed.

### Skipped folders

moss skips `node_modules/` and any folder whose name starts with `.` (such as `.git`): their Markdown never becomes pages and their files are not copied into the built site.

Every other folder is part of the site, whatever its name, including `assets/`, `images/` and `static/`. Markdown inside it becomes pages, other files are served at their path (`assets/portrait.jpg` stays at `/assets/portrait.jpg`), and a folder that holds no Markdown, such as `assets/`, still gets a page of its own at `/assets/`. Reference an image with `![](assets/filename.ext)` or the `![[filename.ext]]` wikilink; moss resolves it regardless of where the source page lives.

## Folder pages

`index.md` in any folder becomes that folder's page. moss also recognizes `readme.md`, `_index.md`, and `main.md` (checked in that order, case-insensitive). ^folder-page

A file named after its parent folder also works:

```
recipes/
├── recipes.md   ← folder page (same name as folder)
├── pasta.md
└── soup.md
```

When a folder has no `index.md`, moss generates a page that lists its children.

The root `index.md` is your **homepage**.

## URLs

Each file's path determines its URL:

| File | URL |
|------|-----|
| `index.md` | `/` |
| `about.md` | `/about/` |
| `posts/index.md` | `/posts/` |
| `posts/hello.md` | `/posts/hello/` |

Override with `url` in frontmatter:

```yaml
---
title: Hello World
url: /blog/2024/hello/
---
```

### When an address moves

Renaming a page that has a `uid` redirects its old address on its own. For anything else — a hand-made `.html` file you moved, a page you removed or merged, a link out to another site — say where the old address should go in `.moss/config.toml`:

```toml
[redirects]
"/scale-compare.html" = "/assets/scale-compare.html"
"/old-post/" = "/writings/new-post/"
"/shop/" = "https://example.com/shop"
```

Each line is the old address on the left and the new one on the right, both starting with `/`. The new address is either something your site serves or a full `https://` link. A page address (`/old-post/`, or `/old-post`) and a `.html` file get a small page that forwards visitors, so it works on any host. Any other file, such as a PDF or a spreadsheet, is replaced by a copy of the new file, so the new address has to be a file on your site; pointing a file at a page or at another site cannot be done without a server's help, and moss tells you so. These forms work on any host. An old address whose folder names have capitals or spaces (`/Old Post/`) cannot be written as a file path, so it has no forwarding page: it is listed in the redirect table and works only on a host that reads it, and moss says so. If your site really serves the old address, that wins and the line is skipped with a note. The same list is published as `_moss/redirects.json` for hosts that can answer with a true permanent redirect.

## Visibility

| Setting | Built | In lists | In sitemap |
|---------|-------|----------|------------|
| _(default)_ | yes | yes | yes |
| `listed: false` | yes | no | no |
| `draft: true` | yes | no | no |

A draft is still published at its own address: it is left out of lists, feeds and the sitemap and marked `noindex`, but anyone who has the link can open it. A page with `listed: false` is left out of the same places but not marked `noindex`. To keep a file out of the site entirely, start its name, or the name of its folder, with a `.`. A leading `_` does not hide anything: `_drafts/` is built like any other folder, at `/drafts/`.

## Sort order

Children are sorted: subfolders first (alphabetically), then by `date` (newest first), then alphabetically. Override with `weight` (lower numbers sort first). ^sort-order

## Navigation

Root-level pages appear in header navigation automatically. Control with `nav: true/false` and `weight`.
