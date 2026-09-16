# Canonical site example

A minimal moss site the agent can copy and adapt. Personalize the content;
keep the file-shape intact.

## On-disk shape

```
My Site/
  index.md
  About.md
  posts/
    posts.md
    My First Post.md
    A Second Post.md
```

No `.moss/` needed before the first build — moss creates it automatically.

## File contents

### `My Site/index.md`

```markdown
> The one-line summary of what this site is about.

Welcome. This is the home of My Site.
```

No `# Heading` — the **filename** (`My Site`) is the title. No `title:`
frontmatter — it would be redundant. The `>` blockquote immediately after the
implied title is the deck.

---

### `My Site/About.md`

```markdown
> A short author bio or mission statement.

Write the body here. Link to the home page with [[index]] if you need to.
Use `![[photo.jpg]]` for images — drop the file next to this document.
```

---

### `My Site/posts/posts.md`

```markdown
> Recent writing.
```

This file is the section home for `posts/`. Its filename matches the folder
name, so moss treats it as the folder's index. Add an intro deck; the rest of
the section is listed automatically.

---

### `My Site/posts/My First Post.md`

```markdown
---
date: 2026-01-15
---

> A one-sentence summary of the post.

The article body starts here. Use standard markdown — headings, lists, code
fences. Link to another post with [[A Second Post]]. Embed an image with
`![[banner.jpg]]`.
```

---

### `My Site/posts/A Second Post.md`

```markdown
---
date: 2026-01-20
---

> Another summary.

Body text.
```

## Rules this example demonstrates

- Filename = title. No body `# Heading`, no `title:` frontmatter.
- Deck = `> blockquote` at the top of the body.
- Folder home = file whose stem matches the folder (`posts/posts.md`).
- Images and links use wikilinks, not paths.
- No `.moss/` setup before first build — run `moss build "My Site"` and it
  appears.
- `date:` is the one frontmatter field a post usually needs. moss picks a
  folder's sort axis from its contents: any child carrying `weight:` switches
  the whole folder to manual order, otherwise it sorts newest-first only if
  most children are dated, and falls back to ordering by title — which reads
  as arbitrary for non-ASCII titles. Date a whole folder or none of it; a
  half-dated folder does not sort the way it looks like it should.

## Adapting it

1. Replace "My Site" with the actual site name (folder name = site title).
2. Add sections by creating subfolders; add a matching `<folder>.md` home
   file in each.
3. Add `.moss/theme/style.css` only when you need visual customization —
   not before.
4. Add `cover: banner.jpg` for a hero image; you do not need
   `children_style:` — moss picks the listing style from the content (summary
   when children have covers or decks, compact dated index otherwise), so set
   it only to override that choice.
