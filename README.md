# moss

[![Latest release](https://img.shields.io/github/v/release/Symbiosis-Lab/moss?label=release)](https://github.com/Symbiosis-Lab/moss/releases/latest) [![License](https://img.shields.io/github/license/Symbiosis-Lab/moss)](LICENSE) [![npm](https://img.shields.io/npm/v/%40symbiosis-lab%2Fmoss)](https://www.npmjs.com/package/@symbiosis-lab/moss)

**Write anywhere. Publish everywhere. Own everything.**

moss turns a folder of markdown into a website. The folder on your disk is the site: there is no database, no CMS and no project to configure. Write in any editor, and moss builds the pages, the navigation, the styling and dark mode, then publishes them.

<img src="site/assets/animations/new%20folder.gif" width="720" alt="A folder of markdown files becomes a website in the moss app">

## Why moss

Your files stay on your computer. Your site lives on your domain. Your audience remains yours.

Like its namesake, moss thrives in the spaces others overlook. It doesn't compete with platforms for sunlight. It creates the foundation layer that enables an ecosystem: you publish to your own site first, and to the platforms your readers use second.

- **No setup.** Point moss at a folder. Sensible defaults stand in for a build config.
- **Any editor.** moss's own app, Obsidian, Typora, or anything else that saves markdown.
- **Media handled.** Images, video, HTML and Jupyter notebooks. Drop them in the folder.
- **Publish and syndicate.** To a `*.mosspub.com` address or a domain you own, then on to other platforms, with comments synced back.
- **Yours to extend.** CSS, JavaScript and plugins, with no generator config to learn.
- **Ready for coding agents.** `moss guide` and `moss describe` give an agent the conventions, the contract and the checks it needs to build a site correctly.

## Status

moss is in beta. Building, previewing and editing are open to everyone. Publishing a new site to `*.mosspub.com` is by invitation while the beta is closed: [request access](https://mosspub.com). The built site is plain static files, so you can also host it anywhere yourself.

## Install

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

Or download a binary or the macOS/Windows app directly from [Releases](https://github.com/Symbiosis-Lab/moss/releases/latest).

## Quick start

```sh
moss build ~/blog/            # build once
moss build ~/blog/ --serve    # build and serve a local preview
moss list ~/blog/             # inventory every page: kind, URL, title, lang
moss deploy ~/blog/ --site-id=my-blog   # first publish: registers my-blog.mosspub.com (by invitation during the beta)
moss deploy ~/blog/           # build and publish on every later run
```

`moss preview` and `moss edit` open a window and need the desktop app; the CLI binary hands off to it if installed, or tells you how to get it. Run `moss --help` for the full command list.

For a coding agent, `moss guide` prints the conventions and rules, and `moss describe --json` prints the whole contract: tokens, components, frontmatter and commands.

## Where moss is going

Designs for things moss does not do yet live in [docs/proposals](docs/proposals/), each with an issue where it is discussed. A proposal is not a promise. The [`proposal` label](https://github.com/Symbiosis-Lab/moss/labels/proposal) lists everything under consideration.

## Contributing

Bug reports, comments on proposals and pull requests are welcome. [CONTRIBUTING.md](CONTRIBUTING.md) says how to build, how to check a change and what we ask of contributors. Issues labelled [`good first issue`](https://github.com/Symbiosis-Lab/moss/labels/good%20first%20issue) are sized for a first contribution.

The desktop app for macOS and Windows is built from a private repository. Its releases, its bug reports and its designs live here.

## Repository layout

[ARCHITECTURE.md](ARCHITECTURE.md) says what each crate owns and where new code belongs.

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

## License

[MIT](LICENSE)
