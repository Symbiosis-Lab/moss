# @symbiosis-lab/moss

moss on your PATH. This package downloads the open [moss-cli](https://github.com/Symbiosis-Lab/moss/tree/develop/crates/moss-cli) binary and exposes it as the `moss` command.

## Install

```sh
npm install -g @symbiosis-lab/moss
moss --version
```

The binary is fetched at install time (postinstall). With `--ignore-scripts`, or if the download failed (e.g. offline), the first `moss` invocation fetches it lazily instead.

Supported platforms: macOS (Intel and Apple Silicon, one universal binary) and Linux x86_64. The binary has no window system in its dependency graph, so it starts on a headless Linux server or inside a container with no desktop libraries to install first.

## Use

```sh
moss build ~/Sites/my-site
```

All arguments pass straight through to the moss binary; see `moss --help`.

`moss preview` and `moss edit` need a window, which this binary doesn't have: they hand off to [moss desktop](https://mosspub.com) if it's installed, or print a one-line hint if it isn't. On macOS, `moss desktop install` fetches and installs it for you (see `moss desktop install --help`); it always asks before downloading and never runs unattended (no terminal attached, as in CI, is treated as declining). moss desktop isn't packaged for Linux or Windows yet, so `moss desktop install` there just prints where to get it.

## Checksums

Downloads are verified against the `SHA256SUMS` file published with each release. Releases that predate checksum publishing install with a warning instead of verification.

## Versioning and overrides

The wrapper downloads the moss version matching its own package version; they are released in lockstep. `MOSS_VERSION_OVERRIDE=v0.11.1` forces a specific release (ops/testing escape hatch).

## Controlling the download

| Variable | Effect |
|---|---|
| `MOSS_SKIP_BINARY_DOWNLOAD=1` | Install the wrapper without fetching a binary. `moss` then fetches it on first run. |
| `MOSS_BINARY_PATH=/path/to/moss` | Run that binary instead of the vendored one, and skip the download entirely. |
| `MOSS_FORCE_BINARY_DOWNLOAD=1` | Download even where the wrapper would otherwise skip. |

Each also reads from `.npmrc` under its lowercased name (`moss_skip_binary_download=true`), because npm passes config keys to lifecycle scripts as `npm_config_*`.

**Inside the moss repo the download is skipped automatically.** A checkout is not an install: `packages/moss` sits in the workspace rather than under `node_modules`, so `pnpm install` at the repo root would otherwise pull a release binary that no moss developer wants — they build from source. Point `MOSS_BINARY_PATH` at `target/release/moss-cli` (`cargo build --release -p moss-cli`) to exercise the wrapper against your own build.

## License

The wrapper code in this package is MIT (see LICENSE). The moss binary it downloads is a separate work, distributed under its own terms via [moss](https://github.com/Symbiosis-Lab/moss).
