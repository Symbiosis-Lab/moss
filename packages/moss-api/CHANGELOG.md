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

- First publish via the open-source release pipeline (#738). moss-api source consolidated into the moss monorepo; CI build pipeline replaces the standalone repo's build setup.
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

- #738 Thanks [@guoliu](https://github.com/guoliu)! - First publish via the open-source release pipeline. moss-api source consolidated into the moss monorepo; CI build pipeline replaces the standalone repo's build setup.

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
