# Proposals

A proposal is a design for something moss does not do yet. It is written before the code, so anyone can see where moss may go and say what is wrong with a design while changing it is still cheap.

Each proposal is a folder here: a `README.md` with the design, and its screens beside it when the feature has any. Each has one issue, linked at the top and labelled [`proposal`](https://github.com/Symbiosis-Lab/moss/labels/proposal), where the discussion happens. That label is also the list of everything under consideration.

| Proposal | Status |
|---|---|
| [Resumable imports and the Activity list](resumable-import/README.md) | Proposed |

## Status

The first lines of a proposal say where it stands.

| Status | Meaning |
|---|---|
| Proposed | Written down and open for comment. Not scheduled. |
| Planned | The maintainers intend to build it as written. No date is implied. |
| In progress | Being built. The issue tracks what has landed. |
| Shipped | Released. The proposal stays as history, and the docs describe what moss does now. |
| Withdrawn | Not going ahead, with the reason. |

A proposal is not a promise. Designs change when they meet the code, and some are never built.

## What lives where

The CLI, the build engine and the packages are in this repository. The desktop app is built from a private one. A proposal may still show what the app would look like, because the people who use the app are the ones who can say whether a design is right. Each proposal marks which of its pieces live in this repository. Those are the pieces a pull request can pick up.

## Taking part

- To comment on a proposal, use its issue.
- To build a piece of one, say so in its issue first, so the approach can be agreed before you spend time on it.
- To suggest something new, open a feature request. If it grows into a design, it gets a folder here.
