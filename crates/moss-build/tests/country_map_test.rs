//! A page with `country_map:` draws its table as a map: matching, the problems
//! an author must see, the markup, and the embed.

#[path = "support/site.rs"]
mod site;

use site::Site;

const NAMES: [&str; 50] = [
    "Mexico", "Japan", "Chile", "Canada", "Brazil", "Norway", "France", "Germany", "Kenya", "Egypt", "India", "China", "Australia",
    "Peru", "Spain", "Italy", "Sweden", "Finland", "Poland", "Turkey", "Iran", "Iraq", "Nigeria", "Ghana", "Ethiopia", "Algeria",
    "Morocco", "Argentina", "Colombia", "Bolivia", "Cuba", "Haiti", "Thailand", "Vietnam", "Indonesia", "Philippines", "Mongolia",
    "Kazakhstan", "Ukraine", "Greece", "Portugal", "Ireland", "Iceland", "Zambia", "Angola", "Namibia", "Tanzania", "Uganda", "Nepal",
    "Laos",
];

fn page(rows: &str) -> String {
    format!(
        "---\ntitle: Risk levels\ndate: 2026-11-03\ncountry_map:\n  by: Level\n  levels:\n    - Low\n    - Guarded\n    - {{name: High, text: \"Avoid travel.\"}}\n  unrated: Not assessed.\n  action: {{label: \"Ask us\", link: \"/ask/?to={{country}}\"}}\n---\n| Country | Level | Note |\n|---|---|---|\n{rows}"
    )
}

fn fifty() -> String {
    let levels = ["Low", "Guarded", "High"];
    NAMES.iter().enumerate().map(|(i, n)| format!("| {n} | {} | Note {i} |\n", levels[i % 3])).collect()
}

fn problems(site: &Site) -> Vec<&String> {
    site.logged.iter().filter(|l| l.contains("[risk.md]")).collect()
}

fn built(rows: &str) -> Site {
    let _ = moss_build::build::cli_output::take_cli_problems();
    Site::new(&[("index.md", "---\ntitle: Home\n---\n\nHi.\n"), ("risk.md", &page(rows))])
}

#[test]
fn fifty_countries_build_without_a_problem_and_the_figure_legend_and_stamp_agree() {
    let site = built(&fifty());
    assert!(problems(&site).is_empty(), "{:?}", site.logged);
    assert_eq!(moss_build::build::cli_output::take_cli_problems(), 0);
    let html = site.html("risk/index.html");
    let coloured = html.matches("class=\"moss-country\" data-slug=").count();
    assert!(coloured > 200, "every country is a path, got {coloured}");
    assert_eq!(html.matches(" data-level=\"").count() - html.matches("moss-country-swatch").count(), 50, "50 coloured countries");
    assert!(html.contains("As of November 3, 2026</time>"), "the stamp shows the page's date");
    assert!(html.contains("datetime=\"2026-11-03\""));
    for level in ["Low", "Guarded", "High"] {
        assert_eq!(html.matches(&format!("</span>{level}</li>")).count(), 1, "legend names {level}");
    }
    assert!(html.contains("Note 0"), "the page's own table follows the map");
    assert!(!html.contains("moss-country-list"), "its own page draws no list");
    let land = html.split("class=\"moss-country\"").nth(1).unwrap();
    assert!(!land.split('>').next().unwrap().contains("stroke"), "no border on land");
}

#[test]
fn an_embed_draws_map_legend_stamp_and_rows_grouped_by_level_with_anchors() {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Home\n---\n\nHi.\n"),
        ("risk.md", &page("| Mexico | High | Night. |\n| Japan | Low | |\n| Chile | | |\n")),
        ("guide.md", "---\ntitle: Guide\n---\n\nBefore.\n\n![[risk|style:map]]\n\nAfter.\n"),
    ]);
    assert!(site.logged.iter().all(|l| !l.contains("risk.md")), "{:?}", site.logged);
    let html = site.html("guide/index.html");
    assert!(html.contains("moss-country-map") && html.contains("moss-country-legend") && html.contains("moss-country-stamp"));
    let list = html.split("moss-country-list").nth(1).expect("embedded list");
    let low = list.find("Low").unwrap();
    let high = list.find("High").unwrap();
    assert!(low < high, "lowest level first");
    assert!(list.contains("<li id=\"mexico\"><a href=\"#mexico\">Mexico</a>"));
    assert!(list.contains("Not rated") && list.contains("Chile"), "a blank cell is listed apart");
    let rated = html.split("<template data-slug=\"mexico\">").nth(1).and_then(|t| t.split("</template>").next()).expect("a panel template for Mexico");
    assert!(rated.contains("Night.") && rated.contains("href=\"/ask/?to=Mexico\""), "{rated}");
    let japan = html.split("<template data-slug=\"japan\">").nth(1).and_then(|t| t.split("</template>").next()).unwrap();
    assert!(japan.contains("Take normal precautions.") || japan.contains("Low"), "{japan}");
    let unrated = html.split("<template data-not-rated>").nth(1).and_then(|t| t.split("</template>").next()).expect("a not-rated template");
    assert!(unrated.contains("Not rated") && unrated.contains("Not assessed.") && unrated.contains("data-link=\"/ask/?to={country}\""), "{unrated}");
}

#[test]
fn a_misspelt_name_is_a_problem_naming_page_row_and_value_and_counts_for_strict() {
    let site = built("| Mexco | High | |\n| Japan | Low | |\n");
    let found = problems(&site);
    assert_eq!(found.len(), 1, "{:?}", site.logged);
    for part in ["risk.md", "row 1", "'Mexco'"] {
        assert!(found[0].contains(part), "{part} missing from {}", found[0]);
    }
    assert!(moss_build::build::cli_output::take_cli_problems() >= 1, "counts toward --strict");
}

#[test]
fn a_value_outside_levels_a_duplicate_and_disputed_and_antarctic_names_each_name_their_row() {
    let site = built("| Mexico | Severe | |\n| Japan | Low | |\n| japan | High | |\n| Somaliland | Low | |\n| Antarctica | Low | |\n");
    let all = problems(&site).iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n");
    for part in ["row 1: 'Severe' for Mexico", "row 3: 'japan' is listed twice", "row 4: 'Somaliland' is disputed", "row 5: Antarctica"] {
        assert!(all.contains(part), "{part} missing from:\n{all}");
    }
    assert!(moss_build::build::cli_output::take_cli_problems() >= 4);
}

#[test]
fn natural_earth_is_credited_in_a_notice_file_not_on_the_page() {
    let site = built(&fifty());
    let html = site.html("risk/index.html");
    assert!(!html.contains("Natural Earth"), "no credit line under the map");
    let moss = site.output("_moss");
    let dir = std::fs::read_dir(&moss).unwrap().map(|e| e.unwrap().path()).find(|p| p.join("NOTICE.txt").is_file()).expect("a NOTICE.txt beside the map");
    let notice = std::fs::read_to_string(dir.join("NOTICE.txt")).unwrap();
    assert!(notice.contains("Natural Earth") && notice.contains("public domain"), "{notice}");
    assert!(!notice.contains("GEBCO"), "a country map ships no elevation data");
}

#[test]
fn each_country_carries_the_box_the_map_frames_with_outlying_islands_left_out() {
    let site = built(&fifty());
    let html = site.html("risk/index.html");
    let box_of = |slug: &str| -> Vec<f64> {
        let tag = html.split(&format!("data-slug=\"{slug}\"")).nth(1).unwrap().split('>').next().unwrap();
        tag.split("data-box=\"").nth(1).unwrap().split('"').next().unwrap().split(' ').map(|n| n.parse().unwrap()).collect()
    };
    let chile = box_of("chile");
    assert_eq!(chile.len(), 4);
    assert!(chile[2] - chile[0] < 60.0, "Easter Island does not widen Chile's frame: {chile:?}");
    assert!(chile[3] - chile[1] > 100.0, "the mainland is long: {chile:?}");
    let japan = box_of("japan");
    assert!(japan[0] < japan[2] && japan[1] < japan[3]);
}

#[test]
fn a_rated_country_carries_its_level_index_for_the_fade_in_delay() {
    let site = built(&fifty());
    let html = site.html("risk/index.html");
    assert_eq!(html.matches("style=\"--i:").count(), 50, "one per rated country, none on unrated land");
    let tag = |slug: &str| html.split(&format!("data-slug=\"{slug}\"")).nth(1).unwrap().split('>').next().unwrap().to_string();
    assert!(tag("mexico").contains("data-level=\"1\" style=\"--i:0\""), "{}", tag("mexico"));
    assert!(tag("japan").contains("style=\"--i:1\""));
    assert!(!tag("chad").contains("--i:"), "Chad has no row");
}
