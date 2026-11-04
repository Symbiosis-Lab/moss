# Measurements behind the proposal

The scripts that produced the numbers in the proposal. They are evidence for a design, not part of the build or its tests.

- `node-render.mjs <pele.js url> <dir>`: renders every `.mmd` in a folder in Node, twice, and reports time, size and whether the two renders matched.
- `scan.mjs <pele.js url> <dir>...`: renders each file and reports errors with their line, and a coarse scan of the output for unsafe markup. It is a text scan, so escaped text can trip it; the proposal's sanitiser parses instead.
- `qjs-proto/`: the same renders inside `rquickjs` 0.12.0, the QuickJS binding moss already links. `BUDGET_MS` sets the per-diagram time budget, `OS_STACK_KB` and `QJS_STACK_KB` the thread stack and the engine's stack limit, and `PROBE_DEPTH` runs a plain recursion to show whether a too-deep call is refused or aborts the process.
- `measure-browser.mjs <built-site-dir> <pele-dist-dir> <out.json>`: takes a page built by `moss build`, makes one copy per rendering shape and opens each in chromium and webkit at 375 and 1280 pixels, light and dark, with JavaScript on and off. `measure33.json` is its output for a page with one diagram of each of the 33 types from `crates/moss-build/tests/fixtures/mermaid-corpus/valid/`.

Two results are not in the scripts' output and were measured by hand. With the engine's stack limit set below the thread's stack, a deep recursion is refused with a `RangeError` and the process carries on; set above it (4 MiB limit on a 2 MiB thread), the process aborts with a stack overflow. And decompressing the brotli-compressed renderer takes about 3 ms of the 62 ms it takes to load.
