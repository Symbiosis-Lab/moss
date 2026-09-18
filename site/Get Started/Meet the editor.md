---
uid: c91e6a42
title: Meet the editor
description: Create pages, set their properties, and write with Markdown in moss.
url: editor
weight: 10
translationKey: docs-editor-intro
---
The editor keeps your site folder above the page you are writing. The path at the top is both a breadcrumb and a compact file tree; below it are the page's properties, title, Markdown body, and writing tools.

## Find and create pages

Click a folder in the breadcrumb to expand its files and subfolders. Click a page to open it. To add something beside the current page, right-click the folder or an empty part of the tree and choose **New Page** or **New Folder**.

Drag the tree's bottom border to show more or fewer files. Double-click that border to collapse the tree; double-click it again to reopen it.

![The real moss file tree expanded from the breadcrumb, with The Tyger selected](../assets/guides/choose-page-source.png)

Your folder structure is also your site structure. A new folder becomes a section, and a Markdown file becomes a page. See [Site structure](/docs/writing/structure/) for the exact file-to-URL rules.

## Start from a template

To save a page for reuse, right-click its Markdown file and choose **Save as Template…**, then give it a name. To save a whole folder shape, use that command on the folder's home file; moss marks it as a folder template.

To use one, open the **New Page options** menu beside the new-page button and choose the named template under **From template**. A page template creates a new page; a folder template creates a new folder with its saved contents. **Manage Templates…** in the same menu renames or deletes saved templates.

## Set page properties

The chips above the page are frontmatter properties: title, date, cover, visibility, and other settings that affect the page or its place in the site. Click a chip to edit it, or click **+** to add a property. moss stores the result as YAML frontmatter at the top of the Markdown file.

The large title uses the filename by default, so editing it renames the file and updates wikilinks to it. Add a `title` property only when the public title should differ from the filename; after that, editing the large title changes the property instead. The complete list is in [[Define pages with frontmatter|Page properties and frontmatter]].

![A real moss page showing its breadcrumb, Date property chip, filename-derived title, Markdown body, and bottom toolbar](../assets/guides/editor-ui-source.png)

## Write the page

Write ordinary Markdown in the main editor. Select text for the floating formatting toolbar, type `/` at the start of a line for the insert menu, or use the toolbar at the bottom. The source stays a normal `.md` file; [[Write with Markdown|the Markdown guide]] covers headings, lists, links, and other basics.

moss layouts and components are fenced with `:::`. Insert one from the `/` menu or write it directly; see [[Lay out with shortcodes|Shortcodes]] for examples.

Drag local files from Finder into the editor to copy and insert them. Images, audio, video, PDFs, notebooks, HTML, and other supported media can also be linked or embedded with wikilinks. Type `[[` to search the site, use `[[page]]` for a link, and add `!` as in `![[image.jpg]]` to embed. See [[Reference files & media|Links and media]] for the full syntax and supported file types.

## Save and restore versions

Right-click a page, folder, or the site root and choose **Versions…**. The scope follows what you clicked. moss saves a version whenever you publish, and **Save a version** makes one immediately with an optional name. Open a saved version to see what changed or restore that page, folder, or the whole site. Before a restore, moss saves the current state so you can undo it.

For keyboard commands, see [Editor shortcuts](/docs/writing/editor/). When the page is ready, check it in the live preview and publish from the preview pane.
