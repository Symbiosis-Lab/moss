---
title: Callouts
uid: 8c2d41a7
weight: 5
description: Highlighted blockquote callouts for notes, warnings, tips, and more.
translationKey: docs-author-callouts
---

## Syntax

Callouts are blockquotes with a `[!type]` marker on the first line. moss turns them into styled panels you can target with CSS.

```markdown
> [!note]
> This is a note callout.

> [!warning] Careful
> This action cannot be undone.

> [!tip] Pro tip
> You can nest markdown inside callouts.
```

The word after `[!` sets the type. An optional title follows on the same line. When the title is omitted, moss uses the capitalized type name (e.g., `[!note]` → "Note").

Callouts can span multiple paragraphs. Use `>` on empty lines between paragraphs:

```markdown
> [!note] Long callout
> First paragraph.
>
> Second paragraph with **formatting**.
```

Markdown formatting inside callouts works as usual: bold, italics, links, lists, code, even nested callouts.

## Supported types

moss supports the Obsidian-compatible types:

`note`, `tip`, `warning`, `caution`, `important`, `info`, `abstract`, `todo`, `success`, `question`, `failure`, `danger`, `bug`, `example`, `quote`.

Unrecognized types leave the blockquote untouched (they render as a plain `<blockquote>`).

### Visual style

Each type gets the CSS class `.callout-<type>` alongside a shared `.callout` class:

```html
<div class="callout callout-warning">
  <div class="callout-title">Careful</div>
  <div class="callout-content">
    This action cannot be undone.
  </div>
</div>
```

Default styles ship with each theme. Override or extend them in `.moss/theme/style.css`:

```css
.callout-warning {
  border-left: 4px solid var(--moss-color-accent);
  background: rgba(255 200 0 / 0.08);
}
```

## Pending

The `pending` type marks placeholder content (sections you intend to fill in later).

```markdown
> [!pending] Trailer video
> Add when the cut is ready.
```

Style it with the `.callout-pending` hook (typical convention: dashed border, "PENDING · " prefix) to make unfinished sections visually obvious during authoring.

## Cross-references

- [Shortcodes](/docs/writing/shortcodes/): for layout blocks like grids and galleries
- [[Links & Embeds]]: for linking between pages and embedding sections
