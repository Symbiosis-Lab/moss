---
title: CLI
uid: 4def1236
weight: 10
description: Command-line interface for testing and automation.
translationKey: docs-extend-cli
---

## Commands

<!-- auto:start:cli -->
| Command | Arguments | Description |
|---|---|---|
| `build` | `<folder> [--serve] [--watch] [--no-plugins] [--wait-plugins] [--site-url=<url>]` | Build the site with plugins. Use --no-plugins for fast CI/CD builds. |
| `deploy` | `<folder> [--prebuilt=<dir>] [--site-id=<name>]` | Build and deploy the site to the configured hosting platform. |
| `preview` | `<path>` | Open a GUI preview window. <path> may be a folder or a .md/.markdown file. |
| `edit` | `<path>` | Open GUI preview, then open <path> directly in the editor (agent-friendly scriptable entrypoint). |
| `domain` | `<list\|link> <folder> [<domain>]` | List or link custom domains for a project's site. |
| `env` | `<staging\|production\|local> <folder>  \|  <folder>` | Set or show the hosting environment (staging / production / local) for a project. |
| `describe` | `[--json]` | Print the moss contract surface (tokens, components, frontmatter, plugin hooks, slots, CLI). --json for machine-readable output. |
| `import` | `<url> [<folder>] [-r\|--recursive]  \|  --list <file> [<folder>] [-r\|--recursive]` | Import a URL (or batch of URLs) into a project folder as markdown. |
| `agents` | `[<subcommand>]` | Agent tooling subcommands (internal/experimental). |
<!-- auto:end:cli -->

## CI and automation

moss works headless:

```bash
moss build /path/to/folder --no-plugins
```

The build output is a self-contained static site in `.moss/build.nosync/current/` — standard HTML, CSS, and JS that can be deployed anywhere. (`.moss/build.nosync/current/` is a symlink to the latest generation under `.moss/build.nosync/generations/`.)

## Import

`moss import <url> [folder] [-r]` converts a live page to markdown. Only `http` and `https` URLs are supported. Images land in `assets/imported/`. Pass `-r` to crawl the same domain and path prefix, capped at 200 pages.

Import extracts content and discards the original CSS. Recreate the look in `.moss/theme/style.css`.

## Development

For contributors working on moss itself:

```bash
# Start dev server with hot reload
npm run dev

# Preview a folder via CLI (routes to running dev instance)
npm run moss -- preview ~/Sites/my-blog

# Switch folders without restarting
npm run moss -- preview ~/Sites/other-folder
```

The single-instance plugin routes CLI commands to the running dev instance, so you can switch folders without recompilation.
