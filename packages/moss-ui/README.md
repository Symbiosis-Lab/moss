# @symbiosis-lab/moss-ui

The moss UI: its typed contract with the moss engine, and the UI code every host of moss shares. Public source, not published to npm. The editor and preview are not in it yet.

## The contract

Every command the UI sends to the engine, with its argument and answer types and its trust tier (`PUBLIC_READS`, `SESSION_READS`, `WRITES`). A transport (the desktop app's in-process calls, the HTTP carrier the CLI serves) uses the tier to pick its route and the types to check what it sends. Import it as `@symbiosis-lab/moss-ui`.

`src/engine.generated.ts` is generated from moss-build's command table and checked by a test that fails when it is stale. After changing a command, regenerate it:

```sh
UPDATE_ENGINE_CONTRACT=1 cargo test -p moss-build --lib commands::tests::the_contract
```

## The UI

`src/ui/` holds the UI code that does not depend on which program shows it. It reaches whatever runs it through one record, `src/ui/host.ts`: the engine and its events, the bus between the UI's parts, links, file picks, and the optional members only some hosts have (a native window, a file manager, files on disk). A host fills that record before the UI mounts, and where a member is absent the UI shows no control for it.

Today `src/ui/` holds the UI's foundations: the strings and their locales (`i18n/`), shared controls and the toast stack (`shared/`), small components (`components/`), the design tokens and shared stylesheets (`styles/`), helpers (`utils/`, `lib/`), the log, and the host record. Import a module by its path under `ui/`, with no extension, or a stylesheet by its file name:

```ts
import { t } from '@symbiosis-lab/moss-ui/ui/i18n/index';
import '@symbiosis-lab/moss-ui/ui/styles/shared/tokens.css';
```

## Tests and checks

```sh
pnpm --filter @symbiosis-lab/moss-ui test        # contract, UI (node and jsdom) and the package's scripts
pnpm --filter @symbiosis-lab/moss-ui typecheck
MOSS_SCRUB_RULES=<rules.json> node packages/moss-ui/scripts/public-ready.mjs   # the UI's comments, names and licences are fit to publish
```

The command checks private names in every file of the package against the rules file `MOSS_SCRUB_RULES` names, and fails when that file is missing or holds no rules. That list is not published, so the package's own test runs every other check of the same script.
