# Editor interface plate research — 2026-09-18

The editor introduction uses the teaching pattern shared by three reference
families: show the whole system first, name its parts, then put a short action
beside each detail.

- The local *How to Keep Your Volkswagen Alive* mood board pairs a welcoming
  cover with a bird's-eye mechanical view and labelled leader lines. It lets a
  novice orient themselves before following a procedure.
- [The Universal Car Ford Manual (1919)](https://archive.org/details/1919_Ford_Model-T_Manual)
  places named illustrations beside short question-and-answer procedures.
- The University of Chicago's [Encyclopédie plate search](https://artfl-iiif.uchicago.edu/encyc_plate_search/)
  and MIT's [printing, papermaking, and mining exhibit](https://libraries.mit.edu/exhibits/diderots-encyclopedia-exhibit-preview/printing-papermaking-and-mining/)
  present complete practical systems alongside details of tools and operations.

The implemented plate therefore assembles two captured moss views: the editor
and the live preview shell. Four annotated crops identify choosing a page,
editing, previewing, and publishing. The SVG embeds the captured pixels so the
figure remains complete when browsers load it through a Markdown `<img>`.
Labels are annotations; the interface pixels come from the real
`ui/editor.html` and `ui/shell.html` surfaces captured at 1440 × 900 on
2026-09-18.
