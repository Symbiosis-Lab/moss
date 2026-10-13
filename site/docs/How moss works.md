---
translationKey: docs
description: How moss turns a folder of files into a website — the mental model.
uid: b4d68dc8
weight: 1
title: How moss works
url: how-moss-works
---

moss turns a folder into a website. There's no project to set up and no build to configure. You point moss at a folder, and it mirrors that folder as a site.

## Folders and files become pages

- Every folder becomes a page that [[frontmatter#Children|lists its children]] by default. A folder full of [[media|media files]] becomes a gallery, with images and videos compressed and converted for the web.
- Every `.md` file becomes a page. Its URL follows its path in the folder, unless you [[frontmatter#Advanced|override]] it with `url` in frontmatter.
- `index.md`, `readme.md`, `_index.md`, `main.md`, or a file named after its folder (e.g. `recipes.md` inside `recipes/`) become the [[structure#^folder-page|folder's page]]. Language-suffixed variants like `index.zh-hans.md` also work.
- Reference a media or markdown file by a relative path or just its filename. moss resolves it to the nearest match in the file tree.

```
my-site/
├── index.md        ← your homepage
├── about.md        ← /about/
└── posts/
    ├── index.md    ← /posts/
    └── hello.md    ← /posts/hello/
```

## What moss does for you

When you preview or publish, moss reads the folder, converts your Markdown to pages, tidies your images and videos for the web, and assembles a complete static website. It writes the result into a hidden `.moss/` folder inside your project; nothing else in your folder changes. The same build runs whether you preview locally or publish, so the site you see is the site you ship.

You don't configure any of this. moss creates its settings on first run, picks sensible defaults, and stays out of the way. To change how the site looks, [[Write a theme|write a theme]]. For new behavior, [[Write a plugin|write a plugin]]. Everything else is your writing.

## Next

Ready to put a folder online? [Get Started](/docs/) takes about five minutes. To see exactly how files, folders, and URLs map to your site, read [[structure]].
