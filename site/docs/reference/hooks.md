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
        → syndicate hooks (POSSE to platforms)
```

`login` runs outside this pipeline: moss invokes it once, via `connect_account`, when the user starts a connection flow — not on every build.

Plugins attach to stages by declaring **capabilities** in their manifest.

## Hook reference

<!-- auto:start:hooks -->
| Hook | Arity | Context | Description |
|---|---|---|---|
| `process` | multiple | `ProcessContext` | Pre-process source files before generation (e.g. download remote assets, transform markdown). |
| `deploy` | single | `DeployContext` | Deploy the built site to a hosting platform (e.g. GitHub Pages, Netlify). |
| `syndicate` | multiple | `SyndicateContext` | POSSE-distribute published content after deployment (e.g. cross-post to Matters, RSS). |
| `login` | multiple | `BaseContext` | Connect a user account for a plugin that needs one (e.g. Matters); invoked once via `connect_account`, not part of the build pipeline. |
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

The generated source-to-output map (paths plus uids) is at `.moss/build.nosync/article-map.json`.

## deploy

Pushes the compiled site to a hosting platform. **Only one plugin** can have this capability.

Use for: GitHub Pages, Netlify, or custom hosting.

**Context:** Includes `site_files` (all compiled output), `deployment` info, and `domain`.

## syndicate

Distributes published content to external platforms (POSSE). Multiple plugins can have this capability.

Use for: cross-posting to Matters.town, Substack, social media.

**Context:** Includes `articles` (published URLs and metadata) and deployment info.

## login

Connects a user account for a plugin that needs one to operate (e.g. Matters). Not a build-pipeline stage: moss calls it once via `connect_account` when the user starts a connection flow — clicking "Connect" in the plugin's settings row, or the auto-open nudge on a folder with no bound session — not on every build.

Use for: authenticating with a remote platform before `process` or `syndicate` can act on the user's behalf.

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
