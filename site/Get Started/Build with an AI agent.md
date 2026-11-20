---
title: Build with an AI agent
url: agents
uid: 98565f4a
weight: 65
description: Give a coding agent your site folder and one prompt. moss gives it a manual that matches your moss, and the folder stays yours.
translationKey: docs-agents
---

Give your AI agent a folder and `moss guide`, and it can help you build and publish a website.

Open the folder in your agent and paste:

```text
Install moss if it is missing:
`npm install -g @symbiosis-lab/moss`
Read moss guide and moss describe --json. Help me make
a website in this folder. Ask what the site is for and
what content I want, then create plain Markdown pages
and keep styling in .moss/theme/. Follow the guide for
any settings. Run moss build . --strict and fix any
problems. Run moss build . --serve and give me the
preview URL it prints so I can review the site. When I
am ready to publish, help me use moss deploy . following
the guide. Ask me for the site name and email if needed,
and let me complete verification before publishing.
```

You get a folder of editable Markdown pages and a built website. To preview it again, run `moss build . --serve` in that folder and open the URL it prints; press Ctrl-C to stop.

[Download moss](/#downloads)

## Why keep your content apart

moss builds a folder of pages and pictures. The code is moss and your theme. An agent could write the whole site as code instead; keeping the two apart means:

- **Your words survive a redesign.** Pages are plain files. A new theme never touches them, and a new moss changes them only when you run `moss migrate`.
- **The agent changes one thing at a time.** It restyles in `.moss/theme/` without touching a sentence, and edits a page without touching the style.
- **The code does not rot.** moss updates the build. When a release retires a class your theme uses, the build names what to use now (coming: `moss migrate` rewrites it for you).
- **Your pages move.** They are markdown files: they open in any editor, live in git, and other markdown tools can read them.
- **The agent knows where it may write.** Pages and theme are yours; the rest of `.moss/` is moss's.

## `moss agents init`

In your site folder, run:

```
moss agents init
```

It writes:

- `.claude/skills/moss/SKILL.md`, which Claude Code reads on its own, when Claude Code is installed;
- `.cursor/rules/moss.mdc`, which Cursor reads on its own, when Cursor is installed;
- `.moss/agents/SKILL.md`, for any other agent;
- `.moss/AGENTS.md`, your site's own conventions, for you or the agent to fill in.

Each of the first three files points the agent at `moss guide` and nothing more. A build keeps them up to date once they exist, and never creates them. `moss agents sync` refreshes them without touching `.moss/AGENTS.md`.

## The agent's manual: `moss guide`

`moss guide` prints moss's instructions for agents: how a project is laid out, how to style a site, the rules to keep, and how to check a change worked. `moss guide --list` names every topic; `moss guide all` prints them all.

The manual is built into the moss you have installed, so it always describes that moss, never an older or newer one, and it needs no network. `moss describe --json` lists every class, token, front-matter field and command by name, for an agent that needs the exact spelling.

## Where the agent may write

- Pages, pictures and folders: anywhere outside `.moss/`.
- The theme: `.moss/theme/`.
- Settings: `.moss/config.toml`.
- Your site's agent notes: `.moss/AGENTS.md`.

Everything else under `.moss/` is moss's: the built site, the version history, the publish record and the site's keys. Each changes only through a moss command, such as `moss history` or `moss deploy`.

Coming: `moss describe --json` will list each of these paths, with the command that changes it, and `moss agents init` will add a three-line `AGENTS.md` at the top of the folder for Codex, Gemini CLI, GitHub Copilot and other tools that read one.

## When moss changes

When a release renames or retires a class or token your theme uses, the build names it, the release it changed in, and what to use now.

`moss migrate` rewrites the folder for you. Without `--apply` it prints the change as a diff and writes nothing. With `--apply` it first saves a version of the site, then writes. To undo it, run `moss history --restore --at <version> --yes` with the version it names, or revert the commit if the folder is in git. Today it can:

- remove a `translationKey` the page's address already implies;
- move a fact written as a classed block on many pages into a front-matter field (`--lift`);
- move an imported page's original address from `syndicated:` to `origin:` (`--import-sources`).

Coming: every rename will ship with its own rewrite. `moss migrate --list` will show what in the folder is out of date, the build will end with one line when there is something to update, and `moss migrate` will rewrite renamed theme classes, tokens and old spellings in pages and settings.

Ask the agent to run `moss migrate`, read the diff, then run it with `--apply` and build. In a git folder, commit the migration on its own.

## Review what the agent did

The agent's changes are files in your folder. Look at the preview, and at `git diff` if the folder is in git. Before a large change, save a version with `moss history --save "before the redesign"`; `moss history` lists the versions and `moss history --restore --at <version> --yes` brings one back.
