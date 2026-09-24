# The plugin manifest

Every plugin ships an `assets/manifest.json`. It is how moss knows who your
plugin is, what the user gets by installing it, and what the user is trusting it
with.

This is the reference for the manifest as moss reads it **today**. The
`capabilities` field is being replaced by a `contributes`-based vocabulary — see
[Coming changes](#coming-changes) before designing around it.

## A minimal manifest

```json
{
  "name": "my-plugin",
  "version": "0.1.0",
  "description": "What the user gets",
  "author": "Your Name",
  "entry": "main.bundle.js",
  "capabilities": ["deploy"],
  "global_name": "MyPlugin",
  "icon": "icon.svg",
  "preview": true
}
```

`name` must match the plugin's directory name. `entry` is the bundle filename
inside `dist/` — always `main.bundle.js` in practice. `global_name` is the IIFE
global your bundler produces; moss looks the plugin object up under that name.

New plugins should ship `"preview": true`. A preview plugin is offered only to
users who have turned on preview features, which is where you want to be until
the plugin is polished. Release zips are immutable, so adding the field after
your first publish costs a version bump.

## Identity and presentation

| Field | Type | Notes |
|---|---|---|
| `name` | string | **Required.** Identifier; matches the directory name |
| `version` | string | **Required.** Semver |
| `entry` | string | **Required.** Bundle filename |
| `description` | string | Shown in the catalog |
| `author` | string | Shown in the catalog |
| `display_name` | string | Title for the plugin's settings section |
| `icon` | string | Filename in `assets/`; falls back to `icon.svg`, `icon.png`, `logo.svg`, `logo.png` |
| `global_name` | string | IIFE global name; defaults to PascalCase name + `Plugin` |
| `repository`, `homepage` | string | Display and provenance only |
| `min_moss_version` | string | Semver floor. The registry client checks it at install and load; a moss older than the check ignores the field |
| `preview` | boolean | Hides the plugin from the catalog unless the user enables preview features. Only read for plugins bundled into the moss binary today; the catalog that would read it for a downloaded plugin does not exist yet |

## What the plugin does

`capabilities` lists what your plugin does. Three of the five map to an exported
function of the same name; two do not, which is easy to get wrong.

| Capability | Hook | What the user sees |
|---|---|---|
| `deploy` | `deploy(ctx)` | a row in the deploy-target dropdown |
| `syndicate` | `syndicate(ctx)` | a channel that publishes after a deploy |
| `process` | `process(ctx)` | nothing directly — runs at scan time |
| `login` | none | a connection row in the plugin's settings. moss calls your `login` export out of band via `connect_account`, not as a build hook |
| `import` | none | a label on the task moss shows while pulling your posts in. There is no `import` export — the word is reserved in JavaScript — so declaring it alone runs nothing |

Deploy plugins may also export `configure_domain(ctx)` to handle custom-domain
setup (DNS records, verification). It is an optional hook, not a capability.

Two capability names still parsed by moss — `generate` and `enhance` — are being
removed. Nothing has ever shipped using them; do not write new plugins against
them.

### `contributes`

Declarative additions moss acts on without running your code:

```json
"contributes": {
  "frontmatter": {
    "fields": {
      "matters_id": { "type": "string", "description": "Matters article id" }
    }
  },
  "jobs": { "syndicate": { "verb": "Syndicated", "noun": "posts" } }
}
```

`frontmatter` adds schema fields the editor validates and completes — note the
`fields` wrapper; a bare list is a type error that fails the whole manifest, not
just the contribution. `jobs` supplies the words moss uses when it reports your
hook's progress — moss owns the pixels, you supply the verb and noun.

There is a third key, `embed_renderers`, for renderers of embed syntax. moss
parses it and **nothing consumes it yet** — the adapter exists but no build path
constructs it, so a plugin declaring one gets nothing. Do not build on it until
this note says otherwise.

## Configuration

Settings are declared as a `setup.settings` list on the contribution they belong to — see [Setup](#setup-what-the-user-arranges-first) below. Each field is one object:

| Key | Notes |
|---|---|
| `key` | **Required.** The config key, and for a `secret` the keystore name `moss.getSecret(key)` reads |
| `type` | **Required.** `"string"`, `"number"`, `"boolean"`, or `"secret"` |
| `label` | Display label; defaults to the key, title-cased |
| `description` | Help text under the field |
| `default` | Applied when the user has set nothing. A `secret` must not declare one |
| `options` | `{value, label}` list; makes a `string` field a dropdown |
| `required` | The publish gate asks for it when empty |
| `hidden` | A real config key moss never draws — plugin bookkeeping, or a secret a login flow deposits |
| `placeholder` | Input placeholder |
| `help_url` | "Where do I get this?" link |
| `pattern`, `pattern_message` | Regex the value must match, and what to say when it doesn't |
| `when` | `{key: value}` conditions on earlier fields; all must match for the field to apply |

A `secret` value goes to the OS keystore and comes back through `moss.getSecret(key)` — it never appears in `ctx.config` or on disk.

The pre-contract spellings still parse, as aliases folded into `settings`: `config_schema` (its `"enum"` becomes a `string` with `options`; `"array"` is no longer drawn), `config_options`, `config_labels` (a dotted `"<field>.<value>"` key labels one option), `config_descriptions`, `config_placeholders`, and `setup.credentials` (each entry becomes a `secret` field). `config_verify` is gone — no plugin ever used it. Write `settings`.

At runtime, values merge in this order: `config.json` > `config.toml` > manifest defaults (see [runtime-environment.md](runtime-environment.md)). `config` in the manifest still supplies plugin-wide defaults; a field's `default` is the per-setting way to say the same thing.

**Config is for the user's settings, not your plugin's bookkeeping.** State your
plugin maintains for itself (sequence numbers, draft ids, caches) belongs in
plugin-private storage — `readPluginFile` / `writePluginFile` — where the user
won't edit it by hand.

## What the user is trusting

Keep this section as short as your plugin allows. moss gates only what protects
the user's identity, another plugin's isolation, or the machine; everything else
is yours to do without asking.

| Field | Type | Notes |
|---|---|---|
| `requires` | string[] | Host grants, named per binary: `"execute_binary:git"` runs exactly `git`. Absent or unlisted = refused, fail-closed. The bare `"execute_binary"` blanket is deprecated — it still grants everything, with a warning per run |
| `domain` | string | The domain whose cookies you may read and write |
| `domains` | string[] | Every domain you operate on (e.g. production + staging), so a force-fresh login clears all of them |
| `requires_stack` | boolean | Legacy alias: parses as `"needs": ["stack"]` on every contribution's setup block; a plugin that declares `contributes.stack` also carries its own artifact pin |

Using the keystore is **not** a gated capability: you only ever sign with your
own scoped key, so it costs nobody else anything.

## Two things worth knowing before you write a hook

**A deploy hook must work without a UI.** Deploys will eventually run from the
CLI and from headless hosts with no action panel. Open a panel for first-run
setup if you like, but never let a successful publish depend on one existing.

**Not every Tauri command is reachable.** Plugins run in QuickJS behind a fixed
list of host functions. If you reach past the SDK with a raw `invoke()` for a
command that isn't on that list, it fails on every version of moss — bumping
`min_moss_version` will not help. Use the SDK; if something you need is missing,
open an issue.

## Coming changes

moss is replacing `capabilities` with declarations of what the user gains.

**Do not migrate yet.** The two replacements below are implemented but not
released. A moss that predates them ignores the new keys — your plugin loads,
its capability list comes out empty, and it is silently absent from the deploy
menu with no error anywhere. Keep writing `capabilities` until this note names
the release that reads contributions; `min_moss_version` will not protect you,
because nothing compares it.

What is coming:

```json
"contributes": {
  "deploy_target": { "display_name": "IPFS" }
}
```

```json
"contributes": {
  "channel": {
    "display_name": "Matters",
    "imports": true,
    "login": true
  }
}
```

`contributes.deploy_target` replaces the `deploy` capability. `contributes.channel` replaces `syndicate`, `login` and `import`: syndication is what a channel *is*, so it needs no flag; `imports` says existing posts can be pulled back into the folder; and `login` says the user connects an account, which is you exporting `login` for moss to invoke. `login` was called `requires_login` before 2026-08-30 and both spellings still read.

```json
"contributes": {
  "stack": {
    "id": "onionpress",
    "version": "v2.4.110-moss.2",
    "sources": [
      { "kind": "download", "platform": "darwin-arm64", "url": "https://github.com/…/onionpress.dmg", "sha256": "68e4…", "archive_format": "dmg", "executable": "OnionPress.app/Contents/MacOS/onionpress" },
      { "kind": "path", "platform": "linux-x64", "binary": "onionpress" }
    ],
    "start": ["start"],
    "stop": ["quit"]
  }
}
```

`contributes.stack` replaces `requires_stack`: the plugin now carries the pin for the machine-wide companion process it needs, instead of moss compiling one in. A `download` source is refused if it has no `sha256` — not warned, not defaulted — because that hash is the only thing standing between the user and an unverified binary; a `path` source is exempt, since it fetches nothing to hash.

### Setup: what the user arranges first

Any contribution — a channel as much as a deploy target — can carry a `setup` block saying what the user must arrange before it works. It is the same block wherever it sits, and it is scoped by where it sits: there is no field naming what it applies to. Three members, ordered by what they cost moss to evaluate:

- `needs` — preconditions the host checks with no plugin code running. `"stack"` means moss's machine-wide companion install; a failed need becomes a blocker whose remedy moss owns. A plugin naming `"stack"` here also declares `contributes.stack`, above.
- `settings` — the fields moss draws, from the [Configuration](#configuration) vocabulary. Order is display order, and a `when` may only reference a field declared earlier in the list.
- `check` — `true` says you implement the `check_setup(ctx)` hook, for the answers only your code can give: is the daemon running, is the token still valid. It returns `{status: "ready"}` or `{status: "blocked", blockers}`. Each blocker has an `id` and a `message`, and may carry a `form` — the same field vocabulary again — plus a `submit` label; when the user submits it, your hook runs again with the blocker's id as `ctx.action` and the values in `ctx.values`. Return `field_errors` (`{key: message}`) to re-ask with the messages under the fields. A rejected stored token is not yours to re-collect: return a blocker whose id is `moss:credentials` and moss opens its own credential modal.

Note that a `setup` block is not a login: a channel authenticated by one API token declares a `secret` setting and leaves `login` alone, and moss will not offer to connect an account that does not exist.

Still to come: `process` disappears from the manifest entirely — which hooks
your plugin exports will be read from your code at install time rather than
declared. `capabilities` keeps working for at least a release after that, so
there is no version where you must have migrated.

The rationale: a manifest should say what
the user is choosing, and anything that merely restates what the code already
says will drift from it.
