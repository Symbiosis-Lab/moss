# @symbiosis-lab/moss

moss on your PATH. This package downloads the official [moss](https://@symbiosis-lab/moss.com) CLI binary from [moss](https://github.com/Symbiosis-Lab/moss) and exposes it as the `moss` command.

> **Publishing note:** the package is currently marked `"private": true` in
> `package.json` so it cannot ship by accident. Flip that (remove the field)
> at publish time — `publishConfig` is already set up.

## Install

```sh
npm install -g @symbiosis-lab/moss
moss --version
```

The binary is fetched at install time (postinstall). With `--ignore-scripts`,
or if the download failed (e.g. offline), the first `moss` invocation fetches
it lazily instead.

Supported platforms: macOS (Intel and Apple Silicon, one universal binary)
and Linux x86_64.

## Use

```sh
moss build ~/Sites/my-site
```

All arguments pass straight through to the moss binary; see `moss --help`.

## Linux runtime dependency (temporary)

The current Linux binary links against WebKitGTK even for headless CLI verbs.
If `moss` fails with `error while loading shared libraries`, install:

```sh
sudo apt install libwebkit2gtk-4.1-0
```

This requirement is temporary — a moss CLI build without the WebKitGTK
linkage is planned.

## Checksums

Downloads are verified against the `SHA256SUMS` file published with each
release. Releases that predate checksum publishing install with a warning
instead of verification.

## Versioning and overrides

The wrapper downloads the moss version matching its own package version;
they are released in lockstep. `MOSS_VERSION_OVERRIDE=v0.11.1` forces a
specific release (ops/testing escape hatch).

## Controlling the download

| Variable | Effect |
|---|---|
| `MOSS_SKIP_BINARY_DOWNLOAD=1` | Install the wrapper without fetching a binary. `moss` then fetches it on first run. |
| `MOSS_BINARY_PATH=/path/to/moss` | Run that binary instead of the vendored one, and skip the download entirely. |
| `MOSS_FORCE_BINARY_DOWNLOAD=1` | Download even where the wrapper would otherwise skip. |

Each also reads from `.npmrc` under its lowercased name (`moss_skip_binary_download=true`),
because npm passes config keys to lifecycle scripts as `npm_config_*`.

**Inside the moss repo the download is skipped automatically.** A checkout is
not an install: `packages/moss` sits in the workspace rather than under
`node_modules`, so `pnpm install` at the repo root would otherwise pull a
release binary that no moss developer wants — they build from source. Point
`MOSS_BINARY_PATH` at `src-tauri/target/release/moss` to exercise the wrapper
against your own build.

## License

The wrapper code in this package is MIT (see LICENSE). The moss binary it downloads is a separate work, distributed under its own terms via [moss](https://github.com/Symbiosis-Lab/moss).
