# Landing documentation audit — 2026-09-18

Compared `origin/develop` at `b2ff97f` with the public site sources and product code. This is an integration record, not a public site page.

## Landing destinations

| Topic | Human introduction | Deeper reference | Coverage |
|---|---|---|---|
| Editor | `/get-started/editor/` | `/docs/writing/editor/` | Added in English, Simplified Chinese, and Traditional Chinese |
| Themes | `/get-started/design/` | `/docs/design/` | Existing introductions in all three languages; English reference |
| Media | `/get-started/links/` | `/docs/writing/media/` | Existing introductions in all three languages; refreshed audio and document coverage |
| Plugins | `/get-started/extend/` | `/docs/extend/` | Existing introductions in all three languages; English reference |
| Getting started | `/get-started/` | `/docs/` | Existing in all three languages |
| Plugin registry | [Public registry](https://github.com/Symbiosis-Lab/moss-registry) | [Live index](https://symbiosis-lab.org/moss-registry/index.json) | Four published plugins are indexed, all marked preview; the in-app catalog is not shipped, so installation currently uses the registry release ZIPs |

Localized routes prepend `/zh-hans/` or `/zh-hant/` to their translated Get Started folder and keep the explicit leaf slugs (`editor`, `design`, `links`, `extend`).

## Feature audit

The hand-edited Get Started set already describes the major shipped authoring surface: the split editor and preview, file-tree site structure, formatting toolbar and slash menu, drag-and-drop media, wikilinks, notebooks, themes, plugins, publishing, domains, email, and Matters sync.

The missing bounded introduction was the editor itself. The new page documents behavior verified in current code and release history: live preview, collapsible split panes, selection toolbar, slash menu, Finder drag-and-drop, file-tree organization, link updates on rename, delete impact confirmation, and preview-to-source navigation.

The media introductions omitted audio and documents even though the current resolver and build registry handle them. The three language pages now name the supported audio formats and PDF delivery. Reliability, cache, deploy-integrity, and vertical-writing fixes since the prior site edit improve existing behavior but do not introduce new authoring concepts that need landing destinations.

The theme and plugin introductions already provide the requested human-level entry points. Their deeper English guides and reference tables cover the implementation contracts; duplicating those tables into the landing path would make the hand-edited introductions harder to maintain.

The registry is maintained in a separate public repository. Its live index was
audited at serial 34 on 2026-09-18 and listed GitHub, IPFS, Matters, and
Onionpress, each with `preview: true`. The landing may link to the repository,
but must not imply that a general in-app catalog is available yet.
