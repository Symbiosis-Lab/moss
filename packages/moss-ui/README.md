# @symbiosis-lab/moss-ui

The moss UI's side of the moss engine. Today it holds the contract: every command the UI sends to the engine, with its argument and answer types and its trust tier (`PUBLIC_READS`, `SESSION_READS`, `WRITES`). A transport (the desktop app's in-process calls, the HTTP carrier the CLI serves) uses the tier to pick its route and the types to check what it sends.

`src/engine.generated.ts` is generated from moss-build's command table and checked by a test that fails when it is stale. After changing a command, regenerate it:

```sh
UPDATE_ENGINE_CONTRACT=1 cargo test -p moss-build --lib commands::tests::the_contract
```
