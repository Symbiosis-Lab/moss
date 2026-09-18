# Interface illustration research and reproduction notes

Date: 2026-09-18

The prototypes adapt the information structure of historical technical illustration without imitating engraving texture. The visual system uses moss’s warm paper, dark ink, green accent, serif headings, restrained rules, and real product captures.

## Sources and useful methods

The 1919 Ford manual establishes the whole object before numbered component explanations and keeps labels secondary to the machine. Source: [Ford Model T manual, Project Gutenberg](https://www.gutenberg.org/files/46206/46206-h/46206-h.htm).

John Muir’s *How to Keep Your Volkswagen Alive* demonstrates friendly hierarchy and a reader-first sequence for intimidating technical work. The prototypes borrow its plain-language pacing and strong title scale, not its illustrations. Source: [Hachette Book Group](https://www.hachette.co.uk/titles/peter-aschwanden/how-to-keep-your-volkswagen-alive/9781566913102/).

Diderot’s plates separate a complete view from details so readers can orient themselves before inspecting a part. The interface plate applies that whole-to-detail structure to a software surface. Source: [MIT Libraries, *Diderot’s Encyclopédie*](https://libraries.mit.edu/exhibits/diderots-encyclopedia-exhibit-preview/printing-papermaking-and-mining/).

The mood board photographs in `moss-desktop/docs/reference/design/mood board/` supplied composition references: a full-object plate with an aligned key, and an emphatic manual cover with a clear reading order. They are research references only and are not shipped.

## Product source material

Both studies use current moss interface captures already stored in the repository: `site/assets/guides/editor-ui-source.png`, `site/assets/guides/shell-ui-source.png`, and `site/assets/guides/choose-page-source.png`. The choose-page image was captured from `site/ui/editor.html` at 1440 × 900 after opening its native file-tree `<details>` element, then cropped to 520 × 520 without repainting the interface. The labels describe visible controls and states. The step tabs navigate the explanation; they do not simulate or invent moss controls.

## Reproduction

Run `cargo run -p moss-cli -- build site --serve --watch` from the repository root, use the printed preview URL, then open `/illustrations/`, `/illustrations/interface-plate.html`, and `/illustrations/publish-procedure.html`. Documentation images remain in moss’s media index rather than passthrough directories, allowing Markdown links and covers to resolve. The preview checker validates the generated image URLs and MIME types; the only remaining build diagnostic is the unpublished-site live-record warning. Verify keyboard focus and Arrow key navigation on the procedure, test a narrow viewport, and print or emulate print media to confirm every step is visible without JavaScript interaction.
