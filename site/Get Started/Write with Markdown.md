---
url: markdown
uid: c579abc5
description: Write in plain text with a few symbols, and moss lays it out as a webpage.
translationKey: docs-editors
weight: 20
---
Markdown is plain text plus a few symbols: `#` makes a heading, `**` makes bold, `-` makes a list. moss reads standard Markdown (CommonMark and GitHub-flavored), and on top of that supports Obsidian-style [[Reference files & media|wikilinks and embeds]], [[Lay out with shortcodes|shortcodes]], and callouts.

Because it's plain text, you can write in any editor, and your files always stay on your own computer.

## Basic syntax

Try the syntax in the real editor below. The example is already filled in and stays still until you edit it.

<moss-ui-demo lang="en" static></moss-ui-demo>

For a full syntax reference, see the [Markdown cheat sheet](https://www.markdownguide.org/basic-syntax/) or [Obsidian's basic formatting syntax](https://help.obsidian.md/syntax).

## Line breaks and paragraphs

In Markdown, starting a new line renders as just a single space after the previous line. To start a new paragraph, leave a blank line between the two.

When laying out poetry and similar forms, to break to a new line without a blank line between, add two spaces at the end of the previous line.

## moss's extensions

Beyond standard Markdown, moss recognizes a few more notations, each with its own page:

- **[[Reference files & media|Wikilinks & embeds]]** — link to and embed other pages and media with `[[double brackets]]`.
- **[[Lay out with shortcodes|Shortcodes]]** — create multi-column layouts, galleries, large images, and more with `:::` fences.
- **[[Define pages with frontmatter|Frontmatter]]** — set title, date, visibility, and other properties in YAML at the top of a file.

## What to write with

moss ships with a WYSIWYG Markdown editor — right-click a folder and choose **Publish** to get started. You can also use any editor you like, such as [Obsidian](https://obsidian.md/), iA Writer, Typora, or VS Code. moss watches the folder and updates the preview the moment you save.
