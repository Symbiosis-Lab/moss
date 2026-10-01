//! `labels.json`: the places-explorer's label data (named cities, mountain
//! ranges, peaks and rivers), for the site's own published language(s), in
//! the SAME `_moss/map.<hash>/` directory `emit::place_map_assets` writes
//! its world/tile SVGs into.
//!
//! Data only. Nothing here places a label on a rendered map — this emitter
//! only picks which of the pack's two label languages (`en`, `zh-Hant`) the
//! site gets and writes them out; the places-explorer's own `labels.ts`
//! (`crates/moss-build/src/js-src/site/places-explorer/`) reads this file
//! client-side to actually place one.
//!
//! Computing this file is cheap — a few hundred small records read straight
//! out of the already-decoded, in-memory [`place_map::Labels`], no
//! rendering — so, like `place_map_assets::emit`'s own `tiles.json` index,
//! it is simply written every build rather than threaded through the
//! transform cache: nothing here is worth caching on its own. It still
//! lands in the SAME content-addressed directory as the cached SVGs, and
//! the mark-and-sweep stage write below is what keeps it from costing
//! anything on a build where nothing changed.

use std::collections::BTreeMap;
use std::path::Path;

use crate::build::context::BuildContext;
use crate::build::manifest::{HashBucket, PendingManifest};
use crate::build::place_map::{self, PointLabel, RiverLabel};
use crate::build::render::lang_roots::site_lang_roots;
use crate::build::served_path::ServedPath;
use crate::build::types::ParsedDocument;
use crate::i18n::Language;

/// The pack's own coordinate quantisation (thousandths of a degree) —
/// matches `scripts/place-map/generate.mjs`'s `QUANT` and
/// `place_map::Header::quantisation` for the rest of the pack.
const QUANT: f64 = 1000.0;

fn dequantize(value: i32) -> f64 {
    f64::from(value) / QUANT
}

/// A label's name in one language, falling back to `en` when the `zh-Hant`
/// field is empty — Natural Earth does not carry a Traditional-Chinese name
/// for every minor feature, and an empty label is never useful on a map.
fn display_name<'a>(name_en: &'a str, name_zht: &'a str, tag: &str) -> &'a str {
    if tag == "zh-Hant" && !name_zht.is_empty() {
        name_zht
    } else {
        name_en
    }
}

#[derive(serde::Serialize)]
struct PointLabelJson {
    name: String,
    lat: f64,
    lng: f64,
    rank: i16,
}

impl PointLabelJson {
    fn from_label(label: &PointLabel, tag: &str) -> Self {
        Self {
            name: display_name(&label.name_en, &label.name_zht, tag).to_string(),
            lat: dequantize(label.lat),
            lng: dequantize(label.lng),
            rank: label.rank,
        }
    }
}

#[derive(serde::Serialize)]
struct RiverLabelJson {
    name: String,
    rank: i16,
    /// `[lng, lat]` pairs along the river's course, in order — not closed,
    /// since a river is an open line.
    line: Vec<[f64; 2]>,
}

impl RiverLabelJson {
    fn from_label(label: &RiverLabel, tag: &str) -> Self {
        Self {
            name: display_name(&label.name_en, &label.name_zht, tag).to_string(),
            rank: label.rank,
            line: label.line.iter().map(|&(x, y)| [dequantize(x), dequantize(y)]).collect(),
        }
    }
}

#[derive(serde::Serialize)]
struct LanguageLabels {
    cities: Vec<PointLabelJson>,
    ranges: Vec<PointLabelJson>,
    peaks: Vec<PointLabelJson>,
    rivers: Vec<RiverLabelJson>,
}

impl LanguageLabels {
    fn from_labels(labels: &place_map::Labels, tag: &str) -> Self {
        Self {
            cities: labels.cities.iter().map(|l| PointLabelJson::from_label(l, tag)).collect(),
            ranges: labels.ranges.iter().map(|l| PointLabelJson::from_label(l, tag)).collect(),
            peaks: labels.peaks.iter().map(|l| PointLabelJson::from_label(l, tag)).collect(),
            rivers: labels.rivers.iter().map(|l| RiverLabelJson::from_label(l, tag)).collect(),
        }
    }
}

#[derive(serde::Serialize)]
struct LabelsJson {
    /// Every language key present below, in the same order they were
    /// inserted into `by_language` (a `BTreeMap`, so always `en` before
    /// `zh-Hant` — the pack's two label languages sort that way already).
    languages: Vec<&'static str>,
    #[serde(flatten)]
    by_language: BTreeMap<&'static str, LanguageLabels>,
}

/// Which of the pack's two label languages (`en`, `zh-Hant`) a site gets,
/// from the SAME site-language signal the nav language switcher reads
/// (`build::render::lang_roots::site_lang_roots` — see that function's own
/// doc for how moss identifies a language-root homepage). A site whose
/// pages are `zh-Hant` gets `zh-Hant` alone, not `en` too; an `en` site
/// gets `en` alone; a bilingual `en`+`zh-Hant` site gets both. A site in a
/// third language (bare `zh-Hans`, or anything else moss's three-language
/// UI does not name) has no matching label language of its own, so it
/// falls back to `en` — the pack carries no Simplified-Chinese names to
/// offer instead, and `en` is the one language every label record always
/// has.
fn label_language_tags(lang_roots: &[crate::i18n::site_languages::LangRoot]) -> Vec<&'static str> {
    let has = |want: Language| {
        lang_roots
            .iter()
            .any(|root| Language::from_bcp47_lenient(&root.lang_tag) == want)
    };
    let mut tags = Vec::new();
    if has(Language::En) || has(Language::ZhHans) {
        tags.push("en");
    }
    if has(Language::ZhHant) {
        tags.push("zh-Hant");
    }
    if tags.is_empty() {
        tags.push("en");
    }
    tags
}

fn labels_json(labels: &place_map::Labels, tags: &[&'static str]) -> LabelsJson {
    let by_language = tags
        .iter()
        .map(|&tag| (tag, LanguageLabels::from_labels(labels, tag)))
        .collect();
    LabelsJson { languages: tags.to_vec(), by_language }
}

/// `pipeline.rs`'s one call site, matching `place_map_assets::
/// emit_if_place_typed`'s own gate: `None` when the site declares no
/// place-typed kind (or the bundled pack failed to decode, already logged
/// when `render_context` was built), and `map_assets_hash` empty when
/// `place_map_assets`'s own directory hash has not been computed this
/// build either — both cases emit nothing rather than invent a directory
/// name of their own.
pub fn emit_if_place_typed(
    render_context: Option<&place_map::PlaceMapRenderContext>,
    site_lang: &str,
    documents: &[ParsedDocument],
    output_dir: &Path,
    pending: &mut PendingManifest,
) {
    let Some(render_context) = render_context else {
        return;
    };
    let hash = render_context.map_assets_hash();
    if hash.is_empty() {
        return;
    }
    let labels = &render_context.maps().pack().labels;
    if let Err(error) = emit(labels, hash, site_lang, documents, output_dir, pending) {
        crate::build::cli_output::log_warn_problem!("place-map labels could not be emitted: {error}");
    }
}

/// Emit `labels.json` under `_moss/map.<hash>/` — the SAME `hash`
/// `place_map_assets::assets_hash` computed for the world/tile SVGs in this
/// same directory, passed in rather than recomputed so the two can never
/// disagree.
pub fn emit(
    labels: &place_map::Labels,
    hash: &str,
    site_lang: &str,
    documents: &[ParsedDocument],
    output_dir: &Path,
    pending: &mut PendingManifest,
) -> Result<(), String> {
    let lang = Language::from_code(site_lang).unwrap_or(Language::En);
    let lang_roots = site_lang_roots(documents, lang);
    let tags = label_language_tags(&lang_roots);
    let body = labels_json(labels, &tags);
    let json = serde_json::to_vec(&body).map_err(|e| format!("Failed to serialize place-map labels: {e}"))?;
    let served = ServedPath::for_place_map_asset(hash, "labels.json")
        .map_err(|e| format!("Invalid place-map labels path: {e}"))?;
    BuildContext::for_render(output_dir, pending)
        .emit(&served, &json, HashBucket::Files)
        .map_err(|e| format!("Failed to emit place-map labels: {e}"))
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;
    use crate::i18n::site_languages::LangRoot;
    use crate::types::content::SiteHashes;

    /// `labels.json`'s worst case is a bilingual site, the only one that
    /// gets BOTH language keys at once (see [`label_language_tags`]).
    /// Measured on the real embedded pack: 519,485 raw bytes, 66,840 bytes
    /// brotli q11 (the same quality `scripts/place-map/generate.mjs`
    /// compresses the pack itself at). This is a fixed cost, not a
    /// per-site one -- every site with a place-typed kind gets the SAME
    /// label set, filtered only by which language(s) it asked for -- so
    /// unlike `places_data.rs`'s own budget test this isn't guarding
    /// against a site's own content growing unboundedly; it is a tripwire
    /// against a future label-set change (a lower rank cutoff, say)
    /// ballooning `labels.json` without anyone noticing.
    #[test]
    fn a_bilingual_sites_labels_json_stays_well_under_its_brotli_budget() {
        const BROTLI_Q11_BUDGET: usize = 100_000;
        let context = place_map::PlaceMapContext::embedded().unwrap();
        let body = labels_json(&context.pack().labels, &["en", "zh-Hant"]);
        let json = serde_json::to_vec(&body).unwrap();
        let mut compressor = brotli::CompressorWriter::new(Vec::new(), 4096, 11, 22);
        compressor.write_all(&json).unwrap();
        let compressed = compressor.into_inner();
        assert!(
            compressed.len() <= BROTLI_Q11_BUDGET,
            "raw={} brotli-q11={} exceeds the {BROTLI_Q11_BUDGET}-byte budget",
            json.len(),
            compressed.len(),
        );
    }

    fn root(tag: &str) -> LangRoot {
        LangRoot { lang_tag: tag.to_string(), url_path: "index.html".to_string() }
    }

    #[test]
    fn an_en_only_site_gets_en_alone() {
        assert_eq!(label_language_tags(&[root("en")]), vec!["en"]);
    }

    #[test]
    fn a_zh_hant_only_site_gets_zh_hant_alone_not_en_too() {
        assert_eq!(label_language_tags(&[root("zh-Hant")]), vec!["zh-Hant"]);
    }

    #[test]
    fn a_bilingual_site_gets_both_keyed_by_language() {
        assert_eq!(label_language_tags(&[root("en"), root("zh-Hant")]), vec!["en", "zh-Hant"]);
    }

    #[test]
    fn a_zh_hans_site_falls_back_to_en_the_pack_has_no_simplified_chinese_names() {
        assert_eq!(label_language_tags(&[root("zh-Hans")]), vec!["en"]);
    }

    #[test]
    fn a_site_with_no_roots_at_all_still_gets_en() {
        assert_eq!(label_language_tags(&[]), vec!["en"]);
    }

    #[test]
    fn zh_hant_name_falls_back_to_english_when_the_pack_has_no_traditional_chinese_name() {
        let label = PointLabel { lng: 0, lat: 0, rank: 0, name_en: "Nameless Cape".into(), name_zht: String::new() };
        let json = PointLabelJson::from_label(&label, "zh-Hant");
        assert_eq!(json.name, "Nameless Cape");
    }

    #[test]
    fn point_and_river_coordinates_are_dequantized_back_to_decimal_degrees() {
        let point = PointLabel { lng: 139_767, lat: 35_681, rank: 0, name_en: "Tokyo".into(), name_zht: "東京".into() };
        let json = PointLabelJson::from_label(&point, "en");
        assert_eq!((json.lng, json.lat), (139.767, 35.681));

        let river = RiverLabel { rank: 1, name_en: "Nile".into(), name_zht: "尼羅河".into(), line: vec![(0, 0), (1000, 2000)] };
        let json = RiverLabelJson::from_label(&river, "zh-Hant");
        assert_eq!(json.name, "尼羅河");
        assert_eq!(json.line, vec![[0.0, 0.0], [1.0, 2.0]]);
    }

    #[test]
    fn emit_writes_only_the_languages_the_site_publishes() {
        let labels = place_map::Labels {
            cities: vec![PointLabel { lng: 0, lat: 0, rank: 0, name_en: "Testville".into(), name_zht: "測試城".into() }],
            ranges: vec![],
            peaks: vec![],
            rivers: vec![],
        };
        let docs = vec![ParsedDocument {
            lang: Language::ZhHant,
            lang_tag: Some("zh-Hant".to_string()),
            url_path: "index.html".to_string(),
            ..Default::default()
        }];
        let output_dir = tempfile::tempdir().unwrap();
        let mut pending = PendingManifest::new(SiteHashes::default());
        emit(&labels, "deadbeef00000000", "zh-Hant", &docs, output_dir.path(), &mut pending).unwrap();
        let path = output_dir.path().join("_moss/map.deadbeef00000000/labels.json");
        let value: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(value["languages"], serde_json::json!(["zh-Hant"]));
        assert!(value.get("en").is_none(), "an en-only key must not appear on a zh-Hant-only site: {value}");
        assert_eq!(value["zh-Hant"]["cities"][0]["name"], "測試城");
    }
}
