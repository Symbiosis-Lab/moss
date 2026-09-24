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

### Excluded folders

The following folder names are reserved for static assets and are **not** treated as content:

`assets/`, `images/`, `static/`, `public/`, `img/`, `css/`, `js/`, `fonts/`, `node_modules/`

Any folder whose name starts with `.` (dot) or `_` (underscore) is also excluded.

Files inside these folders are served as-is but will not become pages. Put images here and reference them with `![[filename.ext]]` wikilinks; moss resolves them regardless of where the source page lives.

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

## Visibility

| Setting | Built | In lists | In sitemap |
|---------|-------|----------|------------|
| _(default)_ | yes | yes | yes |
| `unlisted: true` | yes | no | no |
| `draft: true` | no | no | no |

Prefix a folder with `_` to exclude it entirely: `_drafts/` is ignored.

## Sort order

Children are sorted: subfolders first (alphabetically), then by `date` (newest first), then alphabetically. Override with `weight` (lower numbers sort first). ^sort-order

## Navigation

Root-level pages appear in header navigation automatically. Control with `nav: true/false` and `weight`.
