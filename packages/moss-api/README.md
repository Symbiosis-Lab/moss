# @symbiosis-lab/moss-api

> TypeScript API for writing moss plugins.

[![npm](https://img.shields.io/npm/v/@symbiosis-lab/moss-api)](https://www.npmjs.com/package/@symbiosis-lab/moss-api)
[![downloads](https://img.shields.io/npm/dm/@symbiosis-lab/moss-api)](https://www.npmjs.com/package/@symbiosis-lab/moss-api)
[![license](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)
[![bundle size](https://img.shields.io/bundlephobia/minzip/@symbiosis-lab/moss-api)](https://bundlephobia.com/package/@symbiosis-lab/moss-api)

> **Read-only mirror.** Source lives in the private moss monorepo. PRs cannot be merged here — see [CONTRIBUTING.md](CONTRIBUTING.md).

[moss](https://mosspub.com) is a desktop publishing app; this package is its plugin API surface. Use it to write plugins that publish posts, manage site assets, or extend the editor.

- [Quickstart](#quickstart)
- [Anatomy of a plugin](#anatomy-of-a-plugin)
- [API reference](#api-reference)
- [Stability](#stability)
- [Discussions](https://github.com/Symbiosis-Lab/moss-api/discussions) · [Issues](https://github.com/Symbiosis-Lab/moss-api/issues) · [moss.pub](https://mosspub.com)

## Quickstart

```sh
npm install @symbiosis-lab/moss-api
```

A plugin is an object whose methods are hooks; moss calls them with a typed context:

```ts
import type { DeployContext, HookResult } from "@symbiosis-lab/moss-api";
import { reportProgress } from "@symbiosis-lab/moss-api";

export default {
  async deploy(context: DeployContext): Promise<HookResult> {
    await reportProgress("deploying", 0, 100, "Initializing");
    // ... deployment logic
    return { success: true, message: "Deployed successfully" };
  },
};
```

Test plugins without a running moss app via the mock layer:

```ts
import { setupMockTauri } from "@symbiosis-lab/moss-api/testing";
```

## Anatomy of a plugin

A plugin is three things:

1. **A manifest** (`assets/manifest.json`) — who you are, what the user gains by
   installing you, and what they're trusting you with. See
   [the manifest reference](docs/plugin-manifest.md).
2. **A bundle** (`dist/main.bundle.js`) — one IIFE built with esbuild, exposing
   your plugin object under the manifest's `global_name`.
3. **An icon** (`assets/icon.svg`).

Ship the manifest, the bundle and the icon together; that trio is what moss
installs.

Your code runs in **QuickJS, not a browser** — no DOM, no `crypto.subtle`, no
Web Streams, and `fetch` is a buffered shim. Read
[the runtime environment](docs/runtime-environment.md) before you reach for a
web API; green Node-mock tests do not prove QuickJS compatibility.

## API reference

The full reference — every hook context, utility, and testing mock, generated from the source doc comments — lives in [docs/api](docs/api/README.md).

Topic guides: [the plugin runtime environment](docs/runtime-environment.md) (what globals exist under QuickJS, fetch limits, config merge rules — read this first), [the plugin manifest](docs/plugin-manifest.md), [plugin authentication](docs/plugin-auth.md).

Regenerate after an API change with `pnpm run docs` (CI flags a reference that has drifted from the source).

## Stability

This package is 0.x. The API may change between minor versions until 1.0.
Breaking changes are documented in [CHANGELOG.md](./CHANGELOG.md).

## License

MIT — see [LICENSE](LICENSE).
