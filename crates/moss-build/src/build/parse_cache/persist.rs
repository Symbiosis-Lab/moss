//! The parse cache kept in a file, so the first edit after an open replays
//! pages instead of parsing every one of them again.
//!
//! A document is saved as its own serde shape plus [`Rest`], every field that
//! shape leaves out and capture supports. [`SavedDoc::split`] names every field of
//! `ParsedDocument` with no `..`, so a field added later does not compile
//! until capture explicitly saves it, rejects it, or uses the own shape.
//!
//! The file names the sealed generation it was written for and is read only
//! for a build proved against that same seal (see `ParseSession::begin`): an
//! image a parse baked in can change between two processes without any page
//! changing, and only the seal's proof says it did not.

use super::{ParseCache, ParseCacheEntry, Read};
use crate::build::types::ParsedDocument;
use serde::{Deserialize, Serialize};
use std::path::Path;

const FILE: crate::build::cache::MemoFile = crate::build::cache::MemoFile::new("parse-cache", 5);

#[derive(Serialize, Deserialize)]
struct Saved {
    generation: String,
    inputs_fingerprint: String,
    entries: Vec<(String, SavedEntry)>,
}

#[derive(Serialize, Deserialize)]
struct SavedEntry {
    content_hash: String,
    reads: Vec<Read>,
    doc: SavedDoc,
    problems: Vec<crate::build::cli_output::Problem>,
}

#[derive(Serialize, Deserialize)]
struct SavedDoc {
    doc: ParsedDocument,
    rest: Rest,
}

/// Supported fields that `ParsedDocument`'s own serde shape skips.
#[derive(Serialize, Deserialize)]
struct Rest {
    logo: Option<String>,
    card_cover: Option<String>,
    cover_opted_out: bool,
    byline: Vec<String>,
    colophon: Vec<String>,
    fm_tags: Option<Vec<String>>,
    author: Vec<String>,
    syndicate: Option<Vec<String>>,
    untitled: bool,
    editor: Vec<String>,
    jury: Vec<String>,
    location: Vec<String>,
    event: Option<crate::build::types::EventFields>,
    author_page: Option<moss_core::terms::TermClaim>,
    tag_page: Option<moss_core::terms::TermClaim>,
    editor_page: Option<moss_core::terms::TermClaim>,
    jury_page: Option<moss_core::terms::TermClaim>,
    place_page: Option<moss_core::terms::TermClaim>,
    term_listing: Option<String>,
    term_sections: Option<Vec<(Option<String>, Vec<String>)>>,
    place_children: Option<Vec<(String, String, usize)>>,
    place_names: Option<String>,
    card_extras: Option<String>,
    explorer_places_hash: Option<String>,
    passthrough_root: Option<String>,
    from_sidebar_alias: Option<bool>,
    cascade: Option<std::collections::BTreeMap<String, serde_yaml::Value>>,
    lang: crate::i18n::Language,
    lang_tag: Option<String>,
    clean_stem: String,
    translation_key: Option<String>,
    hero_html: Option<String>,
    header_over: Option<crate::build::markdown::hero_ink::HeaderOver>,
    body_claims_eager: bool,
    lists_folders: bool,
    hero_image_url: Option<String>,
    body_cover_path: Option<String>,
    body_cover_candidates: Vec<crate::build::page::cover::BodyPicture>,
    opening_heading: Option<String>,
    hero_overlay_text: Option<String>,
    citations: Vec<moss_core::ast::quote_source::QuoteSource>,
    features: crate::build::types::PageFeatures,
    kind: moss_core::PageKind,
    typesetting: Option<String>,
    content_width: Option<String>,
    layout: Option<String>,
    color: Option<String>,
    source_path: Option<String>,
    url_override: Option<String>,
    raw_frontmatter: std::collections::BTreeMap<String, serde_json::Value>,
    import_feed_id: Option<String>,
    declared_written: std::collections::BTreeSet<String>,
    field_errors: std::collections::BTreeMap<String, String>,
    title_block: Option<String>,
    members_full: Option<Box<SavedDoc>>,
    bare: bool,
    uid_derived: bool,
    contents_list: bool,
    reader_controls: Option<bool>,
    body_plan: Option<SavedPlan>,
    outgoing_links: Vec<moss_core::resolve::OutgoingLink>,
    embed_deps: Vec<(String, String)>,
    missing_reference_occurrences: Vec<crate::build::types::ReferenceOccurrence>,
    remote_picture_occurrences: Vec<crate::build::types::ReferenceOccurrence>,
}

/// A body plan of HTML, folder-embed sections and inline subscribe forms. A plan
/// with a grid keeps typed AST blocks for the render phase, and the file never
/// reads the AST back: that page is parsed again instead.
#[derive(Serialize, Deserialize)]
struct SavedPlan {
    segments: Vec<SavedSegment>,
    lede_segments: usize,
    first_text_segments: usize,
    after_first_text_segments: usize,
    has_text: bool,
    heading_names: Vec<crate::build::markdown::body_plan::HeadingName>,
}

#[derive(Serialize, Deserialize)]
enum SavedSegment {
    Html(String),
    Section(crate::build::markdown::body_plan::SectionEmission),
    Subscribe(crate::build::markdown::body_plan::SubscribeEmission),
}

impl SavedPlan {
    fn of(plan: &crate::build::markdown::body_plan::BodyPlan) -> Option<Self> {
        use crate::build::markdown::body_plan::{BodyPlan, BodySegment};
        let BodyPlan { segments, lede_segments, first_text_segments, after_first_text_segments, has_text, heading_names } = plan;
        let segments = segments
            .iter()
            .map(|segment| match segment {
                BodySegment::Html(html) => Some(SavedSegment::Html(html.clone())),
                BodySegment::Section(section) => Some(SavedSegment::Section(section.clone())),
                BodySegment::Subscribe(form) => Some(SavedSegment::Subscribe(form.clone())),
                BodySegment::Grid(_) => None,
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            segments,
            lede_segments: *lede_segments,
            first_text_segments: *first_text_segments,
            after_first_text_segments: *after_first_text_segments,
            has_text: *has_text,
            heading_names: heading_names.clone(),
        })
    }

    fn into_plan(self) -> crate::build::markdown::body_plan::BodyPlan {
        use crate::build::markdown::body_plan::{BodyPlan, BodySegment};
        let Self { segments, lede_segments, first_text_segments, after_first_text_segments, has_text, heading_names } = self;
        let segments = segments
            .into_iter()
            .map(|segment| match segment {
                SavedSegment::Html(html) => BodySegment::Html(html),
                SavedSegment::Section(section) => BodySegment::Section(section),
                SavedSegment::Subscribe(form) => BodySegment::Subscribe(form),
            })
            .collect();
        BodyPlan { segments, lede_segments, first_text_segments, after_first_text_segments, has_text, heading_names }
    }
}

impl SavedDoc {
    /// `None` for a document holding something this file does not keep: its
    /// translation links, which the passes after the parse set, and typed
    /// field values, whose serde shape cannot be read back, a hero drawn from
    /// another page or a grid, which hold typed AST blocks.
    /// Term cards, term lines and section accents belong to later passes and
    /// are also refused if present. The page is parsed again instead.
    fn split(doc: &ParsedDocument) -> Option<Self> {
        let ParsedDocument {
            title: _,
            content: _,
            html_content: _,
            url_path: _,
            date: _,
            reading_time: _,
            slug: _,
            permalink: _,
            label: _,
            nav_label: _,
            weight: _,
            analytics: _,
            logo,
            cover: _,
            card_cover,
            cover_opted_out,
            term_card,
            cover_type: _,
            media_items: _,
            nav: _,
            is_root_level: _,
            edition: _,
            is_home_override: _,
            draft: _,
            empty_source: _,
            listed: _,
            search_engines: _,
            hidden_home: _,
            slot: _,
            slot_only: _,
            description: _,
            byline,
            colophon,
            tags: _,
            fm_tags,
            author,
            syndicate,
            untitled,
            editor,
            jury,
            location,
            event,
            author_page,
            tag_page,
            editor_page,
            jury_page,
            place_page,
            term_listing,
            term_sections,
            place_children,
            term_lines,
            place_names,
            card_extras,
            is_place_namespace_root: _,
            explorer_places_hash,
            passthrough_root,
            map: _,
            has_own_map_embed: _,
            country_lists: _,
            embedded_listing_urls: _,
            country_map: _,
            route: _,
            children: _,
            children_source: _,
            sidebar: _,
            children_style: _,
            children_group: _,
            children_depth: _,
            children_listed: _,
            children_in: _,
            children_limit: _,
            children_more: _,
            children_covers: _,
            children_when: _,
            children_pictures: _,
            children_motion: _,
            children_count: _,
            from_sidebar_alias,
            series: _,
            breadcrumb: _,
            footer: _,
            cascade,
            inherited: _,
            also_in: _,
            comments: _,
            thread: _,
            lang,
            lang_tag,
            clean_stem,
            translation_key,
            translations,
            hero_html,
            header_over,
            hero_from,
            body_claims_eager,
            lists_folders,
            hero_image_url,
            body_cover_path,
            body_cover_candidates,
            opening_heading,
            hero_overlay_text,
            citations,
            features,
            kind,
            uid: _,
            uid_derived,
            typesetting,
            content_width,
            layout,
            color,
            section,
            source_path,
            url_override,
            raw_frontmatter,
            import_feed_id,
            declared,
            declared_written,
            field_errors,
            placed_fields,
            title_block,
            members_full,
            bare,
            contents_list,
            reader_controls,
            direct_children_sort: _,
            sort: _,
            body_plan,
            outgoing_links,
            embed_deps,
            missing_reference_occurrences,
            remote_picture_occurrences,
        } = doc;
        if !translations.is_empty() || !placed_fields.is_empty() || !declared.is_empty() || hero_from.is_some() {
            return None;
        }
        if term_card.is_some() || !term_lines.is_empty() || section.is_some() {
            return None;
        }
        let members_full = match members_full {
            Some(full) => Some(Box::new(Self::split(full)?)),
            None => None,
        };
        let body_plan = match body_plan {
            Some(plan) => Some(SavedPlan::of(plan)?),
            None => None,
        };
        Some(Self {
            doc: doc.clone(),
            rest: Rest {
                logo: logo.clone(),
                card_cover: card_cover.clone(),
                cover_opted_out: cover_opted_out.clone(),
                byline: byline.clone(),
                colophon: colophon.clone(),
                fm_tags: fm_tags.clone(),
                author: author.clone(),
                syndicate: syndicate.clone(),
                untitled: untitled.clone(),
                editor: editor.clone(),
                jury: jury.clone(),
                location: location.clone(),
                event: event.clone(),
                author_page: author_page.clone(),
                tag_page: tag_page.clone(),
                editor_page: editor_page.clone(),
                jury_page: jury_page.clone(),
                place_page: place_page.clone(),
                term_listing: term_listing.clone(),
                term_sections: term_sections.clone(),
                place_children: place_children.clone(),
                place_names: place_names.clone(),
                card_extras: card_extras.clone(),
                explorer_places_hash: explorer_places_hash.clone(),
                passthrough_root: passthrough_root.clone(),
                from_sidebar_alias: from_sidebar_alias.clone(),
                cascade: cascade.clone(),
                lang: lang.clone(),
                lang_tag: lang_tag.clone(),
                clean_stem: clean_stem.clone(),
                translation_key: translation_key.clone(),
                hero_html: hero_html.clone(),
                header_over: header_over.clone(),
                body_claims_eager: body_claims_eager.clone(),
                lists_folders: lists_folders.clone(),
                hero_image_url: hero_image_url.clone(),
                body_cover_path: body_cover_path.clone(),
                body_cover_candidates: body_cover_candidates.clone(),
                opening_heading: opening_heading.clone(),
                hero_overlay_text: hero_overlay_text.clone(),
                citations: citations.clone(),
                features: features.clone(),
                kind: kind.clone(),
                typesetting: typesetting.clone(),
                content_width: content_width.clone(),
                layout: layout.clone(),
                color: color.clone(),
                source_path: source_path.clone(),
                url_override: url_override.clone(),
                raw_frontmatter: raw_frontmatter.clone(),
                import_feed_id: import_feed_id.clone(),
                declared_written: declared_written.clone(),
                field_errors: field_errors.clone(),
                title_block: title_block.clone(),
                members_full,
                bare: bare.clone(),
                uid_derived: *uid_derived,
                contents_list: contents_list.clone(),
                reader_controls: reader_controls.clone(),
                body_plan,
                outgoing_links: outgoing_links.clone(),
                embed_deps: embed_deps.clone(),
                missing_reference_occurrences: missing_reference_occurrences.clone(),
                remote_picture_occurrences: remote_picture_occurrences.clone(),
            },
        })
    }

    fn join(self) -> ParsedDocument {
        let Self { mut doc, rest } = self;
        let Rest {
            logo,
            card_cover,
            cover_opted_out,
            byline,
            colophon,
            fm_tags,
            author,
            syndicate,
            untitled,
            editor,
            jury,
            location,
            event,
            author_page,
            tag_page,
            editor_page,
            jury_page,
            place_page,
            term_listing,
            term_sections,
            place_children,
            place_names,
            card_extras,
            explorer_places_hash,
            passthrough_root,
            from_sidebar_alias,
            cascade,
            lang,
            lang_tag,
            clean_stem,
            translation_key,
            hero_html,
            header_over,
            body_claims_eager,
            lists_folders,
            hero_image_url,
            body_cover_path,
            body_cover_candidates,
            opening_heading,
            hero_overlay_text,
            citations,
            features,
            kind,
            typesetting,
            content_width,
            layout,
            color,
            source_path,
            url_override,
            raw_frontmatter,
            import_feed_id,
            declared_written,
            field_errors,
            title_block,
            members_full,
            bare,
            uid_derived,
            contents_list,
            reader_controls,
            body_plan,
            outgoing_links,
            embed_deps,
            missing_reference_occurrences,
            remote_picture_occurrences,
        } = rest;
        doc.logo = logo;
        doc.card_cover = card_cover;
        doc.cover_opted_out = cover_opted_out;
        doc.byline = byline;
        doc.colophon = colophon;
        doc.fm_tags = fm_tags;
        doc.author = author;
        doc.syndicate = syndicate;
        doc.untitled = untitled;
        doc.editor = editor;
        doc.jury = jury;
        doc.location = location;
        doc.event = event;
        doc.author_page = author_page;
        doc.tag_page = tag_page;
        doc.editor_page = editor_page;
        doc.jury_page = jury_page;
        doc.place_page = place_page;
        doc.term_listing = term_listing;
        doc.term_sections = term_sections;
        doc.place_children = place_children;
        doc.place_names = place_names;
        doc.card_extras = card_extras;
        doc.explorer_places_hash = explorer_places_hash;
        doc.passthrough_root = passthrough_root;
        doc.from_sidebar_alias = from_sidebar_alias;
        doc.cascade = cascade;
        doc.lang = lang;
        doc.lang_tag = lang_tag;
        doc.clean_stem = clean_stem;
        doc.translation_key = translation_key;
        doc.hero_html = hero_html;
        doc.header_over = header_over;
        doc.body_claims_eager = body_claims_eager;
        doc.lists_folders = lists_folders;
        doc.hero_image_url = hero_image_url;
        doc.body_cover_path = body_cover_path;
        doc.body_cover_candidates = body_cover_candidates;
        doc.opening_heading = opening_heading;
        doc.hero_overlay_text = hero_overlay_text;
        doc.citations = citations;
        doc.features = features;
        doc.kind = kind;
        doc.typesetting = typesetting;
        doc.content_width = content_width;
        doc.layout = layout;
        doc.color = color;
        doc.source_path = source_path;
        doc.url_override = url_override;
        doc.raw_frontmatter = raw_frontmatter;
        doc.import_feed_id = import_feed_id;
        doc.declared_written = declared_written;
        doc.field_errors = field_errors;
        doc.title_block = title_block;
        doc.members_full = members_full.map(|m| Box::new(m.join()));
        doc.bare = bare;
        doc.uid_derived = uid_derived;
        doc.contents_list = contents_list;
        doc.reader_controls = reader_controls;
        doc.body_plan = body_plan.map(SavedPlan::into_plan);
        doc.outgoing_links = outgoing_links;
        doc.embed_deps = embed_deps;
        doc.missing_reference_occurrences = missing_reference_occurrences;
        doc.remote_picture_occurrences = remote_picture_occurrences;
        doc
    }
}

/// Keep `cache` for the next process, as the parse of the generation sealed as
/// `generation`. A graph identity is not kept: it is stable only within one
/// process, so a loaded entry asks its graph questions again.
pub(super) fn write(dir: &Path, generation: &str, cache: &ParseCache) {
    let started = std::time::Instant::now();
    let entries: Vec<_> = cache
        .entries
        .iter()
        .filter_map(|(path, entry)| {
            let reads = entry.reads.iter().filter(|read| !matches!(read, Read::GraphIdentity(_))).cloned().collect();
            Some((path.clone(), SavedEntry { content_hash: entry.content_hash.clone(), reads, doc: SavedDoc::split(&entry.doc)?, problems: entry.problems.clone() }))
        })
        .collect();
    let skipped = cache.entries.len() - entries.len();
    let saved = Saved { generation: generation.to_owned(), inputs_fingerprint: cache.inputs_fingerprint.clone(), entries };
    FILE.write(dir, &saved);
    log::info!(target: "incremental", "parse cache: kept {} pages ({skipped} with typed fields not kept) in {:?}", saved.entries.len(), started.elapsed());
}

/// The cache kept for the generation sealed as `generation`, if that is the
/// one the file was written for.
pub(super) fn read(dir: &Path, generation: &str) -> Option<ParseCache> {
    let saved: Saved = FILE.read(dir)?;
    if saved.generation != generation {
        return None;
    }
    let entries = saved
        .entries
        .into_iter()
        .map(|(path, entry)| (path, ParseCacheEntry { content_hash: entry.content_hash, reads: entry.reads, doc: entry.doc.join(), problems: entry.problems }))
        .collect();
    Some(ParseCache { inputs_fingerprint: saved.inputs_fingerprint, entries })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::terms::TermLine;
    use crate::build::types::{SectionAccent, TermCard};

    #[test]
    fn saved_shape_omits_later_pass_fields() {
        let doc = ParsedDocument { title: "Kept title".into(), logo: Some("logo.svg".into()), ..Default::default() };
        let saved = SavedDoc::split(&doc).expect("a parse without later-pass values is kept");
        let value = serde_json::to_value(saved).unwrap();
        for shape in ["doc", "rest"] {
            for field in ["term_card", "term_lines", "section"] {
                assert!(!value[shape].as_object().unwrap().contains_key(field), "{shape} must omit {field}");
            }
        }
        let restored = serde_json::from_value::<SavedDoc>(value).unwrap().join();
        assert_eq!(restored.title, doc.title);
        assert_eq!(restored.logo, doc.logo);
        assert!(restored.term_card.is_none());
        assert!(restored.term_lines.is_empty());
        assert!(restored.section.is_none());
    }

    #[test]
    fn capture_refuses_each_later_pass_value_including_members() {
        let cases = [
            ParsedDocument { term_card: Some(TermCard::default()), ..Default::default() },
            ParsedDocument { term_lines: vec![TermLine {
                term: "tags".into(), field: "tags".into(), label: "Tags".into(),
                separator: ": ".into(), links: "[Notes](/tags/notes/)".into(), is_place: false,
            }], ..Default::default() },
            ParsedDocument { section: Some(SectionAccent {
                color: "#123".into(), text_light: "#112233".into(), text_dark: "#aabbcc".into(), segment: "notes".into(),
            }), ..Default::default() },
        ];
        for doc in cases {
            assert!(SavedDoc::split(&doc).is_none(), "later-pass content must be reparsed");
            let members = ParsedDocument { members_full: Some(Box::new(doc)), ..Default::default() };
            assert!(SavedDoc::split(&members).is_none(), "nested content must also be reparsed");
        }
    }
}
