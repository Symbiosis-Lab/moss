# moss

[![Latest release](https://img.shields.io/github/v/release/Symbiosis-Lab/moss?label=release)](https://github.com/Symbiosis-Lab/moss/releases/latest) [![License](https://img.shields.io/github/license/Symbiosis-Lab/moss)](LICENSE) [![CodeQL](https://github.com/Symbiosis-Lab/moss/actions/workflows/codeql.yml/badge.svg?branch=main)](https://github.com/Symbiosis-Lab/moss/actions/workflows/codeql.yml) [![npm](https://img.shields.io/npm/v/%40symbiosis-lab%2Fmoss)](https://www.npmjs.com/package/@symbiosis-lab/moss)

moss turns a folder of markdown files into a website: point it at a folder, and it builds, previews, and publishes the site.

There's no database, no CMS, and no lock-in — the folder on disk is the site. Edit the markdown with whatever editor you like (moss's own desktop app, Obsidian, Typora, anything), and `moss build` compiles it into static HTML with navigation, styling, and dark mode handled for you. `moss deploy` publishes it to a `*.mosspub.com` subdomain or a domain you own, and plugins extend the build without you writing a static-site generator config.

## Install

Desktop app (macOS): download `moss.dmg` from [Releases](https://github.com/Symbiosis-Lab/moss/releases/latest), or run `moss desktop install` from an existing CLI install to fetch and verify it automatically.

macOS (desktop app + CLI, as a Homebrew cask):

```sh
brew install --cask symbiosis-lab/tap/moss
```

Linux (CLI only, x86_64):

```sh
brew install symbiosis-lab/tap/moss
```

Node.js (CLI only, macOS x64/arm64, Linux x64, or Windows x64):

```sh
npm install -g @symbiosis-lab/moss
```

`cargo install moss-cli` is coming — the crate is reserved on crates.io but not yet published.

## Quick start

```sh
moss build ~/blog/            # build once
moss build ~/blog/ --serve    # build and serve a local preview
moss list ~/blog/             # inventory every page: kind, URL, title, lang
moss deploy ~/blog/ --site-id=my-blog   # first publish: registers my-blog.mosspub.com
moss deploy ~/blog/           # build and publish on every later run
```

`moss preview` and `moss edit` open a window and need the desktop app; the CLI binary hands off to it if installed, or tells you how to get it. Run `moss --help` for the full command list.

## Repository layout

| Path | What's there |
|---|---|
| `crates/moss-cli` | the `moss` CLI binary |
| `crates/moss-build` | the build engine the CLI and desktop app both call |
| `crates/moss-core` | pure-Rust parsing and schema code, no I/O |
| `packages/moss` | the `@symbiosis-lab/moss` npm wrapper that installs the CLI binary |
| `packages/moss-api` | the plugin API |
| `packages/moss-syntax` | shared markdown/frontmatter syntax tooling |
| `packages/obsidian-moss` | the Obsidian plugin |
| `site/` | the source of [mosspub.com](https://mosspub.com), including the docs |

## Documentation

Full docs: [mosspub.com/docs](https://mosspub.com/docs). Source: [site/docs/](site/docs/). Changes: [CHANGELOG.md](CHANGELOG.md).

## Issues

[Open an issue](https://github.com/Symbiosis-Lab/moss/issues) for the CLI, the build engine, any of the packages above, or the desktop app.

## License

[MIT](LICENSE)
