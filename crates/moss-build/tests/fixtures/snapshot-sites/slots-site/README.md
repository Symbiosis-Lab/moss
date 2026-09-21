# slots-site

Covers: docs/extend/slots.md

Exercises the slot authoring surface documented in `extend/slots.md`.
Slots are named HTML comment markers (`<!-- slot:head-end -->`, `<!-- slot:after-title -->`, etc.) that moss writes into generated HTML at six fixed positions; plugins fill them during the enhance phase, and any unfilled markers are stripped before the final output is written.

**Note:** Slot rendering requires plugins. This fixture is built with
`--no-plugins`, so `expected/` captures the no-provider baseline (what
slots look like when nothing fills them): all six slot markers are absent
from the output HTML.

See `basic-site/README.md` for the regeneration procedure.
