---
title: Navigation & footer
url: navigation
uid: 7a9f22b4
weight: 3
description: Control what appears in the header nav, the footer, and the breadcrumb trail.
translationKey: docs-author-navigation
---

moss generates your site's header navigation and footer from page frontmatter. Mark pages with `nav:` or `footer:` and moss assembles the chrome. No menu file to configure.

## Header nav

Which pages appear in the header bar depends on your site's shape:

- **Organized sites** (at least one subfolder under the project root): every root-level non-index page auto-appears in the nav.
- **Flat sites** (no subfolders): only pages whose filename is a recognized "nav keyword" (`about`, `关于`, or `關於`) auto-appear.

In either mode, explicit frontmatter always wins:

| `nav` value | Effect |
|-------------|--------|
| `true` | Always show in header nav (works from any depth) |
| `false` | Never show in header nav |
| (unset) | Auto-rule above |

Sort order: pages with a `weight` value come first, lowest → highest. Pages without `weight` follow, sorted alphabetically by title.

```yaml
---
title: Services
nav: true
weight: 10
---
```

## Site name

The header shows the site name at the left, linking home. If the home page's own first heading is the same text as the site name, the header drops the repeated name — the reader has just read it once, in the page's own heading. Set `logo:` (see [[frontmatter]]) to keep a logo in that spot regardless; without a logo the link is simply omitted so the header layout doesn't shift.

Every other page keeps the header name, and a home page that opens with prose, a poem, or an image instead of a matching heading keeps it too — the name never disappears from the site outright.

## Breadcrumb

When a page has no nav items around it (e.g., an article deep inside a section), moss shows a breadcrumb trail in place of the site name so visitors can navigate back up the tree.

Breadcrumbs are generated from the folder tree. Do not hand-author a breadcrumb block (no `::: {.breadcrumb}` div) in markdown.

Force it on or off per page with `breadcrumb: true` / `breadcrumb: false` in frontmatter, or cascade it to a whole folder:

```yaml
# posts/index.md
---
title: Posts
cascade:
  breadcrumb: true
---
```

## Footer

Pages with `footer: true` in frontmatter appear as links in the site footer.

```yaml
# privacy.md
---
title: Privacy
footer: true
---
```

moss also auto-adds an RSS feed link to the left footer when your site emits a feed.

### Footer slot

Plugins and themes can emit HTML into the `footer-right` slot — useful for copyright lines, Creative Commons badges, or small legal text.

## Language-scoped navigation

Nav items are scoped to the current page's language tree. On a multilingual site, the header nav shows only pages in the current language. Visitors on the English homepage see the English nav; visitors on the Chinese homepage see the Chinese nav. The language toggle in the header lets them switch trees.

If `nav: true` is set on a page in one language tree, the corresponding page in another tree needs its own `nav: true`. `nav` is per-page, not global.

## Related pages

- [[frontmatter]]: `nav`, `weight`, `footer`, `logo`, `breadcrumb` field reference
- [[Multilingual sites]]: language trees and how nav scopes to them
- [[structure]]: how root-level vs nested pages are determined
