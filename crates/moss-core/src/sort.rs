//! Folder-listing sort: types and inference cascade.
//!
//! Pure Rust, zero I/O. Consumed by:
//!   - the build pipeline (scan pass, card renderer, series-nav)
//!   - the editor form (to show "inferred: date" next to undeclared sort:)

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "lowercase")]
pub enum SortAxis {
    /// Newest first — the default, and the only direction before `DateAsc`
    /// existed.
    Date,
    /// Oldest first. A second, sibling axis rather than a flag on `Date`: a
    /// reader picks `sort:` from one flat list of tokens, and `date-asc`
    /// reads the same way `date`/`weight`/`title` already do. Presents
    /// identically to `Date` everywhere but the comparator's direction — see
    /// [`SortAxis::shows_date`].
    #[serde(rename = "date-asc")]
    DateAsc,
    Weight,
    Title,
}

impl SortAxis {
    /// True for either date axis — `Date` and `DateAsc` present identically
    /// (a compact date in the card meta slot, auto-year-grouping), and only
    /// [`cmp_date_axis`]'s direction tells them apart. Callers deciding
    /// "does this listing show a date" call this instead of comparing to
    /// `Date` alone, so `DateAsc` is never silently treated like `Weight`/`Title`.
    pub fn shows_date(&self) -> bool {
        matches!(self, Self::Date | Self::DateAsc)
    }
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

/// Minimal document trait for sort inference. Both moss-build's
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

    /// The name an explicit `sort: [a, b, c]` list matches this child
    /// against. Defaults to `clean_stem()`, correct for a leaf — its own
    /// filename is exactly what an author types to name it.
    ///
    /// A folder index is the case this default gets wrong: its home file is
    /// very often literally `index.md` (the generic convention, not a
    /// self-named file), so its `clean_stem()` is the fixed string
    /// `"index"` — never what an author would write to name the folder
    /// itself. `render/html.rs`'s own "every folder index's clean_stem is
    /// 'index'" comment already routes around this same fact for sibling
    /// identity; an explicit-order list must do the same, or a folder named
    /// in the list silently falls through to the inferred axis instead of
    /// keeping its declared position — invisible in a flat folder (an
    /// article's `clean_stem` already matches), and only visible once the
    /// matching child is itself a folder.
    ///
    /// An implementor whose `url_path()` can name the folder some other way
    /// should override this; the default keeps today's behavior.
    fn order_match_name(&self) -> &str {
        self.clean_stem()
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
    // tests (`a.url = "a.html"`) green; moss-build's `ParsedDocument`
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
        assert!(matches!(serde_yaml::from_str::<SortField>("date-asc").unwrap(), SortField::Axis(SortAxis::DateAsc)));
        assert!(matches!(serde_yaml::from_str::<SortField>("weight").unwrap(), SortField::Axis(SortAxis::Weight)));
        assert!(matches!(serde_yaml::from_str::<SortField>("title").unwrap(), SortField::Axis(SortAxis::Title)));
    }

    #[test]
    fn shows_date_is_true_for_both_date_axes_only() {
        assert!(SortAxis::Date.shows_date());
        assert!(SortAxis::DateAsc.shows_date());
        assert!(!SortAxis::Weight.shows_date());
        assert!(!SortAxis::Title.shows_date());
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
    fn root_with_only_subfolders_falls_to_title() {
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
/// All three alphabetical orderings of user-visible labels route here: the
/// `SortAxis::Title` arm below, the undated tiebreak in [`cmp_date_axis`]
/// (which folder listings share), and the tiebreak between undated rows in
/// `year_group`. (The `SortAxis::Weight` arm below does not — it breaks ties
/// on `clean_stem`, a filename, which is not a label.)
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
pub fn cmp_labels(a: &str, b: &str) -> std::cmp::Ordering {
    fn folded(s: &str) -> impl Iterator<Item = char> + '_ {
        s.chars().flat_map(char::to_lowercase)
    }
    folded(a).cmp(folded(b)).then_with(|| a.cmp(b))
}

/// What the date axis reads off one listing entry.
pub struct DateSortKey<'a> {
    pub date: Option<&'a str>,
    pub is_folder: bool,
    pub label: &'a str,
    pub url_path: &'a str,
}

/// The one date-axis order. A folder's listing and its series chain both
/// sort through here, so a reader walking the chain meets the pages in the
/// order the folder's page lists them.
///
/// Newest first when `ascending` is false (the `Date` axis), oldest first
/// when true (`DateAsc`); dated entries always sort before undated ones,
/// regardless of direction — a chronology oldest-first still doesn't want
/// its undated stragglers leading. Among undated entries folders come first,
/// then titles in [`cmp_labels`] order, the same in both directions. The url
/// path, unique per page, settles anything still tied — otherwise a tie
/// keeps the order pages were read from disk, which changes between builds
/// and platforms.
pub fn cmp_date_axis(a: &DateSortKey<'_>, b: &DateSortKey<'_>, ascending: bool) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a.date, b.date) {
        (Some(ad), Some(bd)) => if ascending { ad.cmp(bd) } else { bd.cmp(ad) },
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => b.is_folder.cmp(&a.is_folder).then_with(|| cmp_labels(a.label, b.label)),
    }
    .then_with(|| a.url_path.cmp(b.url_path))
}

pub fn sort_by_resolved<'a, D>(
    docs: &[&'a D],
    resolved: &ResolvedSort,
) -> Vec<&'a D>
where
    D: SortableDoc + SortableLabel,
{
    let date_key = |d: &'a D| DateSortKey {
        date: d.date(),
        is_folder: d.is_folder_index(),
        label: d.label(),
        url_path: d.url_path(),
    };
    let axis_cmp = |a: &&'a D, b: &&'a D| -> std::cmp::Ordering {
        match resolved.axis {
            SortAxis::Date => cmp_date_axis(&date_key(a), &date_key(b), false),
            SortAxis::DateAsc => cmp_date_axis(&date_key(a), &date_key(b), true),
            SortAxis::Weight => match (a.weight(), b.weight()) {
                (Some(aw), Some(bw)) => aw.cmp(&bw),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a.clean_stem().cmp(b.clean_stem()),
            },
            SortAxis::Title => cmp_labels(a.label(), b.label()),
        }
        // Same settlement on every axis: two pages with one weight or one
        // title fall back to their url path, as tied dates do above.
        .then_with(|| a.url_path().cmp(b.url_path()))
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
                order_map.contains_key(d.order_match_name().to_lowercase().as_str())
            });
            listed.sort_by(|a, b| {
                let ai = order_map.get(a.order_match_name().to_lowercase().as_str()).copied().unwrap_or(usize::MAX);
                let bi = order_map.get(b.order_match_name().to_lowercase().as_str()).copied().unwrap_or(usize::MAX);
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

    /// `sort: date-asc` — a chronology wanted oldest-first (a site publishing
    /// a sequence of lectures by year, say) rather than the newest-first
    /// default.
    #[test]
    fn date_asc_orders_oldest_first() {
        let a = doc_with_label("a", Some("2025-01-01"), None, "A");
        let b = doc_with_label("b", Some("2025-03-01"), None, "B");
        let c = doc_with_label("c", Some("2025-02-01"), None, "C");
        let r = ResolvedSort { axis: SortAxis::DateAsc, explicit_order: None, series_default: false };
        let sorted = sort_by_resolved(&[&a, &b, &c], &r);
        assert_eq!(sorted[0].clean_stem(), "a");
        assert_eq!(sorted[1].clean_stem(), "c");
        assert_eq!(sorted[2].clean_stem(), "b");
    }

    /// Undated entries still trail every dated one under `date-asc`, same as
    /// `date` — ascending reverses which dated entry leads, not whether an
    /// undated one does.
    #[test]
    fn date_asc_still_keeps_undated_entries_last() {
        let dated = doc_with_label("dated", Some("2025-01-01"), None, "Dated");
        let undated = doc_with_label("undated", None, None, "Undated");
        let r = ResolvedSort { axis: SortAxis::DateAsc, explicit_order: None, series_default: false };
        let sorted = sort_by_resolved(&[&undated, &dated], &r);
        assert_eq!(sorted[0].clean_stem(), "dated");
        assert_eq!(sorted[1].clean_stem(), "undated");
    }

    /// Pages on the same date come out in one order however they arrive:
    /// by url path, the same order the folder's own listing uses. A tie used
    /// to keep the order the pages were read in, so a series whose chapters
    /// share a date read 4, 1, 2, 3 on one build and 4, 3, 2, 1 on another.
    #[test]
    fn date_ties_order_by_url_path_whatever_the_input_order() {
        let c1 = doc_with_label("chapter-1", Some("1804"), None, "Chapter 1");
        let c2 = doc_with_label("chapter-2", Some("1804"), None, "Chapter 2");
        let c3 = doc_with_label("chapter-3", Some("1804"), None, "Chapter 3");
        let c4 = doc_with_label("chapter-4", Some("1804"), None, "Chapter 4");
        let later = doc_with_label("epilogue", Some("1805"), None, "Epilogue");
        let r = ResolvedSort { axis: SortAxis::Date, explicit_order: None, series_default: true };
        let order = |docs: &[&TestDocWithLabel]| -> Vec<String> {
            sort_by_resolved(docs, &r).iter().map(|d| d.clean_stem().to_string()).collect()
        };
        let expected = vec!["epilogue", "chapter-1", "chapter-2", "chapter-3", "chapter-4"];
        assert_eq!(order(&[&c4, &c1, &later, &c2, &c3]), expected);
        assert_eq!(order(&[&c4, &c3, &c2, &later, &c1]), expected);
    }

    /// The same holds for two chapters given the same weight, or two pages
    /// with the same title in a title-sorted folder.
    #[test]
    fn weight_and_title_ties_order_by_url_path_whatever_the_input_order() {
        let a = doc_with_label("a", None, Some(1), "Same");
        let b = doc_with_label("b", None, Some(1), "Same");
        for axis in [SortAxis::Weight, SortAxis::Title] {
            let r = ResolvedSort { axis, explicit_order: None, series_default: true };
            let order = |docs: &[&TestDocWithLabel]| -> Vec<String> {
                sort_by_resolved(docs, &r).iter().map(|d| d.clean_stem().to_string()).collect()
            };
            assert_eq!(order(&[&b, &a]), vec!["a", "b"], "{axis:?}");
            assert_eq!(order(&[&a, &b]), vec!["a", "b"], "{axis:?}");
        }
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

    /// A doc that can stand in for either a leaf (`order_match_name` equal to
    /// `clean_stem`, the default) or a folder whose home file is the generic
    /// `index.md` (`clean_stem` is "index", `order_match_name` is the
    /// folder's real name) — the same split `ParsedDocument`'s own override
    /// makes in moss-build, reproduced minimally here so `sort_by_resolved`'s
    /// explicit-order match is proven to read `order_match_name`, not
    /// `clean_stem`, at the trait level.
    struct NamedDoc {
        base: TestDocWithLabel,
        order_name: &'static str,
    }
    impl SortableDoc for NamedDoc {
        fn url_path(&self) -> &str { self.base.url_path() }
        fn date(&self) -> Option<&str> { self.base.date() }
        fn weight(&self) -> Option<i32> { self.base.weight() }
        fn declared_sort(&self) -> Option<&SortField> { self.base.declared_sort() }
        fn clean_stem(&self) -> &str { self.base.clean_stem() }
        fn order_match_name(&self) -> &str { self.order_name }
    }
    impl SortableLabel for NamedDoc {
        fn label(&self) -> &str { self.base.label() }
    }

    #[test]
    fn explicit_order_matches_by_order_match_name_not_clean_stem() {
        let intro = NamedDoc { base: doc_with_label("intro", None, None, "Intro"), order_name: "intro" };
        // Stands in for a subfolder named "appendix" whose home file is the
        // generic `index.md`: clean_stem is "index", but order_match_name —
        // what `ParsedDocument` derives from the URL, and what the author
        // wrote in `sort:` — is "appendix".
        let appendix = NamedDoc { base: doc_with_label("index", None, None, "Appendix"), order_name: "appendix" };
        let r = ResolvedSort {
            axis: SortAxis::Title,
            explicit_order: Some(vec!["appendix".into(), "intro".into()]),
            series_default: true,
        };

        let sorted = sort_by_resolved(&[&appendix, &intro], &r);
        assert_eq!(
            sorted.iter().map(|d| d.order_match_name()).collect::<Vec<_>>(),
            vec!["appendix", "intro"],
            "appendix must keep its declared first position, matched by order_match_name"
        );
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
