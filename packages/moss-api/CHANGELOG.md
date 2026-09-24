# Changelog

All notable changes to this package are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Version headings are written by changesets, which prepends each new one directly
under this preamble — so the preamble stays at the top and `[Unreleased]` sits
immediately below it. Before 2026-08-13 the `## 0.10.0` heading sat ABOVE the
preamble, which would have buried these unreleased notes under every future
release heading.

## [Unreleased]

### Added

- `contributes.stack`, replacing `requires_stack`: a plugin can now carry the pin (version, download url, sha256) for the machine-wide companion process it needs, instead of moss compiling that pin into the app. A `download` source with no `sha256` is refused, not defaulted; a `path` source (a binary the user already installed, resolved on PATH) needs no hash since nothing is fetched. `requires_stack: true` still parses, folded into `needs: ["stack"]` on every capability's setup block, so an existing manifest keeps working. See `docs/plugin-manifest.md`.

### Changed

- A `DeployAddress` with both `url` and `value` now renders both — a link and a copy button — instead of moss silently dropping `value` in favor of the link. An IPNS name can be both a bare copyable string and an openable gateway door; give it both fields and both show.
- `AdvisoryProposal.item`'s doc now says what it has always meant: the site-relative path of the file the advisory is about (e.g. `posts/2026/hello.md`), omitted for a build-wide notice. moss also now enforces it — an `item` that is absolute or escapes the site root via `..` is dropped, so the advisory renders build-wide rather than pointing the reader's click at the wrong file.

## [0.13.0] - 2026-08-30

### Added

- An optional `check_setup` hook, for readiness a manifest cannot declare — a daemon that must be running, a name that must be claimed. It answers `{ status: "ready" }` or `{ status: "blocked", blockers }`; each `SetupBlocker` has an `id` and `message` and may carry a `form` (fields in the same `SetupField` vocabulary as `setup.settings`, plus a `submit` label). When the user submits one, the hook runs again with the blocker's `id` as `ctx.action` and the values in `ctx.values`; return `field_errors` (`{key: message}`) to re-ask with messages under the fields. On any answering verdict that is not a `field_errors` re-ask, moss persists submitted values matching your declared settings — `secret` to the keystore, the rest into the plugin's config. moss draws every dialog; you never build UI. Declare `"check": true` in the contribution's `setup` block so moss knows to ask — a plugin that declares nothing pays nothing, and manifest `needs` (like `"stack"`) are checked host-side before your code ever spawns. New types: `SetupContext`, `SetupVerdict`, `SetupBlocker`, `SetupForm`, `SetupField`.
- The reserved `moss:` prefix for `SetupBlocker.id`, and the first id under it: return `{ id: "moss:credentials", message: "..." }` from `check_setup` and moss opens its own credential modal for the secrets your manifest declares, then probes again from the start. Use it when the thing in the way is a credential you already hold and the service rejected — an id of your own can only re-invoke the hook, which hands back the same blocker, since the bad token is stored and moss's publish gate asks only for what is missing. moss never dispatches an id under this prefix to your hook, and refuses one it does not implement, so an id moss defines later cannot silently take over one of yours.
- Every field your manifest declares under `setup.settings` now renders on your plugin's settings page — `string`, `number`, `boolean`, and `secret` types, with `options` for dropdowns, `when` for conditional fields, and `pattern` validated as the user types. The older `config_schema`/`config_labels`/`config_descriptions`/`config_options`/`config_placeholders` maps and `setup.credentials` still parse, as aliases folded into `settings`; `config_verify` is removed — no plugin ever used it.
- `moss.getSecret(key)`, `moss.rejectSecret(key)` and `moss.setSecret(key, value)` — credentials moss holds for your plugin. `getSecret` returns the stored token or `null`. `rejectSecret` says the token no longer works: moss forgets it, asks the user for a replacement in its own modal, and resolves with the new one, so your error path is catch → reject → retry rather than a failed publish and an explanation. All three are scoped to your plugin automatically: you pass no plugin id, and you cannot name another plugin's secret. You never draw a credential input — moss asks, driven by the `secret` fields in `setup.settings`, and the registry refuses a plugin that draws its own password field. What you may do is store what an authenticated flow already handed you, which is `setSecret`; it admits a declared `hidden` secret (one no person should ever type) and refuses a user-supplied or undeclared key, because those hold what the user typed into moss's modal and are moss's to write. If one of those needs replacing, `rejectSecret` is the way — moss forgets it and asks again. The test mock gains `secretStorage` (`seed`, `get`, `wasRejected`) so the 401-then-reject path is exercised for real.

### Changed

- **BREAKING: `ProjectInfo` now matches what moss actually sends.** `project_type` and `content_folders` are removed — moss stopped sending them months ago, so any plugin reading them was already getting `undefined`; derive structure from the file listings instead. `lang` is now required rather than optional, because moss always sends it.
- **BREAKING: the `generate` and `enhance` hooks are gone.** Neither was ever implemented by a published plugin, and moss no longer dispatches either one, so `GenerateContext`, `EnhanceContext`, `EnhanceContent`, `EnhanceResult`, `SourceFiles`, `FileInfo`, `PageNode` and the `"enhance"` member of `PluginHook` are removed. A plugin that declares `generate` or `enhance` in its manifest still installs; the hook simply never runs. Both designs are kept — if page fragments supplied by a plugin come back, they come back as a contribution whose shape is designed then, not as this entry point grandfathered in.
- `executeBinary`'s `onStderr` callback never fires. It has not fired since QuickJS became the default engine — the engine dropped the stream id and ran the blocking path — and the host code behind it is now gone rather than dormant. Read `result.stderr` after the call instead. The option is still accepted so existing plugins keep compiling.
- **The `MOSS_PLUGIN_ENGINE=webview` escape hatch is gone.** QuickJS is the only plugin engine, so the environment your plugin runs in no longer depends on what another plugin in the same project declares — previously one plugin declaring `enhance` moved every plugin in that project onto the webview engine. If your bundle only worked under the webview engine, it needs the QuickJS environment now; [`docs/runtime-environment.md`](docs/runtime-environment.md) lists the globals that differ.
- `setPluginCookie([])` is now rejected by the host instead of silently storing nothing. Call `clearPluginCookies()` to remove stored cookies — that has always been the verb that works, and the error message now says so. The test mock (`@symbiosis-lab/moss-api/testing`) matches: it refuses the empty write and implements `clear_plugin_cookies`, which it previously did not, so a plugin's clear path is exercised for real rather than passing against a mock that cleared when the host did not.

### Fixed

- Git installs now include the built JavaScript and declaration files in the repository, so a package-manager cache cannot leave `@symbiosis-lab/moss-api` without its exported types.
- `setSecret(key, "")` now erases the key instead of storing an empty string. Signing a user out is how plugins have always spelled this, but moss stored the empty value and every stored-ness check reads `is_some()`, so the account stayed listed as connected on the settings page and moss's publish gate believed it still held a credential nothing could authenticate with. Empty files written by earlier releases also read as absent now, so an account already signed out reports correctly without the user doing anything.

## [0.12.0] - 2026-08-20

### Added

- **`toast` on `HookResult`, and the `HookToast` type it carries.** A hook now *describes* the outcome it wants surfaced — `{ outcome: "success" | "info" | "error", title: string, url?: string | null }` — instead of raising a toast itself mid-run. moss maps that description onto its own severity, timing and suppression rules, which is what lets a surface that already reports the outcome (the first-publish wizard, showing the address it just published to) swallow the duplicate rather than stack a second notice on top of it. Optional and additive: a hook that returns no `toast` behaves exactly as before.

### Changed

- **`showToast()` is no longer the documented way to report a hook's outcome.** The function is unchanged and still exported; what changed is the guidance on `HookResult` and `HookToast`, which now says that moss owns every status surface and that outcome UX travels back as data in the return value. The practical reason to migrate is that an imperative toast cannot be reconciled with the surfaces moss is already showing for the same operation — and that a hook has to succeed with no UI attached at all, since CLI and headless hosts run the same hooks.

## [0.11.0] - 2026-08-12

### Removed

- **Thirteen exports no plugin has ever called:** `isTauriAvailable`, `getMessageContext`, `reportComplete`, `createSymlink`, `readProjectFileBase64`, `listSourceFiles`, `listSocialFiles`, `listPluginFiles`, `waitForEvent`, `isEventApiAvailable`, `updateToast`, `clearPlatformCache`, and `resolveBinary` (with `BinaryResolutionError`) — verified against every plugin in both repositories. `resolveBinary` was worse than unused: it wrapped a command that is not a QuickJS host function, so any call to it failed on every version of moss.

  A **minor** bump, not a patch, on purpose. Plugins pin `^0.10.0`, which for a 0.x package admits every 0.10.x — a patch release would have installed these removals automatically and broken such a plugin with no signal. 0.11.0 falls outside that range, so an author upgrades deliberately.

  _Recorded on 2026-08-21._ This entry existed only as an unconsumed changeset file and so never reached the changelog; 0.11.0 was also never published to npm, which went 0.10.0 → 0.12.0. An author upgrading across that gap meets these removals here.

### Added

- Topic guide [`docs/plugin-manifest.md`](docs/plugin-manifest.md) — every manifest field, grouped by what it is for: who your plugin is, what the user gains by installing it, and what the user is trusting it with. Covers the `contributes` keys, config vs. plugin-private state, why a new plugin should ship `"preview": true`, and the two rules that are easy to learn the hard way — a deploy hook must work with no UI, and a raw `invoke()` for a command that isn't a host function fails on every version of moss. The manifest reference in the monorepo's CONTRIBUTING.md was a year stale (it described a webview runtime, example plugins that no longer exist, and omitted nine shipped fields); this file replaces it, and ships in the published package so external authors can actually read it.
- `getKey(name, algorithm)`, `listKeys()`, and `signWithKey(name, payload)` — a keystore for plugin-owned keys. moss holds the key bytes and signs on request; your plugin never receives them. Keys are scoped to your plugin automatically (no id to pass, no other plugin's key reachable) and need no manifest declaration — creating and using your own key spends nothing of anyone else's. `ed25519` (IPNS's key type) and `secp256k1-schnorr`. New `KeyAlgorithm` and `KeyInfo` types. Replaces the never-shipped `identitySign` API.
- `exposeAdvisoryPath()` utility — plugins can read the advisory output path for the current build. (Listed under 0.8.0 below, but never actually included in a published bundle until now.)
- `createMockDialogTracker`, `MockDialogTracker`, and `MockDialogResult` are now exported from `@symbiosis-lab/moss-api/testing`. They were always reachable via `MockTauriContext.dialogTracker` but could not be imported directly, unlike every other mock tracker.
- Topic guide [`docs/runtime-environment.md`](docs/runtime-environment.md) — the QuickJS plugin-runtime contract: available globals (and the missing ones: Web Streams, `crypto.subtle`, DOM), `fetch` shim limits, path-preserving multipart uploads, console→log-level mapping, the env-var allowlist, config merge precedence (`config.json` > `config.toml` > manifest defaults), the plugin keystore for signing instead of WebCrypto, and why green Node-mock tests don't prove QuickJS compatibility.
- Generated API reference at [`docs/api/`](docs/api/README.md) — every exported function, type, and interface, built from source doc comments with TypeDoc. Regenerate with `pnpm run docs`; CI flags a reference that has drifted from the source.

## [0.10.0] - 2026-06-24

_This entry was reconciled retroactively on 2026-08-12 against the published
npm tarball: 0.10.0 shipped without a changelog cut, so the items below sat
under Unreleased for seven weeks after users could already install them._

### Added

- First publish via the open-source release pipeline. moss-api source consolidated into the moss monorepo; CI build pipeline replaces the standalone repo's build setup.
- `httpPostMultipart(url, { textFields, files }, options)` — POST a `multipart/form-data` body (ordered text fields + base64 file parts). Enables binary uploads that the JSON-only `httpPost` cannot express, e.g. uploading image/audio bytes (read via `readSiteFile`) to a syndication target's GraphQL `singleFileUpload`. moss builds the multipart body, generates the boundary, and sets the Content-Type. New `MultipartTextField` / `MultipartFilePart` / `MultipartPostOptions` types.
- First npm appearance of the [0.8.0] items below: `SocialComment`, `contributes.jobs`, and `startTask()` (`exposeAdvisoryPath()` did not make the bundle; it ships in 0.11.0).

### Removed (BREAKING)

- Thirteen exports no plugin has ever called, removed while removing them is still cheap: `isTauriAvailable`, `getMessageContext`, `reportComplete`, `createSymlink`, `readProjectFileBase64`, `listSourceFiles`, `listSocialFiles`, `listPluginFiles`, `waitForEvent`, `isEventApiAvailable`, `updateToast`, `clearPlatformCache`, and `resolveBinary` / `BinaryResolutionError` with their types. Verified against every plugin in both repositories (the monorepo and moss-registry). Each one was a promise the SDK would have had to keep indefinitely for no one; `resolveBinary` was worse than unused — it wrapped `resolve_binary_command`, which is not a QuickJS host function, so any call to it failed on every version of moss. If you need one of these back, open an issue: a request from a real plugin is exactly the evidence that should bring an API into the SDK.
- `utils/window.ts`, a module that exported nothing.

`showBrowserForm` is also uncalled, and stays only because it is already marked `@deprecated` with a migration path (`openBrowserWithHtml()` + explicit `closeBrowser()`); deleting it is the second half of a deprecation, scheduled separately. It is not a form API — it takes raw HTML, exactly like `openBrowserWithHtml`, and adds only lifecycle: hidden submit/cancel listeners, auto-close, and a five-minute timeout. It never solved the reason plugins hand-write panel stylesheets, so its removal costs nothing there.

### Changed (BREAKING)

- `PageNode.unlisted` renamed to `PageNode.draft`. The `unlisted` frontmatter field was removed from moss; page visibility is now expressed via `draft` (a draft renders and is published at its direct URL but is hidden from listings, feeds, sitemap, and navigation).

_Pending publish — cumulative since `0.7.12` (last released on main); full detail under [0.8.0]._

- `SocialComment` social-layer comment type, `contributes.jobs` plugin job descriptor, `startTask()` API, and `exposeAdvisoryPath()` utility.

## 0.10.0

### Minor Changes

- Thanks [@guoliu](https://github.com/guoliu)! - First publish via the open-source release pipeline. moss-api source consolidated into the moss monorepo; CI build pipeline replaces the standalone repo's build setup.

## [0.8.0] - 2026-06-11

_Never published to npm — the `0.8.0` version number was already used by an
unrelated release in January 2026, so this cut skipped npm; its contents first
reached users in 0.10.0 (except `exposeAdvisoryPath`, which ships in 0.11.0)._

### Added

- `SocialComment` type for social-layer comment data (comments written to the platform, not local).
- `contributes.jobs` descriptor: plugins declare job verbs and amount types, normalized at load time.
- `startTask()` API alongside existing `reportProgress` — `TaskHandle` methods no-op when Tauri is unavailable (safe in test/SSR contexts).
- `exposeAdvisoryPath()` utility — plugins can read advisory output path for the current build.

## [0.8.0-rc.1] - 2026-05-29

### Changed

- Source consolidated into the moss monorepo. Build pipeline replaces the standalone repo's build setup. Continues lineage from `@symbiosis-lab/moss-api@0.7.12`.
- `TaskHandle.noOp()` guard added so API methods are safe to call outside Tauri context.
