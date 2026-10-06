# Contributing to moss docs

## sc-demo consistency check

Shortcode documentation pages use `:::grid 2 {.sc-demo}` blocks that show Markdown source on the left and a live rendered shortcode on the right. The left and right cells must stay in sync.

A pre-commit hook runs automatically when you commit changes under `site/docs/writing/shortcodes/`. You can also run it manually:

```bash
bash scripts/check-sc-demos.sh
```

If a block fails, edit the left cell's ` ```markdown ` code block to match the right cell's shortcode content.

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
