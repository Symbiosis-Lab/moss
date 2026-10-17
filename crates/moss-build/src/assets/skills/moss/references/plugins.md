# Building a moss plugin

## When something is a plugin (not core)

Core turns a folder into a site with zero external input. A plugin is for the
**conversation with the outside world**: deploying, syndicating, pulling
comments or review metadata, importing from an external service. The test: if
the feature needs a network call, an API key, or an external server, it is a
plugin.

## Anatomy

A plugin is an npm package named `@symbiosis-lab/moss-plugin-<name>` that
bundles to a single IIFE. Two files matter:

- **`manifest.json`** — the contract moss reads. For the full field list, which
  is required, and what each means:

  ```
  moss describe --json | jq '.manifest_fields'
  ```

  Read it rather than working from the common few: `requires` grants
  capabilities like `execute_binary`, so a field you did not know about is a
  field you did not know you were granting.
- **`main.bundle.js`** — the build output, referenced by `entry`.

Build with esbuild as an IIFE that exposes a global:

```
esbuild src/main.ts --bundle --format=iife --global-name=<Name>Plugin \
  --outfile=dist/main.bundle.js
```

`global_name` in the manifest must match `--global-name`. Import host helpers
from the `moss-api` package (published as `@symbiosis-lab/moss-api` on npm).

## Capabilities (the hooks)

`capabilities` is a list; each maps to a JS hook function of the same name. For
the hooks this moss has, when each runs, and whether it accepts one plugin or
several:

```
moss describe --json | jq '.plugin_hooks'
```

## Derivations: a file type that becomes text

A plugin that reads a kind of file (documents, slide decks) and returns markdown needs no script: it declares `contributes.derivations[<id>]` with the extensions it `accepts`, the named `fragments` it may return, a `noun` for counting files, a `stub` layout for the page written beside each file, and `run`, a pinned one-shot binary (`download` sources need a `sha256`; a `path` source is a binary you already have). Omit `entry` when the plugin declares nothing else. moss runs the binary once per file in a sandbox, with `{"path": "<absolute path>"}` on stdin, and caches its answer by the file's content, so keep the output deterministic. It prints one JSON object: `{"kind":"derived","fragments":{"text":"…"},"facts":{"title":"…","description":"…"}}`, or `{"kind":"skip","reason":"…","message":"…"}` to decline a file (a result, cached; the message is shown to readers and in `moss import --status`). Any other outcome is a failure and is retried next build. The authoring side is `![[file.ext#text]]` in a page; the full field list and protocol are in the manifest reference (`moss describe --json | jq '.manifest_fields'`, and the plugin manifest page on the docs site).

## A listing style

A plugin can draw how a folder lists its pages. It declares `contributes.listing_styles.<id>` (`label`, `description`, optional `content_width` of `wide` or `full`) and exports `listing(ctx)`; an author then writes `children_style: <id>` on the folder's home page, and the editor offers the id in the same select as `list`, `summary` and `grid`. An id belongs to one plugin, and the ids moss draws itself are taken. Without the plugin (or under `--no-plugins`) the folder is listed in moss's default style with one build warning.

`listing` runs after render, once per folder home page that names one of the plugin's styles. `ctx` is `{ model: 1, style, page, pages, folders, terms }`. `page`, every entry of `pages` and every entry of `folders` are the same record: `{ title, url_path, url, lang, translations: [{lang, url_path}], frontmatter, html, folder }`. `pages` are exactly the pages moss's own listing of that folder shows, `url` is relative to the listing page, `html` is the rendered body with its relative links and image sources rewritten to resolve from the listing page, and `terms[namespace][name]` is where the page for each name those pages use is, written like `url`, so it resolves from the listing page. `pages` and `folders` come in the order moss's own listing shows them. Return `{ html, head, files, warnings }`: `html` replaces the listing, `head` is `[{stylesheet: href} | {script: href}]` for the page's `<head>` (a script is emitted with `defer`, so it runs after the listing is in the page), `files` are `{path, text}` or `{path, base64}` written beside the page (a path that is absolute, contains `..`, starts with `_moss/` or collides with a page is refused with a warning), and each `warnings` entry, a string or `{text, page}` with `page` a record's `url_path` (the warning then names that page's source file), becomes a build warning, so `--strict` fails on it. A throw, a timeout or a malformed result also leaves the default listing with one warning.

A plugin that only contributes listing styles declares no `capabilities` (a `"listing"` entry there fails to parse, since `listing` is dispatched by style, not declared). A plugin that carries its own renderer script or stylesheet reads those bundled files with the host call `read_plugin_file` and returns them in `files`: in plain JavaScript, `await __TAURI__.core.invoke("read_plugin_file", { relativePath: "assets/board.js" })` resolves to the file's text, the path is relative to the plugin's own folder, and only text files can be read this way.

```json
{ "name": "boardview", "version": "1.0.0", "entry": "main.js",
  "contributes": { "listing_styles": { "board": { "label": "Board", "description": "One card per page" } } } }
```

```js
var BoardviewPlugin = {
  async listing(ctx) {
    const cards = ctx.pages.map(p => `<li><a href="${p.url}">${p.title}</a></li>`).join("");
    return { html: `<ul class="board">${cards}</ul>`, head: [{ stylesheet: "board.css" }],
             files: [{ path: "board.css", text: ".board { display: grid; gap: 1rem; }" }] };
  }
};
```

## Setup: declare, don't build UI

A contribution that needs anything from the user declares a `setup` block — moss draws all of it; a plugin never builds settings or first-publish UI. Three members, ordered by what they cost to evaluate:

- **`needs`** — preconditions the host checks with no plugin code running (e.g. `"stack"`). A failed need short-circuits: moss owns the remedy. A plugin naming `"stack"` in `needs` also declares `contributes.stack` — the id, version and download/path sources for the companion process that need refers to.
- **`settings`** — a list of fields (`key`, `type` of `string`/`number`/`boolean`/`secret`, plus `label`, `options`, `when`, `pattern`, `required`, `hidden`…). moss renders them on the settings page and, when a required one is empty, at the publish gate. A `secret` goes to the OS keystore, read back with `moss.getSecret(key)` — never config.
- **`check: true`** — the plugin implements the `check_setup(ctx)` hook for what only it can know (daemon running, token valid). It answers `{status: "ready"}` or `{status: "blocked", blockers}`; each blocker may carry a form of the same field vocabulary, and a submitted blocker's id comes back as `ctx.action`. Return `field_errors` to re-ask.

The old spellings — `config_schema` and the other `config_*` maps, `setup.credentials`, `requires_stack` — still parse, as aliases folded into this shape. Write the new one.

## The data contract (verified paths)

- Plugins that fetch external data write JSON to **`.moss/data/social/<plugin>.json`**.
  Enhance hooks and other plugins read it — and so does core, for the built-in
  comment and review features, which load every `.moss/data/social/*.json`
  during the build. Do not assume a file there is inert.
- The source→output map is at **`.moss/build.nosync/article-map.json`** (paths + uids),
  for correlating plugin data with generated pages.
- Core emits standard HTML5 landmarks (`<article>`, `<main>`, `<h1>`) and
  `data-*` attributes; enhance hooks target these.

> Note: older moss documentation says `.moss/social/` and
> `.moss/article-map.json`. Those paths are stale — use `.moss/data/social/`
> and `.moss/build.nosync/article-map.json` above.

## Distribution

**How a user actually gets a plugin today: it ships bundled in the moss
binary**, and they install it from the plugin installer or the first-publish
flow. There is no remote-install path yet, so a plugin you write independently
has no route to a user's site — it has to be contributed upstream and bundled
into a moss release, not published for them to fetch.

First-party source is mirrored to `Symbiosis-Lab/moss-registry`, and the
packages are also published to npm as `@symbiosis-lab/moss-plugin-*`, but
neither is the install mechanism. There is no scaffolding command — copy an
existing plugin and adapt.
