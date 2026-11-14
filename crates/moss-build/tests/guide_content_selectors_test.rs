//! The agent guide teaches theme selectors that find content by the markup
//! moss writes for it ("Style by what the content is" in `authoring.md`).
//! This builds a site holding each shape the guide names and runs every taught
//! selector on every page, so a change to that markup fails here instead of
//! leaving the guide wrong.

#[path = "support/site.rs"]
mod site;

use scraper::{Html, Selector};
use site::Site;

const GUIDE: &str = include_str!("../src/assets/skills/moss/references/authoring.md");

const HOME: &str = "index.html";
const SHAPES: &str = "shapes/index.html";
const PEOPLE: &str = "people/index.html";
const PAGES: [&str; 3] = [HOME, SHAPES, PEOPLE];

struct Pin {
    /// The selector this one is read inside, where the guide names a part of a block.
    within: &'static str,
    selector: &'static str,
    /// False where the guide describes the shape in words and prints no selector.
    printed: bool,
    /// The pages it matches on, or `&[]` for every page; it matches nothing on the rest.
    pages: &'static [&'static str],
    /// One entry per element it matches, in document order: text only that element holds.
    matches: &'static [&'static str],
}

const fn pin(selector: &'static str, pages: &'static [&'static str], matches: &'static [&'static str]) -> Pin {
    Pin { within: "", selector, printed: true, pages, matches }
}

const fn part(within: &'static str, selector: &'static str, pages: &'static [&'static str], matches: &'static [&'static str]) -> Pin {
    Pin { within, selector, printed: true, pages, matches }
}

const fn described(selector: &'static str, pages: &'static [&'static str], matches: &'static [&'static str]) -> Pin {
    Pin { within: "", selector, printed: false, pages, matches }
}

const CARD: &str = ".moss-grid > a.moss-card";
const THREE_PAGES: &[&str] = &["Harbour", "Lantern", "Orchard"];

const PINS: &[Pin] = &[
    // A folded callout; the callout that is not folded is a `div` and stays out.
    pin("details.callout", &[SHAPES], &["A word from the keeper", "Background"]),
    pin(r#"details.callout[data-type="quote"]"#, &[SHAPES], &["A word from the keeper"]),
    part("details.callout", "summary.callout-title", &[SHAPES], &["A word from the keeper", "Background"]),
    part("details.callout", ".callout-content", &[SHAPES], &["opens at nine", "Built in 1900"]),
    // A grid cell holding only `[[Page]]`; the home page lists the same pages outside a grid.
    pin(CARD, &[SHAPES], THREE_PAGES),
    part(CARD, ".moss-card-cover", &[SHAPES], &["/work/one.png", "/work/two.png", "/work/three.png"]),
    part(CARD, ".moss-card-title", &[SHAPES], THREE_PAGES),
    part(CARD, ".moss-card-meta", &[SHAPES], &["2026-01-02", "2026-01-02", "2026-01-02"]),
    part(CARD, ".moss-card-description", &[SHAPES], &["Harbour in one line", "Lantern in one line", "Orchard in one line"]),
    // The grid of linked pictures, and neither the page cards nor the grid of plain cells.
    pin(r#".moss-grid:has(> a.moss-grid-card[data-kind="link"])"#, &[SHAPES], &["/quay.png"]),
    pin("body > footer > p:has(+ .moss-subscribe)", &[], &["a room by the harbour"]),
    pin("body > footer > .moss-grid", &[], &["Prices"]),
    pin("body > footer > .moss-grid + p", &[], &["mailto:mail@example.org"]),
    // Two links on consecutive lines; the paragraph holding one link stays out.
    pin("p.moss-links:has(> br)", &[SHAPES], &["Book the hall"]),
    // The people page opens with a hero too.
    pin(r#"body[data-page="home"] > .moss-hero"#, &[HOME], &["A hall for hire"]),
    pin(r#"body[data-page="home"] > header"#, &[HOME], &["main-nav"]),
    // Matches the jury as well as the credits, which is why the guide asks for
    // a count; the list with no bold and the one under an h3 stay out.
    pin("article h2 + ul:has(> li > strong:first-child)", &[PEOPLE], &["Lighting", "Robin Example"]),
    // A picture and then two lines, or a heading. Only a picture with no caption
    // is a `p > picture` (`![alt|](x.png)`); one with a caption is a `figure`, so
    // the same cell written with one stays out, as does a picture over one line.
    described(
        ".moss-grid-card:has(> p:first-child > picture:only-child):has(> p:nth-child(2):last-child > br)",
        &[PEOPLE],
        &["/alex.png"],
    ),
    described(".moss-grid-card:has(> p:first-child > picture:only-child):has(> h3:nth-child(2))", &[PEOPLE], &["/sam.png"]),
];

/// 1x1 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49,
    0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0xF0, 0x1F, 0x00, 0x05, 0x00, 0x01, 0xFF, 0x89, 0x99, 0x3D,
    0x1D, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

const PICTURES: &[&str] = &[
    "hall.png", "quay.png", "pier.png", "mole.png", "alex.png", "sam.png", "robin.png", "kim.png",
    "work/one.png", "work/two.png", "work/three.png",
];

const HOME_MD: &str = "---
title: Example Hall
---

:::hero {image=hall.png}
## A hall for hire
:::

Welcome.
";

const SHAPES_MD: &str = "---
title: Shapes
---

> [!quote]- A word from the keeper
> The hall opens at nine.

> [!note]- Background
> Built in 1900.

> [!note] Always open
> Not folded.

:::grid 3
[[Harbour]]
+++
[[Lantern]]
+++
[[Orchard]]
:::

:::grid 3
[![[quay.png|The quay]]](/work/harbour/)
+++
[![[pier.png|The pier]]](/work/lantern/)
+++
[![[mole.png|The mole]]](/work/orchard/)
:::

:::grid 2
The hall seats ninety.
+++
The gallery seats thirty.
:::

[Book the hall](/work/harbour/)
[See the rooms](/work/lantern/)

[Write to us](/work/orchard/)
";

const PEOPLE_MD: &str = "---
title: People
---

:::hero {image=hall.png}
## Who runs the hall
:::

## The Winter Play

- **Director** Alex Example
- **Lighting** Sam Sample

## Jury

- **Robin Example**, chair
- **Kim Sample**

## Opening hours

- Weekdays from nine
- Saturdays from ten

### Thanks

- **Printing** Example Press

:::grid 2
![Alex Example|](alex.png)

Alex Example
Director
+++
![Sam Sample|](sam.png)

### Sam Sample
+++
![[robin.png|Robin Example, chair]]

Robin Example
Chair
+++
![Kim Sample|](kim.png)

Jury.
:::
";

const FOOTER_MD: &str = "Example Hall, a room by the harbour.

:::subscribe
:::

:::grid 3
- [Rooms](/work/harbour/)
- [Prices](/work/lantern/)
+++
- [Shapes](/shapes/)
- [People](/people/)
+++
- [Work](/work/)
:::

[mail@example.org](mailto:mail@example.org) © Example
";

fn work(title: &str, cover: &str) -> String {
    format!("---\ntitle: {title}\ndate: 2026-01-02\ncover: {cover}\ndescription: {title} in one line.\n---\n\nBody.\n")
}

fn built_pages() -> Vec<(&'static str, Html)> {
    let site = Site::new(&[
        ("index.md", HOME_MD),
        ("shapes.md", SHAPES_MD),
        ("people.md", PEOPLE_MD),
        ("footer.md", FOOTER_MD),
        ("work/index.md", "---\ntitle: Work\n---\n"),
        ("work/harbour.md", &work("Harbour", "one.png")),
        ("work/lantern.md", &work("Lantern", "two.png")),
        ("work/orchard.md", &work("Orchard", "three.png")),
    ]);
    for name in PICTURES {
        std::fs::write(site.root.join(name), PNG).unwrap();
    }
    site.rebuild();
    PAGES.iter().map(|page| (*page, Html::parse_document(&site.html(page)))).collect()
}

#[test]
fn each_selector_the_guide_teaches_matches_only_the_content_it_names() {
    let pages = built_pages();
    let mut wrong = Vec::new();
    for pin in PINS {
        let full = format!("{} {}", pin.within, pin.selector);
        let selector = Selector::parse(full.trim()).unwrap_or_else(|e| panic!("{full}: {e}"));
        for (page, html) in &pages {
            let expected = if pin.pages.is_empty() || pin.pages.contains(page) { pin.matches } else { &[] };
            let found: Vec<String> = html.select(&selector).map(|el| el.html()).collect();
            let same = found.len() == expected.len() && found.iter().zip(expected).all(|(el, text)| el.contains(text));
            if !same {
                let opening: Vec<&str> = found.iter().map(|el| el.split_inclusive('>').next().unwrap_or("")).collect();
                wrong.push(format!("`{}` on {page}: expected {} match(es) holding {expected:?}, found {}: {opening:?}", full.trim(), expected.len(), found.len()));
            }
        }
    }
    assert!(wrong.is_empty(), "the guide's selectors and the built markup disagree:\n{}", wrong.join("\n"));
}

#[test]
fn the_guide_prints_each_pinned_selector() {
    let missing: Vec<&str> = PINS
        .iter()
        .filter(|pin| pin.printed)
        .flat_map(|pin| [pin.within, pin.selector])
        .filter(|selector| !selector.is_empty() && !GUIDE.contains(&format!("`{selector}`")))
        .collect();
    assert!(missing.is_empty(), "pinned here and not printed in authoring.md: {missing:?}");
}
