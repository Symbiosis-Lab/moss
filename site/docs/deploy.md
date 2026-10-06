---
title: Deploy
uid: 36d59578
weight: 14
description: Publish your site to GitHub Pages.
translationKey: docs-deploy
---

## Prerequisites

- A GitHub account
- A GitHub repository (public or private)

## Setup

1. Open your folder in moss.
2. Click the settings icon in the toolbar.
3. Under "Publishing", click "Connect GitHub".
4. Authorize moss via GitHub's device flow: a code appears in moss; enter it on GitHub when prompted.
5. Select the repository to deploy to.

## Deploy

Click the deploy button. The first deploy takes about a minute — moss creates a GitHub Actions workflow automatically. Subsequent deploys are faster.

moss commits your compiled site, pushes to the repository, and GitHub Actions deploys it to GitHub Pages.

**Plugin settings:**

| Setting | Default | Description |
|---------|---------|-------------|
| `auto_commit` | `true` | Automatically commit changes on deploy |
| `video_max_size_mb` | `75` | Maximum video file size for deployment |

## When a publish would take an address offline

Every address your site has served is a promise: other sites link to it, and readers subscribe to it. If a publish would stop serving an address, and not because you deleted the page or file behind it, moss refuses to publish and lists each address with what happened to it, for example a page that now lives at a new address, or a generated file such as a feed that the site no longer produces.

There are two ways out. Keep the address working: for a page that moved, add the redirect moss prints under `[redirects]` in `.moss/config.toml`, for example `"/old/" = "/new/"`, and publish again. Or accept losing the addresses: `moss deploy <folder> --accept-removals` publishes anyway and accepts exactly the addresses it listed, so a different address that goes offline later asks again. Addresses you removed by deleting their page or file never ask; they are listed after the publish.

To see what a publish would do before you run it, use `moss deploy <folder> --dry-run`. It builds the site exactly as a publish does, including any plugin you allow with the same flags (the build may use the network as any build does: a plugin's requests, and link previews fetched from third-party sites), and prints the pages that would be added, edited and deleted, the addresses that would go offline, and how many files would be uploaded and removed since this folder's last publish (unknown when there is no record of one). It uploads nothing and records nothing, it says when a publish would register a site or create a signing key, and it exits with an error status where the publish would be refused by a check it ran. It does not ask the server, so it cannot tell you whether another copy of the folder has published since; a passing dry run does not promise the publish goes ahead.

## Review comments before you publish

Nothing reviews a new comment before it goes live: whatever has arrived in your site's comments appears at the next publish, spam included. Before you publish, list them and hide the ones you do not want:

```
moss comments list <folder>
moss comments hide <folder> <id>
```

`list` shows every comment, newest first, with the page it is on, who wrote it, the start of its text and whether it is hidden. `hide` takes one or more ids from that list. A hidden comment, and the replies under it, disappears from the site at the next publish. Nothing is erased and nothing is sent to a server, so `moss comments unhide <folder> <id>` brings a comment back. If two comment sources use the same id, moss asks which one you mean: add `--source <name>`.

Hiding is signed with your site's own key, the one in `.moss/identity`. moss never makes a new key for this, because a new key would stop your earlier hides from applying; on a folder without the key, `hide` fails and says so. In the moss app you can do the same from the preview: hover a comment and click Hide.

## Custom domain

Set a custom domain in your GitHub repository settings under **Pages → Custom domain**, or configure it directly in moss's domain settings.

## What's next

Zero-config deploy to moss.host is planned (no GitHub account required).
