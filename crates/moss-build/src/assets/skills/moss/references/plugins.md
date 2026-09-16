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

One thing the JSON does not tell you: `import` receives the **same
`ProcessContext` as `process`**, deliberately, rather than a context of its
own. If you write an `import` hook expecting a bespoke context shape, you will
be looking for fields that were never going to be there.

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
- The source→output map is at **`.moss/build/article-map.json`** (paths + uids),
  for correlating plugin data with generated pages.
- Core emits standard HTML5 landmarks (`<article>`, `<main>`, `<h1>`) and
  `data-*` attributes; enhance hooks target these.

> Note: older moss documentation says `.moss/social/` and
> `.moss/article-map.json`. Those paths are stale — use `.moss/data/social/`
> and `.moss/build/article-map.json` above.

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
