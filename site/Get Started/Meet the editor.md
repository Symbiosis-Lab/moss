---
uid: c91e6a42
title: Meet the editor
description: Create pages, set their properties, and write with Markdown in moss.
url: editor
weight: 10
translationKey: docs-editor-intro
---
The editor keeps your site folder above the page you are writing. The path at the top is both a breadcrumb and a compact file tree; below it are the page's properties, title, Markdown body, and writing tools.

<moss-stage>
</moss-stage>

<noscript><img width="760" height="420" loading="lazy" src="../assets/guides/editor-ui-source.png" alt="Open the moss editor demo"></noscript>

## Find and create pages

Click the site root in the breadcrumb to expand the file tree, then click a page to open it.<moss-scene name="tree"><noscript><img width="520" height="520" loading="lazy" src="../assets/guides/choose-page-source.png" alt="The real moss file tree expanded from the breadcrumb, with The Tyger selected"></noscript></moss-scene>

To add a page beside the current one, click **New Page** above the tree. Its **▾** also creates a **New Folder** or a page from a saved template.<moss-scene name="create-page"></moss-scene>

Drag the tree's bottom border to show more or fewer files. Double-click that border to collapse the tree; double-click it again to reopen it.<moss-scene name="collapse-tree"></moss-scene>

Your folder structure is also your site structure. A new folder becomes a section, and a Markdown file becomes a page. See [Site structure](/docs/writing/structure/) for the exact file-to-URL rules.

## Start from a template

To save a page for reuse, right-click it and choose **Save as Template…**, then name it and press Return. To save a whole folder shape, use that command on the folder's home file; moss marks it as a folder template.<moss-scene name="save-as-template"></moss-scene>

To use one, open the **New Page options** menu beside the new-page button and choose the named template under **From template**. A page template creates a new page; a folder template creates a new folder with its saved contents. **Manage Templates…** in the same menu renames or deletes saved templates.<moss-scene name="new-from-template"></moss-scene>

## Set page properties

The chips above the page are frontmatter properties: title, date, cover, visibility, and other settings that affect the page or its place in the site. Click a chip to edit it. Click **+** and search for one, such as **Cover**, then press Return to add it. moss stores the result as YAML frontmatter at the top of the Markdown file.<moss-scene name="properties"><noscript><img width="760" height="420" loading="lazy" src="../assets/guides/editor-ui-source.png" alt="A real moss page showing its breadcrumb, Date property chip, filename-derived title, Markdown body, and bottom toolbar"></noscript></moss-scene>

The large title uses the filename by default, so editing it renames the file and updates wikilinks to it. Add a `title` property only when the public title should differ from the filename; after that, editing the large title changes the property instead. The complete list is in [[Define pages with frontmatter|Page properties and frontmatter]].

## Write the page

Write ordinary Markdown in the main editor. Select text for the floating formatting toolbar, type `/` at the start of a line for the insert menu, or use the toolbar at the bottom. The source stays a normal `.md` file; [[Write with Markdown|the Markdown guide]] covers headings, lists, links, and other basics.

moss layouts and components are fenced with `:::`. Insert one from the `/` menu or write it directly; see [[Lay out with shortcodes|Shortcodes]] for examples.

Drag local files from Finder into the editor to copy and insert them. Images, audio, video, PDFs, notebooks, HTML, and other supported media can also be linked or embedded with wikilinks. Type `[[` to search the site, use `[[page]]` for a link, and add `!` as in `![[image.jpg]]` to embed. See [[Reference files & media|Links and media]] for the full syntax and supported file types.

## Save and restore versions

Right-click a page, folder, or the site root and choose **Versions…**. The scope follows what you clicked, and moss also saves a version automatically whenever you publish. Click **Save a version**, then confirm to save one immediately with an optional name. Open a version to see what changed, then click **Restore** to bring back that page, folder, or the whole site — moss saves the current state first, so you can undo it.<moss-scene name="versions"><noscript><img width="760" height="420" loading="lazy" src="../assets/guides/editor-ui-source.png" alt="The real moss editor showing a saved version's diff, with Restore ready"></noscript></moss-scene>

For keyboard commands, see [Editor shortcuts](/docs/writing/editor/). When the page is ready, check it in the live preview and publish from the preview pane.
