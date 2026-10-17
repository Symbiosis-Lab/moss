---
title: Write a plugin
url: extend
uid: 944cabf2
weight: 50
description: Build a JavaScript plugin that hooks into the moss build pipeline — from a minimal working example to the full reference.
---

A plugin is a JavaScript bundle you drop into `.moss/plugins/<name>/`. moss finds it automatically, reads its manifest, and calls the hooks it declares at the right stage of every build.

## Minimal working plugin

Start with this layout inside your project:

```
.moss/plugins/my-plugin/
├── manifest.json
├── main.bundle.js
└── icon.svg
```

A `manifest.json` registering one hook:

```json
{
  "name": "my-plugin",
  "version": "1.0.0",
  "description": "Does one thing well",
  "author": "Your Name",
  "entry": "main.bundle.js",
  "capabilities": ["enhance"],
  "global_name": "MyPlugin"
}
```

A `main.bundle.js` that exports the hook as a property on the global object named in `global_name`:

```js
window.MyPlugin = {
  async enhance(ctx) {
    return {
      slots: {
        "after-article": "<p>Added by my-plugin.</p>"
      }
    };
  }
};
```

That is a complete plugin. Copy it into `.moss/plugins/my-plugin/`, build, and it runs.

## How the pipeline works

When moss compiles a site, it runs through five stages in order:

```
process → generate → enhance → deploy → syndicate
```

A plugin attaches to one or more stages by listing capability names in `capabilities`. moss calls the matching method on your global object at the right moment, passes a typed context object, and reads the return value.

**process** runs before HTML generation. Use it to fetch external data or transform source files. Multiple plugins can have this capability.

**generate** builds source content into HTML. Only one plugin can have this capability; it replaces moss's built-in generator. Use it to plug in Hugo, Astro, or another SSG backend.

**enhance** injects content into named template slots after HTML is generated. Multiple plugins can have this capability. Use it for comments, analytics, newsletter forms, or any per-page injection.

**deploy** pushes the compiled site to a hosting platform. Only one plugin can have this capability. Use it for GitHub Pages, Netlify, or custom hosting.

**syndicate** distributes published content to external platforms (POSSE). Multiple plugins can have this capability. Use it for cross-posting to Matters.town, Substack, or social media.

An enhance plugin injects into named template slots. See [[slots]] for the full list, their positions, and the `EnhanceResult` shape.

## Building the bundle

moss loads `main.bundle.js` as a script tag. Bundle with esbuild as an IIFE so the global object lands on `window`:

```sh
esbuild src/main.ts \
  --bundle \
  --format=iife \
  --global-name=MyPlugin \
  --outfile=main.bundle.js
```

The output file goes directly into `.moss/plugins/my-plugin/`. `global_name` in the manifest must match the `--global-name` flag.

## SDK

Install `@symbiosis-lab/moss-api` for TypeScript types and utilities.

```sh
npm install @symbiosis-lab/moss-api
```

It exports typed context interfaces (`ProcessContext`, `GenerateContext`, `EnhanceContext`, `DeployContext`, `SyndicateContext`), the `HookResult` return type, and utilities like `reportProgress`, `reportError`, and `openBrowser`. Every context, utility, and type is documented in the [full API reference](https://github.com/Symbiosis-Lab/moss-api/tree/main/docs/api), generated from the source.

For Rust contributors working on moss itself, the core types are published as `moss-core` on [docs.rs](https://docs.rs/moss-core).

## Privileged capabilities

Most of the SDK is available to every plugin. A few capabilities can reach outside the sandbox in ways the reader cannot undo, so moss refuses them unless your manifest asks for them by name:

```json
{
  "requires": ["execute_binary"]
}
```

One is gated today:

- `execute_binary` — running a native program. The GitHub plugin uses it to run `git`.

If a plugin calls it without declaring it, the call fails with an explanatory error rather than running. Declaring nothing is the safe default, and an undeclared call is never granted by accident. Note that using your own keys (below) is **not** gated — it costs the user nothing, so it needs no declaration.

## Your keys

Your plugin can have its own keys — for signing an IPNS record, a Nostr event, anything a protocol needs. moss holds the key bytes and signs on request; **your plugin never receives them.** This is the arrangement a hardware wallet or a browser's non-extractable key uses: the key stays usable and stays yours, but a compromised build of your plugin cannot walk off with it.

```ts
import { getKey, signWithKey } from "@symbiosis-lab/moss-api";

// Get (or create, the first time) your key. Idempotent.
const key = await getKey("ipns", "ed25519");   // key.publicKey is yours to use

// Sign bytes you built. The signature is the algorithm's standard form.
const signature = await signWithKey("ipns", myRecordBytes);
```

Three things worth knowing:

- **The keys are yours, automatically.** You don't pass a plugin id, and you can't reach another plugin's key — two plugins that both call `getKey("ipns")` get two different keys. `listKeys()` lists only yours.
- **No permission needed.** Creating and using your own key spends nothing of the user's or another plugin's, so there's nothing to declare in your manifest.
- **You own the protocol.** moss signs exactly the bytes you give it. Any framing — an IPNS record's `ipns-signature:` prefix, a Nostr event id — you build before signing.

Two algorithms: `ed25519` (IPNS's key type, and the right default for most protocols) and `secp256k1-schnorr` (Nostr-family).

Why moss holds the bytes instead of handing them to you: a key is the durable identity behind a name you publish — an IPNS name *is* its public key and can't be rotated. Left in your plugin's folder it would be committed to the user's repo and pushed. moss keeps it out of git and lets the user back it up, while you keep full use of it. Worth telling your users that a site's identity lives in `.moss/`, which is worth backing up.

## A plugin with no script

A plugin that only turns one kind of file into text needs no JavaScript. It declares `contributes.derivations` in its manifest: the file extensions it accepts, the names of the text pieces it returns, and a pinned binary that reads one file and prints JSON. moss runs the binary once per file, in a sandbox, and caches the answer by the file's content. The manifest reference has the fields and the stdin/stdout protocol; the author's side is [[Links & Embeds#Text from a file]].

## Reference

Field tables, hook context signatures, slot positions, and CLI flags are in the Reference section. Start with [[manifest]] for the full `manifest.json` field list, then [[hooks]] for context shapes and the plugin runtime lifecycle, [[slots]] for slot positions, and [[cli]] for headless build and automation commands.
