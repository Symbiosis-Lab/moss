# hooks-site

Covers: docs/extend/hooks.md

Fixture for CLI plugin hook integration tests. Renamed from `plugin-test-site`.

## Layout

- `input/` — site root with a co-located `.moss/plugins/test-hook-plugin/`.
  The plugin creates marker files when hooks fire; tests assert on those markers
  plus `.moss/build.nosync/` state after build.
- `expected/` — output of a `--no-plugins` build (the no-hook baseline). Hook
  tests build `input/` live with various flag combinations and do not diff
  against `expected/`.

Referenced by:
- the desktop app's plugin integration tests (via `get_hooks_site_path()`).
- `tests/e2e/tests/cli-hooks.test.ts` (via `FIXTURE_PATH`).

See `basic-site/README.md` for the regeneration procedure.
