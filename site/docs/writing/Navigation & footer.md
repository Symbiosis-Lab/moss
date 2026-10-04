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

## Header mode

`[site] header` in `.moss/config.toml` picks what leads the header, left to right. The default, `"brand"`, is what you've already seen above: the site name (or a breadcrumb trail, on a page with no nav items around it) links home, with the nav items and the theme toggle to its right.

Set `[site] header = "nav"` and the brand drops out of the header entirely, on every page — home page included. The nav opens instead with a Home link, followed by the site's own nav items in the usual weight order, all left-aligned; the theme toggle (and search, if enabled) stay on the right. The Home link carries `aria-current="page"` while you're on the home page, the same signal any other nav item gets when it's the exact page you're viewing.

This mode has no breadcrumb trail — Home already gives every page a way back — so a page nested inside a section, deeper than that section's own nav item, has no other "where am I" cue in the header. The nav item for that section marks itself instead: `aria-current="true"` rather than `"page"`, using the same visual treatment as an exact match. Viewing `/essays/2024/some-post/`, for example, highlights the "Essays" nav item even though the page itself isn't `/essays/`.

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

Set `[site] rss_footer = true` in `.moss/config.toml` to add an RSS feed link to the left footer. It only appears once your site actually has a feed — a preview or otherwise undeployed build resolves no site URL and writes no `rss.xml`, so the toggle alone never links to a feed that isn't there. The feed lives at `/rss.xml`; the same feed is also served at `/feed.xml`, its address before it moved, so existing subscribers keep working.

A footer with nothing to show — no `footer.md`, no `footer: true` pages, no feed link, no subscribe form — doesn't render at all: no empty band, no divider, nothing between your content and moss's own colophon line at the foot of the page.

### Footer slot

Plugins can emit HTML into the `footer-end` slot, the trailing position after the link list — used today for the auto-injected email subscribe form. Authors compose the leading position instead, with `footer.md` or a page's own `slot: footer-left` frontmatter.

## Language-scoped navigation

Nav items are scoped to the current page's language tree. On a multilingual site, the header nav shows only pages in the current language. Visitors on the English homepage see the English nav; visitors on the Chinese homepage see the Chinese nav. The language toggle in the header lets them switch trees.

If `nav: true` is set on a page in one language tree, the corresponding page in another tree needs its own `nav: true`. `nav` is per-page, not global.

## Related pages

- [[frontmatter]]: `nav`, `weight`, `footer`, `logo`, `breadcrumb` field reference
- [[Multilingual sites]]: language trees and how nav scopes to them
- [[structure]]: how root-level vs nested pages are determined
