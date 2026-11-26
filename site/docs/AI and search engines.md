---
title: AI and search engines
url: ai
uid: "e9941d1a"
weight: 15
description: "Choose whether search engines and AI companies may read your site, answer from it or train on it."
---

> New sites ask AI companies not to train on them. One line in your settings changes that.

Three settings under `[site]` in `.moss/config.toml` say what programs that read your site are asked to do. Each covers one use of your pages.

```toml
[site]
search_engines = true   # search engines list your pages
ai_answers = true       # an AI assistant reads a page to answer someone's question
ai_training = false     # AI companies train models on your pages
```

To let AI companies train on your site, set `ai_training = true` and publish again. To keep them out, leave it `false`.

These are requests. A crawler that ignores them can still read the site; only a login keeps a page private.

## What `ai_training = false` writes

Setting `ai_training = false` writes:

- `robots.txt` asks the known training crawlers not to read the site.
- Every page's robots tag gets `noai, noimageai`.
- `/.well-known/tdmrep.json` reserves text-and-data mining. It is written with `0` when training is allowed.

A site that sets none of the three switches gets only the `robots.txt` request.

## The other two

`search_engines = false` puts `noindex` on every page and asks the AI search crawlers not to read the site. `ai_answers = false` asks assistants such as ChatGPT-User and Claude-User not to fetch pages, and stops publishing `llms.txt`.

To set all three at once, `ai_policy` takes `"standard"` (the default: search and AI answers allowed, training asked not to), `"unrestricted"` or `"restricted"`. A switch you set wins over it.
