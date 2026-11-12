# Proposals

A proposal is a design for something moss does not do yet. It is written before the code, so anyone can see where moss may go and say what is wrong with a design while changing it is still cheap.

Each proposal is a folder here: a `README.md` with the design, and its screens beside it when the feature has any. A proposal opened for public discussion has one issue, linked at the top and labelled [`proposal`](https://github.com/Symbiosis-Lab/moss/labels/proposal), where that discussion happens; the others are discussed and revised here.

| Proposal | Status |
|---|---|
| [Resumable imports and the Activity list](resumable-import/README.md) | Proposed |
| [Facts, cascades and groups: design without template code](fields-pointers-views/README.md) | In progress |
| [Leave specific subscribers out of one newsletter send](subscriber-exclusion/README.md) | In progress |
| [Offer missing tools with a Download button](tool-download-offer/README.md) | In progress |
| [A site's config version is written on purpose](config-written-on-purpose/README.md) | In progress |
| [Detail layers: a historical map or today's streets under a site's places](map-detail-layers/README.md) | In progress |
| [Moving an unlisted folder without publishing its address](unlisted-folder-redirects/README.md) | In progress |
| [Diagrams from Mermaid text](mermaid-diagrams/README.md) | In progress |
| [Request forms: a form written in markdown, sent to a form service](request-forms/README.md) | In progress |
| [Syndication: one contract for posting copies to other networks](syndication/README.md) | In progress |
| [Country maps: colour whole countries from a table](country-maps/README.md) | In progress |
| [A position ruler for a row along an ordered axis](position-ruler/README.md) | Proposed |
| [Duplicate stores in a synced folder: heal them, and stop making them](synced-store-duplicates/README.md) | Shipped |
| [Keep what a build reads downloaded, in a cloud site folder](keep-site-downloaded/README.md) | Proposed |
| [Comment moderation that survives a new site key](moderation-key-continuity/README.md) | In progress |
| [Plugin listing styles in body embeds](plugin-styles-in-embeds/README.md) | Proposed |
| [A hero that stays while the page rises over it](fixed-hero/README.md) | Proposed |
| [A site-wide date style](site-date-style/README.md) | Proposed |

## Status

The first lines of a proposal say where it stands.

| Status | Meaning |
|---|---|
| Proposed | Written down and open for comment. Not scheduled. |
| Planned | The maintainers intend to build it as written. No date is implied. |
| In progress | Being built. The proposal, or its issue if it has one, tracks what has landed. |
| Shipped | Released. The proposal stays as history, and the docs describe what moss does now. |
| Withdrawn | Not going ahead, with the reason. |

A proposal is not a promise. Designs change when they meet the code, and some are never built.

## What lives where

The CLI, the build engine and the packages are in this repository. The desktop app is built from a private one. A proposal may still show what the app would look like, because the people who use the app are the ones who can say whether a design is right. Each proposal marks which of its pieces live in this repository. Those are the pieces a pull request can pick up.

## Taking part

- To comment on a proposal, use its issue if it has one, or open one.
- To build a piece of one, say so in its issue first, so the approach can be agreed before you spend time on it.
- To suggest something new, open a feature request. If it grows into a design, it gets a folder here.
