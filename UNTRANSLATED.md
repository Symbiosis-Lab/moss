# Untranslated pages

English docs pages that intentionally have **no zh-hans / zh-hant counterpart yet**.
Tracked here so the three language trees don't silently diverge (the structural-drift
convention). When a page below is translated, add the
zh files with the matching `translationKey` and remove it from this list.

## English-only reference (generated / contract pages)
- `site/docs/reference/css-tokens.md`
- `site/docs/reference/components.md`
- `site/docs/reference/html-structure.md`

## English-only authoring pages
- `site/docs/writing/editor.md` — the built-in moss editor
- `site/docs/writing/shortcodes/` — shortcode pages (index, buttons, fenced-divs, gallery, grid, hero)

## Notes
- The zh `design/` section deliberately keeps separate `css.md` + `javascript.md`
  pages (the English side consolidated them into one "Write a theme" guide). When the
  zh theme guide is written, collapse those to match.
- zh `reference/` leaf pages (cli, hooks, manifest, slots) are translations that may
  lag the English contract; each carries an "English is authoritative" note.
