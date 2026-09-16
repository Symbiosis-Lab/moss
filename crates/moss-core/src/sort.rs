//! Folder-listing sort: types and inference cascade.
//!
//! Pure Rust, zero I/O. Consumed by:
//!   - the build pipeline (scan pass, card renderer, series-nav)
//!   - the editor form (to show "inferred: date" next to undeclared sort:)
//!
//! See docs/archive/2026-05-17-listing-sort-and-embeds-design.md.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "lowercase")]
pub enum SortAxis {
    Date,
    Weight,
    Title,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(untagged)]
pub enum SortField {
    Axis(SortAxis),
    List(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ResolvedSort {
    pub axis: SortAxis,
    pub explicit_order: Option<Vec<String>>,
    /// Default value of `series:` chrome. True iff axis == Weight OR explicit_order is Some.
    pub series_default: bool,
}

impl ResolvedSort {
    /// Axis driving card *presentation* (the meta slot), as opposed to
    /// ordering. An explicit-order listing is a curated sequence, not a
    /// chronological feed, so its cards present like a Weight listing —
    /// no per-card date — even when the ordering axis is Date.
    pub fn presentation_axis(&self) -> SortAxis {
        if self.explicit_order.is_some() {
            SortAxis::Weight
        } else {
            self.axis
        }
    }
}

/// Minimal document trait for sort inference. Both src-tauri's
/// ParsedDocument and the editor's in-memory document model implement this.
pub trait SortableDoc {
    fn url_path(&self) -> &str;
    fn date(&self) -> Option<&str>;
    fn weight(&self) -> Option<i32>;
    fn declared_sort(&self) -> Option<&SortField>;
    fn clean_stem(&self) -> &str;
    /// Whether this doc is a folder-index page. moss uses pretty URLs
    /// (`<stem>/index.html`) for both articles and subfolder indexes, so
    /// the URL pattern alone can't tell them apart. Implementors that
    /// distinguish via metadata (e.g. `kind == Folder`) should override.
    /// Default returns false — safe for callers that only ever pass
    /// articles in.
    fn is_folder_index(&self) -> bool {
        false
    }
}

const DATE_FRACTION_THRESHOLD: f32 = 0.8;

pub fn resolve_folder_sort<D: SortableDoc>(
    folder: &D,
    children: &[&D],
) -> ResolvedSort {
    // Exclude subfolder indexes via the kind-aware trait method.
    // The legacy URL-pattern filter (`!url.ends_with("/index.html")`) is
    // wrong under pretty URLs — every article also ends with
    // `<stem>/index.html` — so we delegate to the impl. The default
    // `is_folder_index() == false` keeps moss-core's existing single-file
    // tests (`a.url = "a.html"`) green; src-tauri's `ParsedDocument`
    // returns true when `kind == Folder`.
    let article_children: Vec<&&D> = children
        .iter()
        .filter(|c| !c.is_folder_index())
        .collect();

    let (axis, explicit_order) = match folder.declared_sort() {
        Some(SortField::Axis(a)) => (*a, None),
        Some(SortField::List(items)) => {
            // Entries may be written as Obsidian `[[Wikilinks]]`, quoted refs, or
            // paths (`travel/foo.md`). Normalize each to the bare filename stem so
            // it matches `clean_stem()` at sort time — otherwise the explicit
            // order is silently ignored and children fall back to the axis sort.
            let stems = items
                .iter()
                .map(|s| crate::frontmatter_typed::frontmatter_ref_to_stem(s))
                .collect();
            (infer_axis(&article_children), Some(stems))
        }
        None => (infer_axis(&article_children), None),
    };

    let series_default = matches!(axis, SortAxis::Weight) || explicit_order.is_some();

    ResolvedSort { axis, explicit_order, series_default }
}

fn infer_axis<D: SortableDoc>(article_children: &[&&D]) -> SortAxis {
    if article_children.is_empty() {
        return SortAxis::Title;
    }
    if article_children.iter().any(|c| c.weight().is_some()) {
        return SortAxis::Weight;
    }
    let total = article_children.len() as f32;
    let dated = article_children.iter().filter(|c| c.date().is_some()).count() as f32;
    if dated / total >= DATE_FRACTION_THRESHOLD {
        return SortAxis::Date;
    }
    SortAxis::Title
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_field_parses_axis_strings() {
        assert!(matches!(serde_yaml::from_str::<SortField>("date").unwrap(), SortField::Axis(SortAxis::Date)));
        assert!(matches!(serde_yaml::from_str::<SortField>("weight").unwrap(), SortField::Axis(SortAxis::Weight)));
        assert!(matches!(serde_yaml::from_str::<SortField>("title").unwrap(), SortField::Axis(SortAxis::Title)));
    }

    #[test]
    fn sort_field_parses_list() {
        let f: SortField = serde_yaml::from_str("[intro, setup, advanced]").unwrap();
        match f {
            SortField::List(items) => assert_eq!(items, vec!["intro", "setup", "advanced"]),
            _ => panic!("expected List"),
        }
    }

    #[test]
    fn sort_field_rejects_unknown_axis() {
        assert!(serde_yaml::from_str::<SortField>("random").is_err());
    }
}

#[cfg(test)]
mod inference_tests {
    use super::*;

    #[derive(Debug, Default)]
    pub(super) struct TestDoc {
        url: String,
        stem: String,
        date_v: Option<String>,
        weight_v: Option<i32>,
        sort_v: Option<SortField>,
    }
    impl SortableDoc for TestDoc {
        fn url_path(&self) -> &str { &self.url }
        fn date(&self) -> Option<&str> { self.date_v.as_deref() }
        fn weight(&self) -> Option<i32> { self.weight_v }
        fn declared_sort(&self) -> Option<&SortField> { self.sort_v.as_ref() }
        fn clean_stem(&self) -> &str { &self.stem }
    }
    pub(super) fn art(stem: &str, date: Option<&str>, w: Option<i32>) -> TestDoc {
        TestDoc {
            url: format!("{}.html", stem),
            stem: stem.into(),
            date_v: date.map(|s| s.into()),
            weight_v: w,
            sort_v: None,
        }
    }
    pub(super) fn folder(url: &str, sort: Option<SortField>) -> TestDoc {
        TestDoc { url: url.into(), sort_v: sort, ..Default::default() }
    }

    #[test]
    fn explicit_axis_wins() {
        let f = folder("blog/index.html", Some(SortField::Axis(SortAxis::Title)));
        let a = art("a", Some("2025-01-01"), None);
        let r = resolve_folder_sort(&f, &[&a]);
        assert_eq!(r.axis, SortAxis::Title);
        assert!(r.explicit_order.is_none());
        assert!(!r.series_default);
    }

    #[test]
    fn explicit_list_implies_chrome_on() {
        let f = folder("blog/index.html", Some(SortField::List(vec!["a".into(), "b".into()])));
        let a = art("a", None, None);
        let b = art("b", None, None);
        let r = resolve_folder_sort(&f, &[&a, &b]);
        assert!(r.series_default, "explicit list-form sort implies series chrome on");
        assert_eq!(r.explicit_order.as_ref().unwrap().len(), 2);
        assert_eq!(r.axis, SortAxis::Title, "tail axis inferred from undated children");
    }

    #[test]
    fn explicit_list_refs_normalized_to_stems() {
        // Entries written as `[[Wikilinks]]`, quoted paths, or bare names must
        // all collapse to the filename stem in `explicit_order`, so they match
        // `clean_stem()` when `sort_by_resolved` partitions listed children.
        let f = folder(
            "blog/index.html",
            Some(SortField::List(vec![
                "[[gamma]]".into(),
                "posts/alpha.md".into(),
                "beta".into(),
            ])),
        );
        let a = art("alpha", None, None);
        let b = art("beta", None, None);
        let g = art("gamma", None, None);
        let r = resolve_folder_sort(&f, &[&a, &b, &g]);
        assert_eq!(
            r.explicit_order.as_deref(),
            Some(["gamma".to_string(), "alpha".to_string(), "beta".to_string()].as_slice()),
            "wikilink/path/bare refs must all normalize to bare stems"
        );
    }

    #[test]
    fn weight_present_infers_weight() {
        let f = folder("docs/index.html", None);
        let a = art("intro", None, Some(10));
        let b = art("advanced", None, Some(20));
        let r = resolve_folder_sort(&f, &[&a, &b]);
        assert_eq!(r.axis, SortAxis::Weight);
        assert!(r.series_default, "weight axis implies series chrome on");
    }

    #[test]
    fn dates_above_threshold_infer_date() {
        let f = folder("blog/index.html", None);
        let a = art("a", Some("2025-01-01"), None);
        let b = art("b", Some("2025-02-01"), None);
        let c = art("c", Some("2025-03-01"), None);
        let d = art("d", Some("2025-04-01"), None);
        let e = art("e", None, None);
        let r = resolve_folder_sort(&f, &[&a, &b, &c, &d, &e]);
        assert_eq!(r.axis, SortAxis::Date);  // 4/5 = 0.8 == threshold
        assert!(!r.series_default);
    }

    #[test]
    fn dates_below_threshold_fallback_to_title() {
        let f = folder("projects/index.html", None);
        let a = art("a", Some("2025-01-01"), None);
        let b = art("b", None, None);
        let c = art("c", None, None);
        let d = art("d", None, None);
        let e = art("e", None, None);
        let r = resolve_folder_sort(&f, &[&a, &b, &c, &d, &e]);
        assert_eq!(r.axis, SortAxis::Title);  // 1/5 = 0.2 < 0.8
    }

    #[test]
    fn weight_beats_date() {
        let f = folder("hybrid/index.html", None);
        let a = art("a", Some("2025-01-01"), Some(1));
        let b = art("b", Some("2025-02-01"), None);
        let r = resolve_folder_sort(&f, &[&a, &b]);
        assert_eq!(r.axis, SortAxis::Weight);
    }

    #[test]
    fn subfolders_excluded_from_inference() {
        let f = folder("root/index.html", None);
        let article = art("welcome", None, None);
        let sub_a = folder("root/news/index.html", None);
        let sub_b = folder("root/projects/index.html", None);
        let r = resolve_folder_sort(&f, &[&article, &sub_a, &sub_b]);
        assert_eq!(r.axis, SortAxis::Title);
    }

    #[test]
    fn chps_style_root_with_only_subfolders_falls_to_title() {
        let f = folder("root/index.html", None);
        let sub_a = folder("root/news/index.html", None);
        let sub_b = folder("root/projects/index.html", None);
        let r = resolve_folder_sort(&f, &[&sub_a, &sub_b]);
        assert_eq!(r.axis, SortAxis::Title);
    }

    #[test]
    fn empty_folder_defaults_to_title() {
        let f = folder("empty/index.html", None);
        let r = resolve_folder_sort::<TestDoc>(&f, &[]);
        assert_eq!(r.axis, SortAxis::Title);
    }
}

/// Optional supplementary trait for label-based sorting.
/// Implementations that want Title-axis support implement both
/// SortableDoc and SortableLabel.
pub trait SortableLabel {
    fn label(&self) -> &str;
}

/// The one comparator for ordering two user-visible listing labels by title.
///
/// All four alphabetical orderings of user-visible labels route here: the
/// `SortAxis::Title` arm below, the dateless tiebreak in each of
/// `folder_embed::generate_children`'s two branches, and the same-date
/// tiebreak in `year_group`. (The `SortAxis::Weight` arm below does not — it
/// breaks ties on `clean_stem`, a filename, which is not a label.)
///
/// Case is a tiebreak, not a primary key. Comparing codepoints put `mao`
/// after every capitalised name on the reference vault's roster, which is a
/// machine's order, not a reader's; folding first sorts it between `Kayla`
/// and `Scarly`. The unfolded comparison still runs second, so the order
/// stays total and stable for labels differing only in case.
///
/// Two things a reader of Chinese expects are still missing, and both need
/// something this function does not have. `黃` sorts after `馬` by codepoint,
/// where a Traditional-Chinese site expects 姓氏筆畫 (surname stroke count) —
/// that needs either an `icu_collator` dependency (not in the tree; only
/// `icu_normalizer`/`icu_properties` are, transitively) or an embedded Unihan
/// `kTotalStrokes` table. And bucketing Han ahead of Latin is only right on a
/// site whose resident script is Han, so it needs the site language, which is
/// not a parameter here. Both ship together as their own reviewed change;
/// this function exists so that change lands in one place.
/// Provenance: docs/archive/2026-09-06-authors-index-design-decision.md
pub fn cmp_labels(a: &str, b: &str) -> std::cmp::Ordering {
    fn folded(s: &str) -> impl Iterator<Item = char> + '_ {
        s.chars().flat_map(char::to_lowercase)
    }
    folded(a).cmp(folded(b)).then_with(|| a.cmp(b))
}

pub fn sort_by_resolved<'a, D>(
    docs: &[&'a D],
    resolved: &ResolvedSort,
) -> Vec<&'a D>
where
    D: SortableDoc + SortableLabel,
{
    let axis_cmp = |a: &&'a D, b: &&'a D| -> std::cmp::Ordering {
        match resolved.axis {
            SortAxis::Date => {
                let ad = a.date().unwrap_or("");
                let bd = b.date().unwrap_or("");
                bd.cmp(ad)
            }
            SortAxis::Weight => match (a.weight(), b.weight()) {
                (Some(aw), Some(bw)) => aw.cmp(&bw),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a.clean_stem().cmp(b.clean_stem()),
            },
            SortAxis::Title => cmp_labels(a.label(), b.label()),
        }
    };

    match &resolved.explicit_order {
        Some(order) => {
            let order_lower: Vec<String> = order.iter().map(|s| s.to_lowercase()).collect();
            let order_map: std::collections::HashMap<&str, usize> = order_lower
                .iter()
                .enumerate()
                .map(|(i, s)| (s.as_str(), i))
                .collect();
            let (mut listed, mut unlisted): (Vec<_>, Vec<_>) = docs.iter().copied().partition(|d| {
                order_map.contains_key(d.clean_stem().to_lowercase().as_str())
            });
            listed.sort_by(|a, b| {
                let ai = order_map.get(a.clean_stem().to_lowercase().as_str()).copied().unwrap_or(usize::MAX);
                let bi = order_map.get(b.clean_stem().to_lowercase().as_str()).copied().unwrap_or(usize::MAX);
                ai.cmp(&bi)
            });
            unlisted.sort_by(axis_cmp);
            listed.extend(unlisted);
            listed
        }
        None => {
            let mut sorted: Vec<&'a D> = docs.to_vec();
            sorted.sort_by(axis_cmp);
            sorted
        }
    }
}

#[cfg(test)]
mod sort_dispatch_tests {
    use super::*;
    use super::inference_tests::*;  // reuse TestDoc

    fn doc_with_label(stem: &str, date: Option<&str>, w: Option<i32>, label: &str) -> TestDocWithLabel {
        TestDocWithLabel {
            base: art(stem, date, w),
            label_v: label.into(),
        }
    }

    #[derive(Debug)]
    struct TestDocWithLabel {
        base: TestDoc,
        label_v: String,
    }
    impl SortableDoc for TestDocWithLabel {
        fn url_path(&self) -> &str { self.base.url_path() }
        fn date(&self) -> Option<&str> { self.base.date() }
        fn weight(&self) -> Option<i32> { self.base.weight() }
        fn declared_sort(&self) -> Option<&SortField> { self.base.declared_sort() }
        fn clean_stem(&self) -> &str { self.base.clean_stem() }
    }
    impl SortableLabel for TestDocWithLabel {
        fn label(&self) -> &str { &self.label_v }
    }

    #[test]
    fn date_desc() {
        let a = doc_with_label("a", Some("2025-01-01"), None, "A");
        let b = doc_with_label("b", Some("2025-03-01"), None, "B");
        let c = doc_with_label("c", Some("2025-02-01"), None, "C");
        let r = ResolvedSort { axis: SortAxis::Date, explicit_order: None, series_default: false };
        let sorted = sort_by_resolved(&[&a, &b, &c], &r);
        assert_eq!(sorted[0].clean_stem(), "b");
        assert_eq!(sorted[2].clean_stem(), "a");
    }

    #[test]
    fn weight_asc_unweighted_last() {
        let a = doc_with_label("a", None, Some(2), "A");
        let b = doc_with_label("b", None, None, "B");
        let c = doc_with_label("c", None, Some(1), "C");
        let r = ResolvedSort { axis: SortAxis::Weight, explicit_order: None, series_default: true };
        let sorted = sort_by_resolved(&[&a, &b, &c], &r);
        assert_eq!(sorted[0].clean_stem(), "c");
        assert_eq!(sorted[1].clean_stem(), "a");
        assert_eq!(sorted[2].clean_stem(), "b");
    }

    #[test]
    fn title_alpha() {
        let a = doc_with_label("zebra", None, None, "Zebra");
        let b = doc_with_label("apple", None, None, "Apple");
        let c = doc_with_label("mango", None, None, "Mango");
        let r = ResolvedSort { axis: SortAxis::Title, explicit_order: None, series_default: false };
        let sorted = sort_by_resolved(&[&a, &b, &c], &r);
        assert_eq!(sorted[0].clean_stem(), "apple");
        assert_eq!(sorted[2].clean_stem(), "zebra");
    }

    #[test]
    fn explicit_wikilink_order_normalized_and_beats_date_axis() {
        // Folder declares an explicit order using `[[Wikilinks]]`; every child is
        // dated, so the axis infers Date. The bracketed refs must normalize to
        // stems, match the children, and the explicit order must win over
        // date-descending. Regression guard for the two-part collection-order fix.
        let f = TestDocWithLabel {
            base: folder(
                "blog/index.html",
                Some(SortField::List(vec![
                    "[[gamma]]".into(),
                    "[[alpha]]".into(),
                    "[[beta]]".into(),
                ])),
            ),
            label_v: "Blog".into(),
        };
        let alpha = doc_with_label("alpha", Some("2025-01-01"), None, "Alpha");
        let beta = doc_with_label("beta", Some("2025-03-01"), None, "Beta");
        let gamma = doc_with_label("gamma", Some("2025-02-01"), None, "Gamma");

        let r = resolve_folder_sort(&f, &[&alpha, &beta, &gamma]);
        assert_eq!(r.axis, SortAxis::Date, "all children dated => Date axis inferred");

        let sorted = sort_by_resolved(&[&alpha, &beta, &gamma], &r);
        let order: Vec<&str> = sorted.iter().map(|d| d.clean_stem()).collect();
        assert_eq!(
            order,
            vec!["gamma", "alpha", "beta"],
            "explicit [[wikilink]] order must beat date-desc (beta, gamma, alpha) after stem normalization"
        );
    }

    #[test]
    fn explicit_list_with_tail() {
        let a = doc_with_label("a", Some("2025-03-01"), None, "A");
        let b = doc_with_label("b", Some("2025-02-01"), None, "B");
        let intro = doc_with_label("intro", Some("2025-01-01"), None, "Intro");
        let r = ResolvedSort {
            axis: SortAxis::Date,
            explicit_order: Some(vec!["intro".into()]),
            series_default: true,
        };
        let sorted = sort_by_resolved(&[&a, &b, &intro], &r);
        assert_eq!(sorted[0].clean_stem(), "intro");  // listed first
        assert_eq!(sorted[1].clean_stem(), "a");      // newest in tail
        assert_eq!(sorted[2].clean_stem(), "b");
    }
}

#[cfg(test)]
mod cmp_labels_tests {
    use super::cmp_labels;
    use std::cmp::Ordering;

    /// The reference vault's roster: codepoint order exiled the one lowercase
    /// name past every capitalised one.
    #[test]
    fn a_lowercase_name_sorts_among_its_peers() {
        let mut names = vec!["Scarly", "mao", "Kayla"];
        names.sort_by(|a, b| cmp_labels(a, b));
        assert_eq!(names, vec!["Kayla", "mao", "Scarly"]);
    }

    /// Case is a tiebreak, not a primary key — so the order stays total and
    /// two labels differing only in case never compare Equal.
    #[test]
    fn case_only_differences_stay_ordered_and_never_equal() {
        assert_eq!(cmp_labels("Ada", "ada"), Ordering::Less);
        assert_eq!(cmp_labels("ada", "Ada"), Ordering::Greater);
        assert_eq!(cmp_labels("Ada", "Ada"), Ordering::Equal);
    }

    /// Han is still codepoint-ordered. Pinned so the pending 姓氏筆畫 change
    /// has to come here and say so, rather than landing beside a test that
    /// never noticed. The two surnames disagree: 于 is 3 strokes and 丘 is 5,
    /// so 姓氏筆畫 puts 于 first, and U+4E18 < U+4E8E puts 丘 first.
    #[test]
    fn han_is_not_yet_collated_by_stroke_count() {
        assert_eq!(cmp_labels("丘", "于"), Ordering::Less);
    }
}
