---
title: Hooks
uid: 2d40c48f
weight: 21
description: "The plugin lifecycle: four capabilities and their contexts."
translationKey: docs-extend-hooks
---

## Build pipeline

When moss compiles a site, it runs through these stages in order:

```
Scan folder
  → process hooks (pre-generation)
    → moss builds HTML (no plugin hook)
      → deploy hook (push to hosting)
        → copy hooks (a copy of each new post on other networks)
```

`login` runs outside this pipeline: moss invokes it once, via `connect_account`, when the user starts a connection flow — not on every build.

Plugins attach to stages by declaring **capabilities** in their manifest.

## Hook reference

<!-- auto:start:hooks -->
| Hook | Arity | Context | Description |
|---|---|---|---|
| `process` | multiple | `ProcessContext` | Pre-process source files before generation (e.g. download remote assets, transform markdown). |
| `deploy` | single | `DeployContext` | Deploy the built site to a hosting platform (e.g. GitHub Pages, Netlify). |
| `publish_copy` | multiple | `CopyJob` | Post one copy of a post to a channel's network once its page is live (contributes.channel.syndication); returns the copy's id and address. |
| `update_copy` | multiple | `CopyJob` | Edit a posted copy after its post's source changed; optional, absent means the network cannot edit a copy. |
| `find_copy` | multiple | `CopyJob` | Look a copy up after a send whose outcome is unknown, or one handed off to the author; optional. Answers the copy, a `handoff` still waiting, or `absent: true`; anything else leaves it unknown, and only `absent` lets moss send again. |
| `copy_account` | multiple | `AccountQuery` | Say which account a channel is signed in to, as `copy.account` (and `copy.name`, shown to the person), or `copy.signed_out: true` when its session or credential was read and holds none; optional. Asked once per run before a channel's copies are worked on; a copy sent from another account is left alone until that account is signed in again, and a signed-out channel's copies wait for a sign-in. A read that failed or is not available on this host is a failed call, never `signed_out`. |
| `listing` | multiple | `ListingContext` | Draw a folder's listing in a style the plugin contributes (contributes.listing_styles); runs after render, once per folder home page whose children_style names one of the plugin's styles. |
| `login` | multiple | `BaseContext` | Sign the person in to a channel's account (contributes.channel.login: true) when they press Sign in, never during a build. Answers a `LoginResult`: `completed` with the account's stable `id` (and `name`, shown), which moss records as the folder's account in `.moss/state.toml`, `cancelled`, or `failed` with a `message`. A `login` that answers nothing is read from its own config, as before. |
<!-- auto:end:hooks -->

## process

Runs before HTML generation. Multiple plugins can have this capability.

Use for: fetching external data, transforming source files, pre-processing content.

**Context passed to the hook:**

| Field | Type | Description |
|-------|------|-------------|
| `project_path` | string | Absolute path to the project folder |
| `moss_dir` | string | Path to `.moss/` directory |
| `project_info` | object | `total_files`, `homepage_file`, `site_name`, `lang` |
| `config` | object | Plugin configuration values |

**Data contract:** A process hook that fetches external data writes JSON to `.moss/data/social/<plugin>.json`. The build core does not read this file; moss's native slot-filling and other plugins consume it.

The generated source-to-output map (paths plus uids) is `article-map.json` in the build output's root, the folder above the `output` path `moss build --json` prints.

## deploy

Pushes the compiled site to a hosting platform. **Only one plugin** can have this capability.

Use for: GitHub Pages, Netlify, or custom hosting.

**Context:** Includes `site_files` (all compiled output), `deployment` info, and `domain`.

## The copy hooks

A channel posts a copy of each new post to another network (POSSE: publish on your own site, syndicate elsewhere). It declares what its network carries under `contributes.channel.syndication`, and moss decides which posts are due, shapes each copy to the network's limits, never posts one twice, and writes each copy's address back into the post's `syndicated:`. The channel exports one function per operation, each handling exactly one copy: `publish_copy`, and optionally `update_copy` and `find_copy`. Deleting a post leaves its copies where they are. A channel that exports `copy_account` keeps each copy with the account it was sent from.

Use for: cross-posting to Bluesky, Mastodon, Matters.town, LinkedIn, X; a hand-off for a network with no posting API.

**Context:** a `CopyJob`: the site's address and name, the post's address, title, language, date and tags, the form moss chose, and the copy already shaped (a long article also as plain text). The full contract is in the [syndication design](https://github.com/Symbiosis-Lab/moss/tree/develop/docs/proposals/syndication).

## login

Signs the person in to a channel's account (e.g. Matters). Not a build-pipeline stage: moss calls it once via `connect_account` when the person starts a sign-in, from the channel's settings or the prompt on a folder with no signed-in account, not on every build. The channel declares it as `contributes.channel.login: true`.

How it is known that a channel is signed in: its `copy_account` answer. `copy.account` (with `copy.name`, shown to the person) means signed in; `copy.signed_out: true` means its session or credential was read and holds none, or the service refused authentication. A read that failed, or that is not available where moss runs (the webview's cookies exist only in the desktop app, not in a terminal), is `success: false`, never `signed_out`, so it leaves the recorded sign-in as it was. moss records each definite answer per channel on this computer, outside the site folder.

An OAuth channel signs in through the person's default browser, where they are usually signed in already: `startBrowserSignIn()` from `@symbiosis-lab/moss-api` listens once on `127.0.0.1` and gives the redirect address to register, and its `open(authorizeUrl)` opens the provider's page and resolves with the redirect's query. The plugin checks `state` and exchanges the code; `crypto.subtle.digest` makes the PKCE challenge. moss opens only `https` addresses (and `http` to a loopback address) in the browser. A sign-in ends after ten minutes, when the person presses Cancel, when the channel starts another, or in a terminal at once ("Sign in from the moss app"). A token the sign-in earns is a setting with `"type": "secret", "hidden": true`, which signing out removes.

`login` answers a `LoginResult`: `{ status: "completed", account: { id, name } }`, `{ status: "cancelled" }` or `{ status: "failed", message }`. A channel whose accounts live on many servers, as Mastodon's do, asks which server inside its own sign-in page and adds `account.host`, the account's server: moss records it with the account and takes a `syndicated:` address on that host as the channel's copy.

Use for: authenticating with a remote platform before `process` or a copy hook can act on the user's behalf.

**Context:** `BaseContext` — `project_info` and the plugin's resolved `config`.

## Plugin runtime

Plugins run in the Tauri webview. The lifecycle:

1. Rust backend sends plugin code and manifest.
2. Plugin code is injected as a `<script>` tag.
3. Plugin creates a global object (e.g., `window.MattersPlugin`).
4. moss calls `onload({ project_path, config })` if defined.
5. Hooks are called with their respective contexts.
6. Results are sent back to the Rust backend.

`console.log`, `console.warn`, and `console.error` from plugins are forwarded to the moss terminal.

## Plugin modes

How plugins run depends on the compilation mode:

| Mode | Behavior |
|------|----------|
| **Blocking** | `moss compile`: waits for process hooks to complete |
| **NonBlocking** | Preview mode: fires process hooks but doesn't wait |
| **SlotsOnly** | Watch rebuilds: skips `process` hooks, renders native template slots only |
| **Skip** | `--no-plugins`: bypasses all plugin hooks |
