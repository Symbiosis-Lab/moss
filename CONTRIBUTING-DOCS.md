# Contributing to moss docs

## sc-demo consistency check

Shortcode documentation pages use `:::grid 2 {.sc-demo}` blocks that show Markdown source on the left and a live rendered shortcode on the right. The left and right cells must stay in sync.

A pre-commit hook runs automatically when you commit changes under `site/docs/writing/shortcodes/`. You can also run it manually:

```bash
bash scripts/check-sc-demos.sh
```

If a block fails, edit the left cell's ` ```markdown ` code block to match the right cell's shortcode content.

---

## Walkthrough videos

The Get Started and Privacy pages show short screen recordings where they used to show gifs. To replace one, record the screen, then:

```bash
scripts/encode-walkthrough.sh "/path/to/new folder.mov"
```

It writes `site/assets/animations/new folder.mp4` at the size and frame rate the build publishes untouched; the script's header says why those numbers. Keep the recording's name, so every page that embeds it picks up the new file. A silent walkthrough is embedded as a captioned loop, `![[new folder.mp4|loop|100%|What it shows]]`; a video with sound keeps its controls and can carry a caption, `![[Intro to moss.mp4|100%|Caption]]` (pass `--audio` to the script).

After a build, `node scripts/check-docs-media.mjs` fails if any video on those pages is drawn wider than its own picture, or in a box of a different shape.

---

# Editing docs with CriticMarkup

We use [CriticMarkup](https://criticmarkup.com) to annotate documentation. It's the standard for editorial markup in Markdown.

## Syntax

| Markup | Purpose | Example |
|--------|---------|---------|
| `{++ text ++}` | Addition | `{++ new sentence ++}` |
| `{-- text --}` | Deletion | `{-- remove this --}` |
| `{~~ old ~> new ~~}` | Substitution | `{~~ deploy ~> publish ~~}` |
| `{== text ==}` | Highlight | `{== important phrase ==}` |
| `{>> text <<}` | Comment | `{>> why this matters <<}` |

Highlight and comment are often combined: `{== highlighted ==}{>> reason <<}`

## Workflow

1. Commit the current docs as a baseline
2. Edit files using CriticMarkup — mark what changes and why
3. Commit the annotated version
4. Ask Claude Code to process: accept edits, propagate to other languages, strip markup

## Example

```markdown
## {~~ Deploying ~> Publishing ~~} your site

{>> "deploy" is too technical for our audience. Use 发布 in Chinese. <<}

{-- You need to configure your deployment target before proceeding. --}

{++ moss makes publishing simple — connect your account and click publish. ++}
```
