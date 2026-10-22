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

Set `[site] header = "none"` and there is no header at all: no band, no site name, no nav links. A one-page site, an index of collections each under its own `##` heading, usually wants this, because the band would hold only the site's name (a link to the page you are already on) and the theme toggle. The theme toggle, the search button and the language switcher (whatever the header would have carried besides the name and the links) move into the floating nav island, which a site with no header shows on every page. The island has no breadcrumb trail and no site name: one button names the section you are reading and opens the list of sections, and the controls sit at its end. It stays hidden while the page's title is in view, appears once the first section heading reaches the top of the window, and then stays to the end of the page rather than hiding as you scroll down. A page with fewer than two sections shows the controls alone, from the start. The skip link stays. The page's first element keeps the top space the header would have given it, except on a page that opens with a hero. Without scripting there is no island, so a visitor who cannot run scripts gets neither the toggles nor the language links; keep `header = "brand"` for a multilingual site that serves them.

On a wide screen the page's contents ruler lists the sections in the left margin. Mark where the main sections stop with `contents=end` on a fenced block (`::: {.band screen contents=end}`) or on a heading (`## Colophon {contents=end}`): the ruler lists only the sections before the marker, steps aside once the marker passes the middle of the window, and comes back when the reader scrolls above it. Headings after the marker still appear in the island's sections panel. A long heading can give the contents a shorter name: `## Opening hours of the reading room and its annex {contents-label="Opening hours"}` shows "Opening hours" in the ruler, on the floating bar and in its sections panel, while the heading keeps its full text. The name is quoted when it has spaces, and it combines with `{#id}` and `contents=end` in any order. A name too long for its row ends in an ellipsis. Independently of the marker, the ruler steps aside while a wide block's content reaches its margin (a `screen` picture or gallery), and returns once the block has passed. A band whose words keep the text column covers nothing, so the ruler stays over it. On a page with `contents: pictures`, a wide window with a mouse or trackpad keeps a lane free for the list: the text column narrows so the list never sits on the text, and the floating bar does not show on that page.

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

A footer with nothing to show — no `footer.md`, no `footer: true` pages, no feed link, no subscribe form — doesn't render at all: no empty band, no divider, nothing between your content and moss's own colophon line at the foot of the page. That line is a small moss mark linking to mosspub.com; to leave it out, set `colophon = false` under `[site]` in `.moss/config.toml` and the build emits none.

### Closing picture

End `footer.md` with a `:::hero` and the page closes with that picture, edge to edge, the last thing on the page. The words sit on the picture where `align` puts them (`align=top` is the top, for a sky) at every width, including a phone, and moss's own colophon moves onto the picture's bottom edge. `dark=` names the picture dark mode shows in place of `image=`. A `caption="…"` on the hero is the picture's credit, set small at its foot on the end side. Where the picture is quiet under the words, they are set in dark or light ink straight on it with no panel, and over a busy picture they keep the panel; `panel=always` or `panel=none` forces either. On a phone the words take the top and the picture keeps at least half of its own height below them. Add `fade=top` and the picture rises out of the page: its top fades into the page colour, and with `align=top` the credit joins the words at the top of the sky, beside them on a wide screen and under them on a phone, in a quieter ink.

```
:::hero {image=closing.jpg dark=closing-night.jpg align=top}
## The shelf is open. What should stand on it next?

[Propose a collection](https://example.org/new)

*Credit line for the picture.*
:::
```

The footer drops its column width, padding, divider and fill for this. Anything written above the hero stays in the reading column.

### Footer slot

Plugins can emit HTML into the `footer-end` slot, the trailing position after the link list — used today for the auto-injected email subscribe form. Authors compose the leading position instead, with `footer.md` or a page's own `slot: footer-left` frontmatter.

## Language-scoped navigation

Nav items are scoped to the current page's language tree. On a multilingual site, the header nav shows only pages in the current language. Visitors on the English homepage see the English nav; visitors on the Chinese homepage see the Chinese nav. The language toggle in the header lets them switch trees.

If `nav: true` is set on a page in one language tree, the corresponding page in another tree needs its own `nav: true`. `nav` is per-page, not global.

## Related pages

- [[frontmatter]]: `nav`, `weight`, `footer`, `logo`, `breadcrumb` field reference
- [[Multilingual sites]]: language trees and how nav scopes to them
- [[structure]]: how root-level vs nested pages are determined
