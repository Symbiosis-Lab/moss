---
weight: 30
uid: 939e6b3e
translationKey: docs-author-frontmatter
description: Set page and site properties with a few lines of YAML at the top of a file.
url: frontmatter
---
Frontmatter is a block of YAML at the very top of a markdown file, between two `---` lines, that tells moss the page's title, date, visibility, and more.

```yaml
---
title: My First Post
date: 2024-06-15
description: A short summary for list previews and search engines.
---

The body goes here.
```

Frontmatter is optional. Without it, the file still becomes a page, and moss uses the filename as the title. moss only recognizes the fields listed below; other custom fields are ignored.

## Name files by their titles

Every file's name becomes the page title by default: `About me.md` is titled "About me". Adding `title: About the author` overrides it.

When a filename contains spaces (or non-Latin characters), pin a short, stable URL with `url:`: give `About me.md` a `url: about`, and it publishes at `/about`. The filename is for you, organizing your folder; `url:` is the address readers see. Pin it, and renaming the file never breaks a link.

## Common fields

**Identity** — describes the page itself.

| Field | What it does |
|------|------|
| `title` | Page title (overrides the one taken from the filename) |
| `description` | Summary for list previews and SEO |
| `date` | Publication date (`YYYY-MM-DD`) |
| `tags` | Content tags |

**Navigation & visibility** — controls where the page appears.

| Field | What it does |
|------|------|
| `nav` | Whether the page appears in the top navigation (root-level pages default to `true`) |
| `weight` | Order in navigation and lists; smaller comes first |
| `draft` | `true` means the page isn't generated at all |
| `unlisted` | Generated, but left out of lists and the sitemap |

**Sections & media** — how a folder page presents its children.

| Field | What it does |
|------|------|
| `children` | Whether to list child pages (defaults to `true`; set `false` to turn off) |
| `children_style` | `list`, `card`, or `summary` |
| `sort` | `date`, `date-asc`, `weight`, `title`, or an explicit order |
| `cover` | Cover image for cards and the page header |

Most folders never need to declare `sort`: moss infers it — pages with dates sort by date, pages with weights by weight, and the rest by title.

## Multilingual sites

moss supports English (`en`), Simplified Chinese (`zh-hans`), and Traditional Chinese (`zh-hant`). Any of them can be your site's default; the others show up in the language switcher at the top of each page.

**One folder per language**: put translations in subfolders named after their language.

```
my-site/
├── index.md          ← default language
├── zh-hans/
│   └── index.md      ← Simplified Chinese
└── zh-hant/
    └── index.md      ← Traditional Chinese
```

When two language versions have different filenames, connect them with `translationKey` so the language switcher knows they belong together:

```yaml
# posts/hello.md          # posts/你好.md
---                       ---
translationKey: hello     translationKey: hello
---                       lang: zh-hans
                          ---
```

## All fields

For advanced fields like `cascade`, `series`, `also_in`, `content_width`, `typesetting`, and `sidebar` — and each field's type and default — see the [Frontmatter reference](/docs/writing/frontmatter/).
