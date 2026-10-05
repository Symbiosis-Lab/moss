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
| `build` | `<folder> [--serve [--watch]] [--no-plugins] [--allow-plugins] [--wait-plugins] [--strict] [--site-url=<url>]` | Build the site with plugins. --serve starts a local preview server when the build finishes, and --watch, which only works with --serve, rebuilds on every file change and refreshes the preview. Use --no-plugins for fast CI/CD builds. A plugin that came with the folder and was never allowed in the moss app is refused unless --allow-plugins is passed. --strict exits 1 if the build reported any problems (they are printed to stderr and summarized as `moss: N problems …`); without it a build with warnings still exits 0. |
| `deploy` | `<folder> [--prebuilt=<dir>] [--site-id=<name>] [--allow-plugins] [--overwrite-newer] [--dry-run] [--accept-removals]` | Build and deploy the site. Where it goes is the folder's to say: a prebuilt directory wins, then a `[hooks] deploy` plugin, otherwise moss hosting. A folder publishing for the first time needs `moss env <staging\|production\|local>` first, and then registers a site named after the folder (or --site-id) — a site ID cannot be un-minted, so moss will not pick one for a folder that never named an environment. A plugin that came with the folder and was never allowed in the moss app is refused unless --allow-plugins is passed, exactly as in `moss build`. A publish to moss hosting, --prebuilt included, is refused when the live site was published from another copy after this folder last published — deploying would undo that publish. Bring this folder up to date with that copy (for example, git pull) and deploy again, or pass --overwrite-newer to replace the live site anyway. A publish that would stop serving an address the site has served, for a reason other than you deleting its source (a moved page with no redirect, a generated file no longer produced), is refused with each address and its cause; add the redirect it shows under [redirects] in .moss/config.toml, or pass --accept-removals to accept losing exactly those addresses. Run with --dry-run first to see what a deploy would do: it builds exactly as a deploy builds, including any plugin the flags allow (the build may use the network as any build does: a plugin's requests, and link previews fetched from third-party sites), and prints the pages added, edited and deleted, the addresses that would go offline, the upload and remove counts measured against this folder's last publish record (unknown when there is none), and whether the publish check passes. It uploads nothing and records nothing, runs the checks a deploy makes locally (the folder has a site, the config is not from a newer moss), says when a deploy would register a site or create a signing key, exits 1 where those or the removed-address check would refuse the deploy, and says that it did not check what needs the server, such as whether another copy published since. |
| `preview` | `<path>` | Open a GUI preview window, in moss desktop (which `moss desktop install` provides). <path> may be a folder or a .md/.markdown file. |
| `edit` | `<path>` | Open GUI preview in moss desktop (which `moss desktop install` provides), then open <path> directly in the editor (agent-friendly scriptable entrypoint). |
| `desktop` | `install` | Install moss desktop, the GUI editor and preview app. Downloads the signed release from the same update channel the app itself uses and installs it into /Applications. macOS only for now; elsewhere it says where to get moss desktop. It asks before downloading, so it refuses when stdin is not a terminal. |
| `domain` | `<list\|link> <folder> [<domain>]` | List or link custom domains for a project's site. |
| `env` | `<staging\|production\|local> <folder>  \|  <folder>` | Set or show the hosting environment (staging / production / local) for a project. |
| `describe` | `[--json] [--css <selector>]` | Print the moss contract surface (tokens, components, custom properties, frontmatter, plugin hooks, slots, CLI). --json for machine-readable output. --css <selector> prints the default CSS rules mentioning that selector or custom property, with their enclosing media queries and the comments explaining them — what a theme override has to displace. |
| `import` | `<url> [<folder>] [-r\|--recursive]  \|  --list <file> [<folder>] [-r\|--recursive]` | Import a URL (or batch of URLs) into a project folder as markdown. |
| `agents` | `init` | Seed .moss/AGENTS.md with this site's own conventions, for a coding agent to read. moss's own guidance is `moss guide`. |
| `guide` | `[<topic>\|all\|--list]` | Print moss's agent guidance: project conventions, the four styling rungs, the hard rules, and the checks that tell you whether a change worked. No argument prints the entry point; `--list` names the topics (authoring, plugins, debugging, importing, example); `all` prints every topic as one document. Reads nothing but the binary, so it needs neither a project nor a network — and unlike a copy written into a project folder, it cannot describe a different moss than the one running it. |
| `rename` | `<old_path> <new_path>` | Rename a file or folder and rewrite every [[wikilink]] and [text](link) that points at it, project-wide. Scriptable, so it is the safe way to bulk-rename (e.g. localizing filenames) without breaking links. `mv` is an alias. |
| `doctor` | `--math [<folder>]` | Audit a vault without changing it. --math reports every $-span moss would parse as math (these render as their LaTeX source; moss does not typeset math yet), so an imported corpus can be checked before publishing. Escape false positives as \$. |
| `list` | `[<folder>] [--json]` | List every document the last build parsed: source file, the URL it publishes at, the BCP-47 tag it will carry in `<html lang>`, kind, date, title, and — in the HIDDEN column — why it is absent from generated listings (draft, unlisted, nav-item, slot). The LANG column is how you confirm a new language tree registered: an unrecognized folder name is treated as ordinary content and the build succeeds either way (`moss describe --json` lists the recognized codes under `languages`). Reports the last build, so run `moss build` first. Use --json to filter the same rows programmatically. |
| `history` | `[<folder>] [<path>] [--json] \| [<folder>] --save [<name>] [--json] \| [<folder>] <path> --restore --at <id> [--copy] \| [<folder>] --restore --at <id> --yes` | moss's own version history, kept inside the site at `.moss/history` (not git — a git user already has their own history, and this store never adds commits): every landed publish gets a snapshot, and `--save` takes one on demand. With no path, lists the site's timeline newest first: when, whether it was a publish or a named save, a `live` marker on the version currently published, and what changed. With a path, lists that one page's timeline instead, noting when its content was not kept (over the size ceiling, or unreadable at publish time). `--save [<name>]` builds the site and saves a version of it right now, ending with one line naming the version saved; `--save --json` prints that one version as a single JSON object on stdout instead (build progress and warnings still go to stderr), so a script never has to scrape the saved id out of the build log. `--restore --at <id>` restores that version — a path restores just that page (`--copy` writes it beside the current file instead of overwriting it); with no path it restores the whole site, which needs `--yes` since it can move files to the Trash. `<id>` is a version's id from the timeline, or an unambiguous prefix of one. With no folder, this command operates on the site containing the current directory; pass `<folder>` — a directory that already contains `.moss` — to act on a site from outside it, the same as `moss build <folder>`. |
| `comments` | `list [<folder>] [--json] \| hide [<folder>] <id>... [--source <name>] [--json] \| unhide [<folder>] <id>... [--source <name>] [--json]` | Review and hide a site's comments. `list` prints every comment, newest first: id, source, the page it is on (address and title once a build has run, otherwise the page uid), author, time, the first 60 characters of its text, and whether it is hidden; it needs no build. `hide` takes one or more ids from `list` and removes those comments, and the replies under them, from the site at the next publish; `unhide` reverses it. Nothing is erased and nothing is sent anywhere: a hide is a signed event in `.moss/data/social/moderation.jsonl`, signed with the site's own key in `.moss/identity`, which moss never creates for this (no key, no hide). A call validates every id first, so an unknown id changes nothing; an id that exists under two sources needs `--source <name>`; an already-hidden comment is left alone. Nothing reviews new comments before they go live, so list them before publishing and hide the spam. `--json` prints rows, or `{"changed": [...], "unchanged": [...]}` for hide and unhide, and failures as `{"error": ...}`. With no folder, this command operates on the site containing the current directory. |
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
