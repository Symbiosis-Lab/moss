# Plugin listing styles in body embeds

**Status:** Proposed. Not scheduled: no site has asked for it yet, and `style:stream` covers the case that raised it.

## What it is

A plugin can provide a listing style (`children_style: <style>` on a folder's home page), and moss hands it a record of the folder's pages, bodies included, to draw. Only a folder's own listing can use one. A folder embed in a page body (`![[posts/|style:<style>]]`) draws moss's own styles only.

## Why it is not built yet

The case that raised it, a stream of short posts drawn whole on a site's home and on the posts folder's own page, is moss's own `stream` style: it shares the body rebasing and title stripping the plugin record uses, and needs no plugin. Running plugin styles in embeds needs the plugin call moved from the folder-home pass into marker expansion, a cache key per embed, and the same feature absorption `stream` does, so the host page loads the scripts and styles the listed bodies need. That is worth doing when a plugin style is first wanted in a body.

## The likely shape

An embed's `style:` value that is not one of moss's styles is looked up among the installed plugins' listing styles. The plugin receives the same record a folder home gets, with the embedding page as `page` and the embed's `limit:`, `sort:` and filters already applied. Its output is placed where the embed was, and the pages it lists add their features to the host page.
