# Facts, cascades and groups: design without template code

**Status:** In progress
**Issue:** to be opened with the `proposal` label
**Pieces in this repository:** all of them (moss-core, moss-build, the guide `moss guide` prints)

## The idea

A fact is a field. moss places it. A folder can give all its pages the same fact. Several facts can be placed together as one.

Every kind of setting already has one home in moss, and this proposal keeps it that way. It adds no template language and no new markdown grammar. It fills the gaps builds keep running into, each with a form other tools already read:

| Home | Holds | Where authors and coding agents know it from |
|---|---|---|
| A page's markdown | prose, and pointers with `![[…]]` | wiki links in Obsidian and other note tools |
| A page's frontmatter, a folder's `cascade:`, `.moss/config.toml` | what the page's facts are, and where moss places each declared field | Obsidian properties, Astro collection schemas, Hugo's `cascade:` |
| The theme in `.moss/theme/` | how a place looks | CSS |

## Why

Site builds keep asking for the same few things, and each time the alternative was theme code, raw HTML or the same value typed by hand on many pages:

- a crop that keeps a picture's subject, or a picture shown whole instead of cropped;
- a heading a link can point at by a stable name;
- one page showing a fact that another page holds (the next production's date and place on the home page);
- a value every page in a folder repeats (a credit line on each of 55 prints);
- a line composed from several fields (a credit from title, date, holder and licence).

A general template engine would let a site do all of these, and it would also let a site drop what moss's renderer guarantees: landmarks, headings, navigation, dark mode, accessibility fixes, the archival copy's edition and citation. Its inputs would become a public contract that every later feature owes a variable to. So moss does not get one. Measured against real builds, none of these needs logic the author writes.

## The changes

### 1. Document the image display words moss already reads

An image embed already takes display words, parsed once for every media context: `![[eagles.jpg|focus:50%,20%]]` keeps that point of the picture in view when it is cropped, and `![[leaf.jpg|contain]]` shows the whole picture instead of cropping it (`cover`, `fill`, `scale-down` and named positions such as `top` or `bottom left` are read too). Today they reach `object-position` and `object-fit` for a lone image embed, a hero, a page cover, a grid or link card's cover and a folder listing's cover. The guide never mentions them, so builders wrote per-image CSS or gave up on the crop.

The change documents them, lists them in `moss describe --json`, and makes the picture cells of a gallery and a collage honour them too, since that is where a crop most often loses a face.

Two traps the guide must state. A word moss does not know becomes the image's caption and alt text, because the pipe also carries a caption (`![[x.jpg|contian]]` is captioned "contian"). And a display word mixed with anything else (`contain color=black`) makes the whole pipe a caption. The build warns only where a typo is unambiguous: a `focus:` that does not parse, or a word one letter away from a display word.

### 2. A heading can name its anchor: `## Opening hours {#hours}`

This is the one attribute form every widely used markdown tool with attributes reads the same way: Pandoc, kramdown, Python-Markdown, markdown-it-attrs, and Hugo with it on by default. moss reads an `{#id}` at the end of an ATX heading as the heading's anchor id and drops it from the heading's text. Classes and key-value pairs on a heading stay unsupported and keep their warning; the guide keeps pointing to the fenced div for styling.

A section embed, `![[Page#Opening hours]]` or `![[Page#hours]]`, finds a heading whose id was named by either its words or its id. A link to a named heading uses the id, `[[Page#hours]]`: a link resolves its fragment from the words written in it, without reading the other page's headings, so the words reach only a heading whose id was not named.

**Compatibility.** Today such a heading prints the braces in its text and in its generated id (`opening-hours-{hours}`), and the build has warned about it since it was released. After the change the braces leave the text and the id is the one written. One more case follows from the same parser: a section embed by a heading's words now also finds a heading that carries `{contents=end}`, which its braces used to hide. These are the only differences this proposal allows in an existing page's output. The old id is not kept: moss called it wrong when it was generated, and its braces make it an unlikely link target.

### 3. Deferred: a field as something an embed can point at

`![[page#name]]` would draw a declared field of another page when the page has no heading or block of that name, with headings keeping precedence and an unmatched name behaving as today. It was built and reviewed and is held back. Its one use case, a home page showing the next production's date and place, is served by a hero drawn from a listing (`:::hero {from="work/|when:upcoming"}`), and what remains is a new seam into the embed resolver with no case of its own. If a case that names one page appears, the shape to take is one selector with two consumers: a page target is either a page's name or the first page of a listing written exactly as `from=` writes it, a hero draws that page, and a pointer draws one of its fields.

### 4. `cascade:` carries declared fields

A folder's home page can already push page keys to every page beneath it with `cascade:`. It now also carries values for declared fields, so a value every page in the folder repeats is written once:

```yaml
cascade:
  credit_holder: "Example Museum of Prints"
  licence: "CC0"
```

A page's own value wins, as with every cascaded key. A cascaded value is checked exactly as if the page had written it: its type, and the field's `in` scope. A name that is neither a cascade key nor a declared field keeps its warning.

**Compatibility.** A declared field name in `cascade:` is ignored with a warning today; after the change it is carried. Nothing else moves.

### 5. A group places several fields as one

A declaration can name other fields instead of holding a value of its own:

```toml
[fields.credit]
type  = "group"
of    = ["title", "date", "credit_holder", "licence"]
place = "subtitle"
```

moss draws the fields that have a value, in that order, as one `moss-` component, each part keeping its own `data-field` and the theme drawing the separator between parts, as the eyebrow already keeps several fields as separate words. A part with no value is left out with its separator, so there is nothing to test for presence. A group has no value of its own to write, cascade or point at, except as a whole by its own name.

**Compatibility.** New declarations only; a site without a group renders as today.

## Deliberately not adopted

- **Attributes on images, links and inline spans** (`![a](b){.x}`, `[text]{.label}`). GitHub and Obsidian print them as literal braces. A recurring inline label is a field, and its look is the field's placement.
- **Format strings and templates in config.** A group composes fields without one.
- **Theme views and a link template.** A view (a logic-free file in the theme filling one placement) and a URL template filled from fields were considered. Each has one build asking for it so far, and in that build the links sit inside a list on one page rather than on pages of their own, so neither would serve it. They wait for a second case.
- **Loops and conditionals.** A list of pages is a listing; a list built from a page's own sections is a moss component when it recurs.
- **Values derived from a page's position** ("Station 2 of 53"). The station number is a fact the series writes; a position count would print the wrong number where the series skips or repeats.

## Use cases

From recent site builds, grouped by what serves them:

| Need | Served by |
|---|---|
| Keep a picture's subject in a crop; show a page or album leaf whole | image display words (1) |
| A stable anchor for a section link | heading `{#id}` (2) |
| The home page shows the next production's date and place from that production's page | a hero drawn from a listing (`from=`), already in moss |
| A credit line or licence repeated on every page of a folder | `cascade:` with declared fields (4) |
| A credit from title, date, holder and licence | a group (5) |
| A label inside a catalogue entry; a chip above a title | declared fields and their placements, as today |

## How compatibility is checked

Every step builds the maintained sites with the released moss and with the candidate and compares them page by page. The only difference allowed is a heading that carried an attribute block, which loses its literal braces and takes the id written.
