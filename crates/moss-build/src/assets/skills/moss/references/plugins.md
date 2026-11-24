# Building a moss plugin

Read `moss guide` (the overview) first: it holds the task recipes this reference does not repeat.

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

The bundle runs in QuickJS, not a browser or Node. Its web globals are `console`, `atob`/`btoa`, `TextEncoder`/`TextDecoder`, timers, `URL`, a buffered `fetch`, `crypto.randomUUID` and part of `crypto.subtle`: `digest`, HMAC (`importKey("raw")`, `sign`) and ECDSA P-256 with SHA-256 (`generateKey`, `exportKey` as a public `jwk` or a `pkcs8` private key, `importKey("pkcs8")`, `sign`), enough for a DPoP sign-in. There is no `crypto.getRandomValues`, no `subtle.verify`, no DOM and no Web Streams; a call outside this list throws, so a plugin whose tests pass under Node can still fail in moss.

## Capabilities (the hooks)

`capabilities` is a list; each maps to a JS hook function of the same name. For
the hooks this moss has, when each runs, and whether it accepts one plugin or
several:

```
moss describe --json | jq '.plugin_hooks'
```

A `process` hook's context carries `site_config`: the site's `.moss/config.toml` as a plain JSON object, read-only (`{}` when there is none). A plugin reads its own documented tables from it, such as `[map.layers.<name>]`. Withheld: the `[plugins]` table (a plugin's own settings arrive as `config`) and any key, at any depth, named like a credential (`token`, `secret`, `password`, `api_key`, `credential`, `private_key`). `.moss/` paths stay closed to `readFile`/`writeFile`, so this is the only way to read the config.

To save binary files into the site folder, `writeFileBinary(path, bytes)` writes exact bytes under the same rules as `writeFile` (inside the project, never `.moss/`). `downloadAsset(url, dir, { filename, headers })` saves a download under a name you choose; without `filename` it is named from the URL's last segment, so URLs that all end in `default.jpg` overwrite each other. Every plugin HTTP request carries moss's `User-Agent` unless the plugin sets its own header.

## Derivations: a file type that becomes text

A plugin that reads a kind of file (documents, slide decks) and returns markdown needs no script: it declares `contributes.derivations[<id>]` with the extensions it `accepts`, the named `fragments` it may return, a `noun` for counting files, a `stub` layout for the page written beside each file, and `run`, a pinned one-shot binary (`download` sources need a `sha256`; a `path` source is a binary you already have). Give each `download` a `platform` (`darwin-arm64`, `linux-x64`, …) when the binary is built for one kind of computer; a source with none fits every computer, and a plugin with no source for the computer it is on is shown as not available there and refused at install. Omit `entry` when the plugin declares nothing else. moss runs the binary once per file in a sandbox, with `{"path": "<absolute path>"}` on stdin, and caches its answer by the file's content, so keep the output deterministic. It prints one JSON object: `{"kind":"derived","fragments":{"text":"…"},"facts":{"title":"…","description":"…"}}`, or `{"kind":"skip","reason":"…","message":"…"}` to decline a file (a result, cached; the message is shown to readers and in `moss import --status`). Any other outcome is a failure and is retried next build. The authoring side is `![[file.ext#text]]` in a page; the full field list and protocol are in the manifest reference (`moss describe --json | jq '.manifest_fields'`, and the plugin manifest page on the docs site).

## A listing style

A plugin can draw how a folder lists its pages. It declares `contributes.listing_styles.<id>` (`label`, `description`, optional `content_width` of `wide` or `full`) and exports `listing(ctx)`; an author then writes `children_style: <id>` on the folder's home page, and the editor offers the id in the same select as `list`, `summary`, `grid` and `stream`. An id belongs to one plugin, and the ids moss draws itself are taken. Without the plugin (or under `--no-plugins`) the folder is listed in moss's default style with one build warning.

`listing` runs after render, once per folder home page that names one of the plugin's styles. `ctx` is `{ model: 1, style, page, pages, folders, terms }`. `page`, every entry of `pages` and every entry of `folders` are the same record: `{ title, url_path, url, lang, translations: [{lang, url_path}], frontmatter, html, folder }`. `pages` are exactly the pages moss's own listing of that folder shows, `url` is relative to the listing page, `html` is the rendered body with its relative links and image sources rewritten to resolve from the listing page, and `terms[namespace][name]` is where the page for each name those pages use is, written like `url`, so it resolves from the listing page; on a page in a language tree such as `de/`, that is the term page of the same tree (`de/authors/ada-example/`), not the default edition's. `pages` and `folders` come in the order moss's own listing shows them. Return `{ html, head, files, warnings }`: `html` replaces the listing, `head` is `[{stylesheet: href} | {script: href}]` for the page's `<head>` (a script is emitted with `defer`, so it runs after the listing is in the page), `files` are `{path, text}` or `{path, base64}` written beside the page (a path that is absolute, contains `..`, starts with `_moss/` or collides with a page is refused with a warning), and each `warnings` entry, a string or `{text, page}` with `page` a record's `url_path` (the warning then names that page's source file), becomes a build warning, so `--strict` fails on it. A throw, a timeout or a malformed result also leaves the default listing with one warning.

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


## A block: a custom element an author places

A plugin can give authors a custom element to place on a page, without running any plugin code at build. It declares the element under `contributes.blocks.<short>`, with a script and stylesheet from its own folder and the attributes the element accepts:

```json
{ "name": "catalog", "version": "1.0.0",
  "contributes": { "blocks": { "card": {
    "element": "catalog-card", "script": "card.js", "stylesheet": "card.css",
    "attributes": [{ "key": "size", "type": "number" }] } } } }
```

An author writes a `:::card` fence around ordinary markdown, and moss emits `<catalog-card>` with the body as its children and loads the script and stylesheet once, only on pages that use the block. Write the body so it works alone: it is what a reader gets with the plugin missing, JavaScript off or a failed start, and another tool shows it too.

- Each of `attributes` has a `key` and may say `type` (`string`, the default, `number` or `boolean`), `required`, `options` (the only values a string takes) and `pattern`, a regular expression the value must contain a match of (anchor it with `^…$` for the whole value), with an optional `pattern_message` for the warning. An attribute the block does not declare, or a value its `type`, `options` or `pattern` refuses, is dropped with a warning at the fence's line, and a missing `required` one is reported. A fence's `.class`, `#id` and width are ignored the same way.
- moss emits only what the fence wrote; an attribute has no default in the manifest. The element reads its attribute first, then any data the site publishes for it, then its own default.
- `script` (`.js` or `.mjs`) and `stylesheet` (`.css`) are paths inside the plugin's folder, served at `/_moss/plugins/<plugin>/<path>`. A plugin that only declares blocks needs no `entry`.
- A block's full name is `<plugin>--<short>`. The short name works while one installed plugin declares it; when two do, neither places it, the body shows, and a warning names the full names to write. Names moss uses for its own shortcodes cannot be declared.
- The element must be a valid custom element name, an attribute may not be an event handler (`on…`), and a `pattern` must compile. A manifest that breaks one of these does not load.

An element that needs the site's own settings names them under `contributes.public_data`: `{ "keys": ["currency", "cancel.late_percent", "services.*.name"] }`, paths below the plugin's own table `[services.<plugin>]` in `.moss/config.toml`, where `*` stands for any one key, such as the ids of a map. moss publishes `/_moss/extensions/<plugin>.json` holding `{ "schema": 1, "data": { … } }`, built from those paths only: each ends at a value that is not a table (a string, number, boolean, date, or a list of those), and anything else in the table, at any depth, is left out, so a private address or a token added there later cannot publish. A left-out key the plugin's own `site_settings` draws is private by design; any other is named in a warning. A key that `*` admits is left out too when its name looks like a secret, and a path cannot put a named key beside `*` at the same step. A path cannot start with `*` or contain a name that looks like a secret (`token`, `secret`, `password`, `api_key` and the like). A site that has no such table publishes nothing. An installed plugin named `booking` that declares `public_data` owns `[services.booking]`: moss's own booking reader is off, so the plugin's keys draw no warning and moss writes no `booking` object into `/_moss/commerce.json`. A plugin that only declares blocks or public data needs no `entry`.

A key the site leaves out can be filled by the build with the address of one of moss's own services for this site: `"fill": { "endpoint": { "service": "commerce", "path": "/sites/{site}/booking" } }` beside `keys`. The address follows the folder's environment and is never written into the folder; a value written in the table wins. Where the service does not run, or the folder has never been published, nothing is filled.

### Settings kept in the site's own table

A plugin whose settings belong to the site, and should travel with it and be readable in `.moss/config.toml`, declares them under `contributes.site_settings` instead of `setup.settings`. moss draws them in the plugin's tab and writes each change into `[services.<plugin>]`, one key at a time, keeping the file's comments and order, so a person, an agent or a text editor can change the same keys by hand.

```json
"site_settings": {
  "settings": [
    { "key": "currency", "type": "string", "label": "Currency" },
    { "key": "items", "type": "map", "label": "Products", "add_label": "Add product",
      "items": { "type": "object", "fields": [
        { "key": "name", "type": "string", "label": "Name" },
        { "key": "sold_as", "type": "string", "label": "Sold", "default": "unit",
          "options": [{ "value": "unit", "label": "Each" }, { "value": "weight", "label": "By weight" }] },
        { "key": "price", "type": "number", "label": "Price", "scale": 100, "when": { "sold_as": "unit" } },
        { "key": "per_kilo", "type": "number", "label": "Price", "unit": "per kg", "scale": 100, "when": { "sold_as": "weight" } }
      ] } },
    { "key": "open", "type": "object", "label": "Opening hours", "fields": [
      { "key": "mon", "type": "array", "label": "Monday", "items": { "type": "string" } } ] }
  ],
  "actions": [{ "id": "connect", "label": "Connect payments", "service": "commerce", "method": "POST", "path": "/sites/{site}/stripe/onboarding" }]
}
```

- The field vocabulary is `setup.settings`' plus `object` (named `fields`, a table; `optional: true` draws a switch that adds or removes it), `map` (entries under ids the person names, fixed once made, checked by `key_pattern`; its `placeholder` shows an example id in the box where a new one is typed; `items` is an `object` or a scalar) and `array` (scalar values, written comma-separated). A `number` takes `scale` (a power of ten: `100` shows `12000` as `120.00`) and `unit` (the word after the input).
- `when` looks back within the same table. A select with no stored value is read from the fields it reveals; changing it removes the keys of the fields it hides and writes the defaults of the ones it shows, in one save.
- `secret` and `hidden` are refused: the table syncs with the folder, and a key the tab does not draw is simply left out of the manifest and kept as written.
- An action calls a route on one of moss's own services, signed by the folder, and opens the `https` address in the answer. moss makes the call only for plugins it ships or its registry lists.
- `translations` at the manifest's top level, keyed by locale and then by the English text (`{ "zh": { "Currency": "币种" } }`), show the manifest's words in the reader's language; a missing entry shows the English.

### Proving the site is the person's

A network that lets a person's domain stand for their account (a Bluesky handle, a verified Mastodon link) checks a proof on the site. A plugin declares that proof under `contributes.identity`, and moss writes it on every build from the plugin's own `[services.<plugin>]` table, so the person writes one line, never the file:

```json
"site_settings": { "settings": [
  { "key": "did", "type": "string", "label": "Account ID" },
  { "key": "account", "type": "acct", "label": "Account" }
] },
"identity": [
  { "file": ".well-known/atproto-did", "body": "{did}" },
  { "link": "me", "href": "https://{account.host}/@{account.user}" },
  { "meta": "fediverse:creator", "content": "@{account.user}@{account.host}" }
]
```

- A proof is a `file` under `.well-known/`, a `link` or a `meta` written into the head of every page. A file is `atproto-did` or `nostr.json`, a link's rel is `me`, and a meta's name is `fediverse:creator`. Any other is refused and the manifest does not load: another file could grant an app authority over the domain (`webauthn`, `assetlinks.json`), and another rel or name could change every page (`canonical`, `robots`). A network that needs one more is added to moss by name.
- `{key}` reads a setting the plugin declares in `site_settings`. An `acct` setting holds `@name@host`, refused in any other shape, and `{key.user}` and `{key.host}` read its two halves. Any other brace is text, so a `.json` body is written as JSON: each value goes in a string, escaped, and a body that is not JSON once filled does not load.
- An `acct` setting marked `"from": "account"` is not drawn in the settings form: while `[services.<plugin>]` holds no value for it, a proof reads the account the folder is signed in to (its name must be `@name@host` on the account's own server), and signing out removes the proof; a value written in the table wins.
- A proof whose setting is unset is not written, without a warning: the person has not set that network up. A hand-written value that cannot be written (an `acct` without a host, a control character) is left out with a warning naming the setting.
- Proofs are data, read even by a build that runs no plugin code (`moss build --no-plugins`), so a CI deploy keeps them. A path two suppliers claim is written once, from `[site]` first and then plugins by name, with a warning naming both. When a proof the last build served is gone, the build says so (`identity: .well-known/atproto-did removed`), since that was someone's live handle.

## A newsletter service

A mailing service a site's subscribers can be copied to declares `contributes.newsletter_service` and needs no `entry`. The person picks it under Subscribers in the app, which writes `provider = "<plugin name>"` under `[channels.email]` in `.moss/config.toml`, then makes a key in the service and pastes it. moss draws the steps and checks the pasted key's prefix and length before anything is sent; the service's side runs on moss's server, which keeps a narrower key it makes from the pasted one.

```json
"newsletter_service": {
  "key": { "url": "https://app.sendgrid.com/settings/api_keys", "prefix": "SG.", "length": 69, "permission": "Full Access" },
  "steps": ["Open API Keys in SendGrid", "Create API Key, name it {name}, keep {permission}, then Create & View", "Copy the key and paste it here"],
  "webhooks_url": "https://app.sendgrid.com/settings/mail_settings/webhook_settings",
  "senders_url": "https://app.sendgrid.com/settings/sender_auth"
}
```

The steps are written in the service's own words, since the person reads them beside the service's window; the first is the link to `key.url`. `{name}` stands for `moss`, the name the server looks for before it deletes the pasted key, and `{permission}` for `key.permission`, the service's word for the access the key needs, which stays in the service's words in every translation. The service is shown by the manifest's `display_name`. Every address is `https`. The steps are translated through the manifest's `translations` table, keyed by the English text.

## Setup: declare, don't build UI

A contribution that needs anything from the user declares a `setup` block — moss draws all of it; a plugin never builds settings or first-publish UI. Three members, ordered by what they cost to evaluate:

- **`needs`** — preconditions the host checks with no plugin code running (e.g. `"stack"`). A failed need short-circuits: moss owns the remedy. A plugin naming `"stack"` in `needs` also declares `contributes.stack` — the id, version and download/path sources for the companion process that need refers to.
- **`settings`** — a list of fields (`key`, `type` of `string`/`number`/`boolean`/`secret`, plus `label`, `options`, `when`, `pattern`, `required`, `hidden`…). moss renders them on the settings page and, when a required one is empty, at the publish gate. A `secret` goes to moss's secret store on this computer, outside every folder, read back with `moss.getSecret(key)` — never config. A key passed to `setSecret`/`getSecret` may be any text up to 128 bytes except one containing `/`, `\`, NUL or `..`, or a Windows device name (`con`, `com1`); moss makes the file name safe on every system, and each byte other than an ASCII letter, digit, `.`, `_` or `-` counts three toward a limit of 200.
- **`check: true`** — the plugin implements the `check_setup(ctx)` hook for what only it can know (daemon running, token valid). It answers `{status: "ready"}` or `{status: "blocked", blockers}`; each blocker may carry a form of the same field vocabulary, and a submitted blocker's id comes back as `ctx.action`. Return `field_errors` to re-ask.

The old spellings — `config_schema` and the other `config_*` maps, `setup.credentials`, `requires_stack` — still parse, as aliases folded into this shape. Write the new one.

## Signing in to a channel

A channel that posts to an account signs in with one button: its manifest sets `contributes.channel.login` to `true` and the plugin exports `login(ctx)`, or sets it to the provider's sign-in address for moss to open. Never ask for a handle, a server or a token in `settings`; the sign-in finds them out.

`login` answers a `LoginResult`: `{ status: "completed", account: { id, name } }`, `{ status: "cancelled" }` or `{ status: "failed", message }`. `id` is the provider's stable identifier for the account (a DID, a numeric user id written as a string, `id@server`), never the handle: moss records the folder as signed in to it, under `[accounts.<plugin>]` in `.moss/state.toml`, and runs a channel's first sync when that binds a folder that had no account. `name` is only shown. A plugin never writes its own binding into its config (`boundUserName`, `boundUserId` are read only for plugins from before this contract), and a plugin that returns a `LoginResult` sets `min_moss_version` to `0.15.29`.

An OAuth sign-in runs in the person's own browser with `startBrowserSignIn()`: register its `redirectUri`, then `open(authorizeUrl)` resolves with the redirect's query. It rejects with `SignInCancelled` when the person closes the sign-in or another one starts; answer `{ status: "cancelled" }` for that, and let other errors fail. Keep what the sign-in earned with `moss.setSecret(key, value)` under a key the manifest declares as a hidden `secret` setting, and answer `copy_account` from it.

## The data contract (verified paths)

- Plugins that fetch external data write JSON to **`.moss/data/social/<plugin>.json`**.
  Enhance hooks and other plugins read it — and so does core, for the built-in
  comment and review features, which load every `.moss/data/social/*.json`
  during the build. Do not assume a file there is inert.
- The source→output map (paths + uids), for correlating plugin data with
  generated pages, is `article-map.json` in the build output's root, the
  folder above the `output` path `moss build --json` prints.
- Core emits standard HTML5 landmarks (`<article>`, `<main>`, `<h1>`) and
  `data-*` attributes; enhance hooks target these.

> Note: older moss documentation says `.moss/social/`,
> `.moss/article-map.json` or `.moss/build.nosync/article-map.json`. Those
> paths are stale; use the ones above.

## Distribution

**How a user gets a plugin: `moss plugin install <id> [<folder>]`.** It puts the plugin into the site, either from moss's plugin registry or, for a plugin moss ships, by copying it out of moss. In the app, the plugin installer and the first-publish flow install plugins too. The registry is moss's own and its catalog is fetched on every install, so a plugin you write reaches a user's site by being added to that registry, not by publishing it somewhere for users to fetch.

A plugin whose manifest says `"preview": true` is unfinished: moss's plugin catalog hides it and `moss plugin install` refuses it until the user turns on **Preview features** in moss's Settings (without the app, by adding `"experimental": {"preview_features": true}` to `app-config.json` in moss's app-data folder, which the refusal names). A copy already in a site keeps running either way.

**Trying a plugin before it is in the registry.** Copy the built plugin (its `manifest.json`, the file its `entry` names, its icon) into `<site>/.moss/plugins/<name>/`, where `<name>` is the manifest's `name`. moss runs a folder's plugins only once the folder is trusted on that computer: the app asks "Trust this folder?" the first time it opens a folder with plugins, and from the command line `--allow-plugins` on `moss build` or `moss deploy` is the consent. `moss plugin install` records no trust and says so. The answer is kept outside the folder, so every computer that opens it asks once; after that, a plugin added or changed in a trusted folder runs without asking. A plugin whose `min_moss_version` is above the running moss, or one the registry withdrew, is refused whatever the answer. The settings it declares under `setup.settings` are read from `.moss/plugins/<name>/config.toml`, one `key = value` line each; those under `contributes.site_settings` live in `.moss/config.toml`, as above. `moss deploy <folder> --dry-run --allow-plugins` shows what each channel would be sent without sending it.

The packages are also published to npm as `@symbiosis-lab/moss-plugin-*`, but moss does not install from npm. There is no scaffolding command: copy an existing plugin and adapt it.
