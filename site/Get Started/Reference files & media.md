---
uid: 252bd472
description: Link pages, embed content, and reference media with double brackets — no paths required.
url: links
weight: 40
translationKey: docs-author-links
---
moss lets you connect pages and add media without hand-maintaining site URLs. Use a wikilink when you want moss to find a file by name, title, or media name; use standard Markdown when you already know the path.

Use `[[page]]` for a link and `![](photo.jpg)` for an image, video, audio, PDF, HTML or 3D model file in your site; write the path relative to the page. The wikilink form `![[photo.jpg]]` embeds the same file by name alone, and pages, tables, notebooks and folders embed only that way (`![[page]]`). A leading `!` places the target into the page; without it, readers get a normal link. The editor can insert these references for you, and the source remains portable Markdown.

Images, video, audio, PDFs, notebooks, and HTML files can live beside your writing. moss resolves references, prepares supported media for the web, and keeps the original files in your folder. A `.mov` is transcoded to web video; notebooks run in the browser through JupyterLite and therefore add a substantial download for readers.

Use captions and display parameters when a figure needs a specific description, size, fit, or position. Keep public-only files in an asset folder when they should be served without becoming pages.

For exact wikilink, embed, media-format, and display rules, continue with [Links & Embeds](/docs/writing/wikilinks-and-embeds/) and [Media](/docs/writing/media/).
