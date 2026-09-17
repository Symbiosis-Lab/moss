//! Email/RSS math PNG projection (ADR-030 §3.4/§3.5, design
//! `docs/archive/2026-07-21-latex-math-design.md`).
//!
//! The website inlines each typeset equation as an `<svg>` (see
//! `build::markdown::math`). Two surfaces cannot show that SVG:
//!
//! - **Email**: Gmail strips `<svg>`; Outlook has been actively blocking SVG
//!   since 2025-09; data URIs are blocked by Gmail. The only universally
//!   supported form (caniemail, 2026-07-20 dataset) is a **hosted PNG**.
//! - **RSS**: feed readers sanitize `<svg>` away — equations would silently
//!   vanish for every reader subscriber.
//!
//! So this module rasterizes the SAME validated engine SVG the web inlines to
//! a 2× PNG with an **opaque light background baked in** (~0.5 em padding).
//! Opaque-light is the only dark-mode-safe design: Gmail supports neither
//! `prefers-color-scheme` nor `<picture>`, so near-black-on-transparent would
//! go invisible on a dark client.
//!
//! ## Content addressing + append-only retention (§3.4)
//!
//! The PNG is served at `/_moss/math/<hash>.png` where `<hash>` is SHA-256
//! over the full render-input tuple `(tex, display, font_size, fg, bg,
//! engine_semver, cjk_font_sha256|"none")`, truncated to 16 lowercase hex
//! (the `og_card.rs` pattern). Gmail's image proxy and Apple MPP cache these
//! URLs **forever**, so a URL must never serve different bytes — an engine
//! upgrade re-keys new sends while every URL already in an inbox keeps its
//! original bytes. That also means the family is **append-only**: math PNGs
//! are exempt from stale-file/dir pruning (`media::pipeline`), and
//! [`emit_math_pngs`] re-registers every PNG already on disk into each
//! build's manifest so old equations keep shipping in every deployed
//! generation. *Falsifier (ADR-030): if pruning ever removes a math PNG that
//! a sent email references, adopt the append-only ledger mechanism
//! (`.moss/data/math-ledger.json`) in place of the exemption rule.*
//!
//! ## The newsletter needs no AssetSnapshot
//!
//! `content_hash` is pure — the email renderer computes the same hash from
//! the same event text and emits the same URL, and the build guarantees the
//! file exists (plus the publish-before-send guard in `email::commands`).
//!
//! CJK-refused math ([`MathRefusal`]) gets NO PNG: email keeps the escaped
//! `<code>` source form. Only typeset-eligible math upgrades to `<img>`.

use crate::build::markdown::math::{font, typeset, MathRefusal};
use crate::build::served_path::ServedPath;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::Path;

/// Served-path prefix of the family; the stale-cleanup exemptions in
/// `media::pipeline::{remove_stale_files, remove_stale_dirs}` key on this.
pub const MATH_PNG_PREFIX: &str = "_moss/math/";

/// Email body font size in CSS px at 1×. Matches the newsletter's paragraph
/// style (`font-size: 16px`) so an inline equation sits at text size.
const EMAIL_FONT_PX: f64 = 16.0;

/// Raster scale factor: 2× for high-DPI inboxes; the `<img>` carries 1× dims.
const RASTER_SCALE: u32 = 2;

/// Opaque padding around the ink box, in em (each side).
const PAD_EM: f64 = 0.5;

/// Glyph color baked into the PNG. The engine bakes `rgba(0,0,0,1)` fills;
/// we rasterize them as-is, so this is black. Part of the content address.
const FG: &str = "#000000";

/// The opaque light background baked into the PNG. Part of the content
/// address.
const BG: &str = "#ffffff";

/// The pinned RaTeX version. A version bump MUST change this constant (the
/// `engine_semver_matches_cargo_pin` test enforces it) so every re-keyed
/// render gets a fresh URL instead of new bytes at a cached URL.
const ENGINE_SEMVER: &str = "0.1.13";

/// Pure content address: 16 lowercase hex over the full render-input tuple.
/// Both call sites (build emission, email render) MUST go through this
/// function — same inputs, same hash, same URL, no AssetSnapshot needed.
///
/// The CJK-font component is the sha256 of the verified+pinned system CJK font
/// (§3.6), or the literal `"none"` when no font is pinned (pure-ASCII math, or
/// a machine without a CJK font). Cross-machine font drift then yields a
/// *different URL* — never different bytes at a URL Gmail's proxy cached.
pub fn content_hash(tex: &str, display: bool) -> String {
    // Fold the pinned font's fingerprint in ONLY when this equation actually
    // carries CJK (needs the font). Pure-ASCII/Latin math must hash to the same
    // URL on every build host, so it stays "none" whether or not a CJK font
    // happens to be installed — otherwise identical ASCII email images would get
    // machine-dependent (macOS vs Linux CI) content addresses.
    let cjk_font_sha256 = if tex.chars().any(font::needs_cjk_font) {
        font::pinned().map(|f| f.sha256()).unwrap_or("none")
    } else {
        "none"
    };
    let mut h = Sha256::new();
    for part in [
        tex,
        if display { "display" } else { "inline" },
        &format!("{EMAIL_FONT_PX}"),
        FG,
        BG,
        ENGINE_SEMVER,
        cjk_font_sha256,
    ] {
        h.update(part.as_bytes());
        h.update(b"\0");
    }
    let bytes = h.finalize();
    bytes.iter().take(8).map(|b| format!("{:02x}", b)).collect()
}

/// A typeset equation ready for the email `<img>` projection: the raw engine
/// SVG (for rasterization) plus everything the `<img>` tag needs.
pub struct EmailMathImg {
    /// 16-hex content address ([`content_hash`]).
    pub hash: String,
    /// The equation source, no delimiters.
    pub tex: String,
    pub display: bool,
    /// `width` attribute at 1× CSS px (padding included).
    pub width_1x: u32,
    /// `height` attribute at 1× CSS px (padding included).
    pub height_1x: u32,
    /// Inline math: `vertical-align` in CSS px at 1× (negative — descent plus
    /// bottom padding), so the equation's baseline lands on the text baseline.
    pub valign_1x_px: f64,
    /// The validated engine SVG (path-only; `build::markdown::math` layer B).
    svg: String,
}

impl EmailMathImg {
    /// The served path `_moss/math/<hash>.png`. Infallible by construction:
    /// [`content_hash`] always produces the 16-lowercase-hex shape
    /// `ServedPath::for_math_png` validates.
    pub fn served_path(&self) -> ServedPath {
        ServedPath::for_math_png(&self.hash).expect("content_hash emits 16 lowercase hex")
    }

    /// The email-client-safe `<img>` element (§3.5): `width`/`height` at 1×
    /// (Outlook ignores them without `style`, hence also `max-width:100%`),
    /// `alt` = the raw LaTeX source WITH delimiters, `role="img"`, inline
    /// font styling so a blocked-image alt text reads as code, display math
    /// block + centered, inline math baseline-aligned via `vertical-align`.
    ///
    /// `site_url` empty → root-relative src (tests/preview); otherwise the
    /// absolute URL Gmail's proxy will cache forever.
    pub fn email_html(&self, site_url: &str) -> String {
        let rel = self.served_path().to_relative_url();
        let src = if site_url.is_empty() {
            rel
        } else {
            format!("{}{}", site_url.trim_end_matches('/'), rel)
        };
        let alt = escape_attr(&moss_core::ast::math_text::math_source(
            &self.tex,
            self.display,
        ));
        let (w, h) = (self.width_1x, self.height_1x);
        let style = if self.display {
            "display:block;margin:16px auto;max-width:100%;border:0;font-family:monospace;font-size:14px;color:#333;".to_string()
        } else {
            format!(
                "max-width:100%;vertical-align:{:.2}px;border:0;font-family:monospace;font-size:14px;color:#333;",
                self.valign_1x_px
            )
        };
        format!(
            r#"<img src="{src}" alt="{alt}" role="img" width="{w}" height="{h}" style="{style}" />"#
        )
    }
}

/// Typeset `tex` for the email projection, or refuse (→ the caller keeps the
/// escaped-source `<code>` form). Runs the full `markdown::math` envelope —
/// guard, big-stack render, output validation — then derives the `<img>`
/// geometry from the same layout metrics that drive the web SVG.
pub fn email_math_img(tex: &str, display: bool) -> Result<EmailMathImg, MathRefusal> {
    let t = typeset(tex)?;
    let ink_w = t.width_em;
    let ink_h = t.height_em + t.depth_em;
    if !(ink_w > 0.0 && ink_h > 0.0) || !ink_w.is_finite() || !ink_h.is_finite() {
        return Err(MathRefusal::InvalidOutput {
            reason: "non-positive layout metrics",
        });
    }
    let width_1x = ((ink_w + 2.0 * PAD_EM) * EMAIL_FONT_PX).ceil().max(1.0) as u32;
    let height_1x = ((ink_h + 2.0 * PAD_EM) * EMAIL_FONT_PX).ceil().max(1.0) as u32;
    let valign_1x_px = -((t.depth_em + PAD_EM) * EMAIL_FONT_PX);
    Ok(EmailMathImg {
        hash: content_hash(tex, display),
        tex: tex.to_string(),
        display,
        width_1x,
        height_1x,
        valign_1x_px,
        svg: t.svg,
    })
}

/// One-call form for the email/RSS renderers: the finished `<img>` HTML for
/// typeset-eligible math, `None` on any refusal (caller falls back to the
/// escaped-source form). Pure with respect to the filesystem — the build
/// guarantees the referenced PNG exists (plus the publish-before-send guard).
pub fn email_math_html(tex: &str, display: bool, site_url: &str) -> Option<String> {
    email_math_img(tex, display).ok().map(|img| img.email_html(site_url))
}

/// Rasterize to the 2× opaque PNG. The engine SVG is path-only (layer B
/// guarantees it), so an EMPTY fontdb is fine — proven by `og_card.rs`'s
/// text-free rendering path and the tests below.
fn rasterize_png(img: &EmailMathImg) -> Result<Vec<u8>, String> {
    let opt = usvg::Options::default();
    let tree = usvg::Tree::from_str(&img.svg, &opt).map_err(|e| format!("math svg parse: {e}"))?;
    let tree_w = tree.size().width() as f64;
    if !(tree_w > 0.0) {
        return Err("math svg has zero width".to_string());
    }

    let scale = RASTER_SCALE as f64;
    let px_w = img.width_1x * RASTER_SCALE;
    let px_h = img.height_1x * RASTER_SCALE;
    let mut pixmap = tiny_skia::Pixmap::new(px_w, px_h)
        .ok_or_else(|| "pixmap allocation failed".to_string())?;

    // Opaque light background — the dark-mode-safe floor (module docs).
    pixmap.fill(tiny_skia::Color::from_rgba8(0xff, 0xff, 0xff, 0xff));

    // Uniform scale mapping the parsed tree size onto the ink box in device
    // px, then translate by the padding. Deriving the factor from the parsed
    // size (not the pt→px convention) keeps this correct if usvg's unit
    // handling ever changes.
    let ink_w_px = img.tex_ink_width_px(scale);
    let s = (ink_w_px / tree_w) as f32;
    let pad_px = (PAD_EM * EMAIL_FONT_PX * scale) as f32;
    let transform = tiny_skia::Transform::from_scale(s, s).post_translate(pad_px, pad_px);
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    pixmap.encode_png().map_err(|e| format!("png encode: {e}"))
}

impl EmailMathImg {
    /// Ink-box width in device px at `scale`× (padding excluded).
    fn tex_ink_width_px(&self, scale: f64) -> f64 {
        (self.width_1x as f64 - 2.0 * PAD_EM * EMAIL_FONT_PX) * scale
    }
}

/// Atomic write: temp + rename, the `og_card.rs` pattern (`.pending.<uuid>`
/// because iCloud Drive excludes `.tmp` from sync). rename(2) is atomic
/// within a directory, so a reader sees no file or a complete one; identical
/// inputs render identical bytes, so whichever writer wins is immaterial.
fn write_atomic(disk_path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = disk_path.parent() {
        crate::build::io_utils::create_output_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    let tmp = disk_path.with_extension(format!("pending.{}", uuid::Uuid::new_v4()));
    std::fs::write(&tmp, bytes).map_err(|e| format!("write {}: {e}", tmp.display()))?;  // allow:raw_write the temp this fn just minted; the rename below is the output write
    // allow:unlink rename into place: the PNG is replaced, never absent
    std::fs::rename(&tmp, disk_path).map_err(|e| format!("rename {}: {e}", disk_path.display()))
}

/// Collect every math event from `markdown`, in source order, delimiter-free.
/// Parses with the SAME pulldown dialect as the web build and the email
/// walkers (`moss_core::ast::parser_options`), so the event text — and thus
/// the content hash — is identical at every call site.
fn collect_math_events(markdown: &str) -> Vec<(String, bool)> {
    use pulldown_cmark::{Event, Parser};
    Parser::new_ext(markdown, moss_core::ast::parser_options(true))
        .filter_map(|ev| match ev {
            Event::InlineMath(t) => Some((t.to_string(), false)),
            Event::DisplayMath(t) => Some((t.to_string(), true)),
            _ => None,
        })
        .collect()
}

/// Content hashes of every typeset-*eligible* equation in `markdown` (sorted,
/// deduped). Refused math (CJK, over-caps, parse failures) is excluded — it
/// ships as `<code>` source and references no PNG. The email send path uses
/// this for the publish-before-send guard.
pub fn typeset_math_hashes(markdown: &str) -> Vec<String> {
    let mut out = BTreeSet::new();
    for (tex, display) in collect_math_events(markdown) {
        if email_math_img(&tex, display).is_ok() {
            out.insert(content_hash(&tex, display));
        }
    }
    out.into_iter().collect()
}

/// Build-phase entry point, gated (not implemented) in `blocking.rs`:
///
/// 1. When `math_enabled`, rasterize a 2× PNG for every unique
///    typeset-eligible equation across `documents` (idempotent — existing
///    non-empty files are kept byte-for-byte; a 0-byte cloud-eviction stub is
///    re-rendered).
/// 2. **Always** re-register every PNG already under `_moss/math/` into this
///    build's manifest (append-only retention: URLs in sent emails must keep
///    resolving in every future deployed generation).
///
/// A single equation failing to rasterize logs a warning and is skipped —
/// its email form falls back to `<code>` source; it must not fail the build.
pub fn emit_math_pngs(
    documents: &[crate::build::types::ParsedDocument],
    math_enabled: bool,
    output_dir: &Path,
    pending: &mut crate::build::manifest::PendingManifest,
) -> Result<(), String> {
    use crate::build::manifest::HashBucket;

    if math_enabled {
        let mut unique: BTreeSet<(String, bool)> = BTreeSet::new();
        for doc in documents {
            unique.extend(collect_math_events(&doc.content));
        }
        for (tex, display) in &unique {
            let img = match email_math_img(tex, *display) {
                Ok(img) => img,
                Err(_) => continue, // refused math ships as <code>; no PNG
            };
            let disk = img.served_path().to_disk(output_dir);
            if crate::build::io_utils::output_present(&disk) {
                continue; // content-addressed: same hash ⇒ same bytes
            }
            match rasterize_png(&img) {
                Ok(png) => write_atomic(&disk, &png)?,
                Err(e) => {
                    log::warn!("math png rasterize failed for {tex:?}: {e}");
                }
            }
        }
    }

    // Append-only retention: every PNG on disk — this build's AND every
    // earlier build's — joins the manifest so it ships in this generation.
    let math_dir = output_dir.join("_moss").join("math");
    if math_dir.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(&math_dir)
            .map_err(|e| format!("read {}: {e}", math_dir.display()))?
            .filter_map(|e| e.ok())
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            // A crash between temp-write and rename leaves `*.pending.<uuid>`
            // behind. It is never registered; the permitted staging sweep
            // removes it. Removing it here could take a concurrent
            // `write_atomic`'s temp mid-rename.
            if name.contains(".pending.") {
                continue;
            }
            let Some(stem) = name.strip_suffix(".png") else { continue };
            let Ok(sp) = ServedPath::for_math_png(stem) else { continue };
            match std::fs::read(entry.path()) {
                Ok(bytes) if !bytes.is_empty() => {
                    pending.register(&sp, &bytes, HashBucket::Files);
                }
                // Unreadable or a 0-byte eviction stub. Dropping it would let
                // the next publish remove a URL that is already in somebody's
                // inbox, which ADR-030 §3.4 forbids — so carry the previous
                // build's entry instead. The PNG is content-addressed, so that
                // entry describes these exact bytes; C1 is what makes the file
                // itself come back.
                _ => {
                    if let Some(previous) = pending.files().get(sp.as_str()).cloned() {
                        pending.register_hashed(&sp, &previous, HashBucket::Files);
                    }
                }
            }
        }
    }
    Ok(())
}

/// Swap every typeset math `<svg>` in already-rendered website HTML for the
/// hosted-PNG `<img>` form — the RSS projection. Feed readers sanitize
/// `<svg>` away, so item content must carry the same absolute PNG URLs email
/// uses. Refused math never produced an `<svg>` (it is a `<code>` node) and
/// passes through untouched.
///
/// Operates on the exact byte shape `markdown::math::assemble` emits: the
/// open tag carries `class="moss-math" data-moss-math="…"` + an `aria-label`
/// holding the escaped source, the body is path-only (validated — no nested
/// `</svg>` possible), and display math is wrapped in
/// `<div class="moss-math-scroll">…</div>`.
pub fn swap_math_svgs_for_email_imgs(html: &str, site_url: &str) -> String {
    const SCROLL_OPEN: &str = r#"<div class="moss-math-scroll">"#;

    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some((before, from_svg)) = split_at_math_svg(rest) {
        let Some(gt) = from_svg.find('>') else { break };
        // `..=gt` and `gt + 1..` both land on the ASCII '>' just found.
        let (Some(open_tag), Some(after_open)) = (from_svg.get(..=gt), from_svg.get(gt + 1..))
        else {
            break;
        };
        let Some(close_rel) = after_open.find("</svg>") else { break };
        let Some(after_svg) = after_open.get(close_rel + "</svg>".len()..) else { break };

        let display = open_tag.contains(r#"data-moss-math="display""#);
        let tex = attr_value(open_tag, "aria-label").map(|v| unescape_attr(v));

        // Display math: consume the scroll wrapper around the svg too.
        let (lead, tail) = match (display, after_svg.strip_prefix("</div>")) {
            (true, Some(after_wrapper)) => match before.strip_suffix(SCROLL_OPEN) {
                Some(before_wrapper) => (before_wrapper, after_wrapper),
                None => (before, after_svg),
            },
            _ => (before, after_svg),
        };

        out.push_str(lead);
        match tex.as_deref().and_then(|t| email_math_html(t, display, site_url)) {
            Some(img) => out.push_str(&img),
            None => {
                // Unreachable in practice (the svg only exists because this
                // equation typeset); keep the reader-safe source form anyway.
                let src = tex.unwrap_or_default();
                out.push_str(&format!(
                    r#"<code class="moss-math">{}</code>"#,
                    escape_attr(&moss_core::ast::math_text::math_source(&src, display))
                ));
            }
        }
        rest = tail;
    }
    out.push_str(rest);
    out
}

/// Split `html` at the next `<svg` whose open tag carries the moss-math
/// marker, into `(before, from_svg)`.
fn split_at_math_svg(html: &str) -> Option<(&str, &str)> {
    for (pos, _) in html.match_indices("<svg") {
        let (before, from_svg) = (html.get(..pos)?, html.get(pos..)?);
        let tag_end = from_svg.find('>')?;
        if from_svg
            .get(..tag_end)?
            .contains(r#"class="moss-math" data-moss-math=""#)
        {
            return Some((before, from_svg));
        }
    }
    None
}

/// Extract a double-quoted attribute value from an element open tag.
fn attr_value<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let (_, after_name) = tag.split_once(&format!("{name}=\""))?;
    let (value, _) = after_name.split_once('"')?;
    Some(value)
}

/// Inverse of `markdown::math::escape_attr`. `&amp;` is replaced LAST so a
/// literal `&lt;` in the source (escaped to `&amp;lt;`) round-trips.
fn unescape_attr(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// Attribute-escape (the five break-out chars), local copy of
/// `markdown::math::escape_attr` (private there).
fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::manifest::PendingManifest;
    use crate::build::types::ParsedDocument;

    fn doc_with(content: &str) -> ParsedDocument {
        ParsedDocument {
            content: content.to_string(),
            ..Default::default()
        }
    }

    // ---- Content addressing ----

    #[test]
    fn content_hash_is_pure_and_16_lowercase_hex() {
        let a = content_hash(r"E=mc^2", false);
        let b = content_hash(r"E=mc^2", false);
        assert_eq!(a, b, "same inputs must produce the same hash");
        assert_eq!(a.len(), 16);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn content_hash_distinguishes_tex_and_display() {
        assert_ne!(content_hash("a+b", false), content_hash("a+c", false));
        assert_ne!(
            content_hash("a+b", false),
            content_hash("a+b", true),
            "display mode is part of the address"
        );
    }

    #[test]
    fn hash_agrees_between_build_and_email_call_sites() {
        // The build side hashes via email_math_img; the newsletter side via
        // email_math_html → email_math_img. Both bottom out in content_hash
        // with identical inputs — the URL in the email must be the URL the
        // build wrote.
        let img = email_math_img(r"\sum_{i=1}^n x_i", true).unwrap();
        assert_eq!(img.hash, content_hash(r"\sum_{i=1}^n x_i", true));
        let html = email_math_html(r"\sum_{i=1}^n x_i", true, "https://example.com").unwrap();
        assert!(
            html.contains(&format!("https://example.com/_moss/math/{}.png", img.hash)),
            "email html must reference the content-addressed URL: {html}"
        );
    }

    /// Every ratex crate whose output shapes the PNG bytes must sit at the
    /// pinned [`ENGINE_SEMVER`]. Not just `ratex-parser`: `ratex-layout`
    /// (glyph placement) and `ratex-svg` (path emission) each change the
    /// rasterized bytes, and a bump of ANY of them without re-keying serves
    /// new bytes at URLs Gmail's proxy cached forever.
    fn check_engine_pins(manifest: &str, semver: &str) -> Result<(), String> {
        let pin = format!("\"={semver}\"");
        for krate in ["ratex-parser", "ratex-layout", "ratex-svg"] {
            let line = manifest
                .lines()
                .find(|l| l.trim_start().starts_with(&format!("{krate} ")))
                .ok_or_else(|| format!("{krate} not found in Cargo.toml"))?;
            if !line.contains(&pin) {
                return Err(format!(
                    "{krate} is not pinned at ={semver}: {line:?} — bump ENGINE_SEMVER \
                     so new renders get fresh URLs instead of new bytes at cached URLs"
                ));
            }
        }
        Ok(())
    }

    #[test]
    fn engine_semver_matches_cargo_pin() {
        // A RaTeX bump MUST re-key every URL (Gmail's proxy caches forever).
        // This trips when Cargo.toml moves any ratex crate off =ENGINE_SEMVER
        // while the constant stays behind (or vice versa).
        let manifest = include_str!("../../../Cargo.toml");
        if let Err(e) = check_engine_pins(manifest, ENGINE_SEMVER) {
            panic!("ENGINE_SEMVER ({ENGINE_SEMVER}) no longer matches Cargo.toml: {e}");
        }
    }

    #[test]
    fn engine_pin_check_catches_a_lone_layout_or_svg_bump() {
        // The mutation this guards against: someone bumps ratex-svg (or
        // -layout) alone — parser still matches, bytes change anyway.
        let doctored = r#"
            ratex-parser = "=0.1.13"
            ratex-layout = "=0.1.13"
            ratex-svg = { version = "=0.1.14", features = ["standalone"] }
        "#;
        assert!(
            check_engine_pins(doctored, "0.1.13").is_err(),
            "a lone ratex-svg bump must trip the pin check"
        );
        let missing = r#"ratex-parser = "=0.1.13""#;
        assert!(
            check_engine_pins(missing, "0.1.13").is_err(),
            "a manifest missing a ratex crate must trip the pin check"
        );
    }

    // ---- The <img> form (§3.5) ----

    #[test]
    fn inline_img_carries_dims_valign_and_source_alt() {
        let img = email_math_img("a_i", false).unwrap();
        let html = img.email_html("https://example.com");
        assert!(html.contains(r#"alt="$a_i$""#), "alt must be delimited source: {html}");
        assert!(html.contains(r#"role="img""#));
        assert!(html.contains(&format!(r#"width="{}""#, img.width_1x)));
        assert!(html.contains(&format!(r#"height="{}""#, img.height_1x)));
        assert!(html.contains("max-width:100%"));
        assert!(
            html.contains("vertical-align:-"),
            "a_i has a descender; inline math must baseline-align: {html}"
        );
        assert!(!html.contains("display:block"));
    }

    #[test]
    fn display_img_is_block_centered_without_valign() {
        let html = email_math_html(r"\sum_{i=1}^n x_i", true, "https://example.com").unwrap();
        assert!(html.contains(r#"alt="$$\sum_{i=1}^n x_i$$""#), "got {html}");
        assert!(html.contains("display:block"));
        assert!(html.contains("margin:16px auto"));
        assert!(!html.contains("vertical-align"));
    }

    #[test]
    fn refused_math_gets_no_img() {
        // Emoji is a permanent refusal (blank render) → never an <img>.
        assert!(email_math_html("🎉", false, "https://example.com").is_none());
        // CJK upgrades to <img> only when a verified font is pinned (§3.6);
        // otherwise it degrades to source too. Whichever holds on this machine,
        // email_math_html must agree with email_math_img's eligibility.
        let cjk = email_math_html(r"\text{线性}", false, "https://example.com");
        assert_eq!(cjk.is_some(), email_math_img(r"\text{线性}", false).is_ok());
    }

    #[test]
    fn empty_site_url_yields_root_relative_src() {
        let html = email_math_html("x+y", false, "").unwrap();
        assert!(html.contains(r#"src="/_moss/math/"#), "got {html}");
    }

    // ---- Rasterization ----

    #[test]
    fn emit_writes_a_2x_opaque_png_and_registers_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut pending = PendingManifest::new(crate::types::content::SiteHashes::default());
        let docs = vec![doc_with("Einstein: $E=mc^2$ and\n\n$$\\sum_{i=1}^n x_i$$\n")];
        emit_math_pngs(&docs, true, dir.path(), &mut pending).unwrap();

        let img = email_math_img("E=mc^2", false).unwrap();
        let disk = img.served_path().to_disk(dir.path());
        let bytes = std::fs::read(&disk).expect("png must exist");
        assert!(!bytes.is_empty());
        assert_eq!(&bytes[..8], &[137, 80, 78, 71, 13, 10, 26, 10], "PNG signature");

        let pixmap = tiny_skia::Pixmap::decode_png(&bytes).expect("decode png");
        assert_eq!(pixmap.width(), img.width_1x * 2, "raster is 2× the width attr");
        assert_eq!(pixmap.height(), img.height_1x * 2, "raster is 2× the height attr");

        // Corner pixel = opaque light background (dark-mode safety floor).
        let px = pixmap.pixel(0, 0).unwrap();
        assert_eq!(px.alpha(), 255, "background must be opaque");
        assert!(px.red() > 200 && px.green() > 200 && px.blue() > 200, "background must be light");

        // Ink present: some pixel differs from the background.
        let has_ink = pixmap.pixels().iter().any(|p| p.red() < 128);
        assert!(has_ink, "glyphs must actually rasterize");

        // Registered in the manifest so it ships + deploys.
        assert!(
            pending.files().contains_key(img.served_path().as_str()),
            "math png must be registered in the manifest"
        );
        // Display equation landed too.
        let disp = email_math_img(r"\sum_{i=1}^n x_i", true).unwrap();
        assert!(disp.served_path().to_disk(dir.path()).exists());
    }

    /// An unreadable math PNG must keep its manifest entry, not lose it.
    ///
    /// These URLs are already in people's inboxes: ADR-030 §3.4 makes the
    /// directory append-only precisely so a published `<img>` never 404s. The
    /// walk used to `continue` past a file it could not read, which silently
    /// dropped the entry and let the next publish delete the URL from the live
    /// site. The hash is in the previous manifest and the file is
    /// content-addressed, so the entry still describes these exact bytes.
    #[cfg(unix)]
    #[test]
    fn an_unreadable_math_png_keeps_its_previous_entry() {
        let dir = tempfile::tempdir().unwrap();
        let docs = vec![doc_with("Einstein: $E=mc^2$\n")];

        // Build one: writes and registers the PNG.
        let mut first = PendingManifest::new(crate::types::content::SiteHashes::default());
        emit_math_pngs(&docs, true, dir.path(), &mut first).unwrap();
        let img = email_math_img("E=mc^2", false).unwrap();
        let key = img.served_path().as_str().to_string();
        let sealed = first.seal();
        let previous_entry = sealed.files().get(&key).expect("registered by build one").clone();

        // Build two: the output is present as far as `output_present` is
        // concerned — so the rasterize step skips it as already built — but
        // reading it fails. A dangling symlink is the stand-in: a link is
        // present by the link (that is the `120000:` arm), `read` follows it
        // and gets ENOENT, and neither fact depends on the test process's
        // uid the way a `chmod 000` would. A directory would not do: C1
        // classifies one as absent, so emit would correctly regenerate and
        // the carry arm under test would never run.
        let disk = img.served_path().to_disk(dir.path());
        std::fs::remove_file(&disk).unwrap();
        std::os::unix::fs::symlink(dir.path().join("no-such-file.png"), &disk).unwrap();
        let mut second = PendingManifest::new(crate::types::content::SiteHashes {
            files: sealed.files().clone(),
            ..Default::default()
        });
        emit_math_pngs(&docs, true, dir.path(), &mut second).unwrap();

        assert_eq!(
            second.seal().files().get(&key),
            Some(&previous_entry),
            "the entry must be carried forward verbatim, not dropped"
        );
    }

    #[test]
    fn cjk_math_emits_a_file_only_when_a_font_is_pinned() {
        let dir = tempfile::tempdir().unwrap();
        let mut pending = PendingManifest::new(crate::types::content::SiteHashes::default());
        let docs = vec![doc_with("CJK: $\\text{线性注意力}$\n")];
        emit_math_pngs(&docs, true, dir.path(), &mut pending).unwrap();
        let math_dir = dir.path().join("_moss").join("math");
        // Typeset when a verified font is pinned → a PNG; degraded otherwise →
        // no PNG (§3.6). The emit outcome tracks render eligibility.
        match email_math_img(r"\text{线性注意力}", false) {
            Ok(_) => assert!(math_dir.exists(), "typeset CJK must produce a PNG"),
            Err(_) => assert!(!math_dir.exists(), "degraded CJK must produce no PNG"),
        }
    }

    #[test]
    fn math_disabled_rasterizes_nothing_but_still_retains() {
        let dir = tempfile::tempdir().unwrap();
        // A PNG from an earlier build (site later flipped [site].math=false).
        let old = dir.path().join("_moss").join("math");
        std::fs::create_dir_all(&old).unwrap();
        let old_png = old.join("aaaaaaaaaaaaaaaa.png");
        std::fs::write(&old_png, b"\x89PNG old bytes").unwrap();

        let mut pending = PendingManifest::new(crate::types::content::SiteHashes::default());
        let docs = vec![doc_with("now math-free, but $x$ would have typeset")];
        emit_math_pngs(&docs, false, dir.path(), &mut pending).unwrap();

        assert!(old_png.exists(), "retention must keep the old PNG");
        assert!(
            pending.files().contains_key("_moss/math/aaaaaaaaaaaaaaaa.png"),
            "old PNG must still be registered so it ships in this generation"
        );
        // And nothing new was rasterized.
        assert_eq!(std::fs::read_dir(&old).unwrap().count(), 1);
    }

    #[test]
    fn retention_reregisters_pngs_from_earlier_builds() {
        let dir = tempfile::tempdir().unwrap();

        // Build 1: an equation exists.
        let mut pending1 = PendingManifest::new(crate::types::content::SiteHashes::default());
        emit_math_pngs(&[doc_with("$E=mc^2$")], true, dir.path(), &mut pending1).unwrap();
        let hash = content_hash("E=mc^2", false);
        let key = format!("_moss/math/{hash}.png");
        assert!(pending1.files().contains_key(&key));

        // Build 2: the equation was deleted from every page. The PNG's URL is
        // in sent inboxes — it must stay on disk AND in the new manifest.
        let mut pending2 = PendingManifest::new(crate::types::content::SiteHashes::default());
        emit_math_pngs(&[doc_with("no math left")], true, dir.path(), &mut pending2).unwrap();
        assert!(
            dir.path().join(&key).exists(),
            "append-only: the PNG survives the equation's deletion"
        );
        assert!(
            pending2.files().contains_key(&key),
            "append-only: the PNG is still promised by the new generation"
        );
    }

    #[test]
    fn emit_is_idempotent_byte_for_byte() {
        let dir = tempfile::tempdir().unwrap();
        let docs = vec![doc_with("$\\frac{a}{b}$")];
        let mut p1 = PendingManifest::new(crate::types::content::SiteHashes::default());
        emit_math_pngs(&docs, true, dir.path(), &mut p1).unwrap();
        let disk = email_math_img(r"\frac{a}{b}", false).unwrap().served_path().to_disk(dir.path());
        let first = std::fs::read(&disk).unwrap();
        let mtime1 = std::fs::metadata(&disk).unwrap().modified().unwrap();

        std::thread::sleep(std::time::Duration::from_millis(20));
        let mut p2 = PendingManifest::new(crate::types::content::SiteHashes::default());
        emit_math_pngs(&docs, true, dir.path(), &mut p2).unwrap();
        assert_eq!(std::fs::read(&disk).unwrap(), first, "bytes must not churn");
        assert_eq!(
            std::fs::metadata(&disk).unwrap().modified().unwrap(),
            mtime1,
            "existing file must be reused, not rewritten"
        );
    }

    // ---- Guard helper ----

    #[test]
    fn typeset_math_hashes_excludes_refused_math() {
        let md = "ok: $E=mc^2$, cjk: $\\text{线性}$, display: $$a+b$$";
        let hashes = typeset_math_hashes(md);
        // E=mc^2 and a+b always typeset; the CJK term references a PNG only when
        // a verified font is pinned (§3.6), else it degrades to source.
        let cjk_eligible = email_math_img(r"\text{线性}", false).is_ok() as usize;
        assert_eq!(
            hashes.len(),
            2 + cjk_eligible,
            "only typeset-eligible equations reference PNGs"
        );
        assert!(hashes.contains(&content_hash("E=mc^2", false)));
        assert!(hashes.contains(&content_hash("a+b", true)));
    }

    #[test]
    fn typeset_math_hashes_empty_for_math_free_body() {
        assert!(typeset_math_hashes("no math here").is_empty());
    }

    // ---- RSS swap ----

    #[test]
    fn swap_replaces_inline_and_display_svgs_with_absolute_imgs() {
        use crate::build::markdown::math::render_math;
        let inline = render_math("a_i", false).unwrap();
        let display = render_math(r"\sum_{i=1}^n x_i", true).unwrap();
        let html = format!("<p>before {inline} middle</p>{display}<p>after</p>");

        let out = swap_math_svgs_for_email_imgs(&html, "https://example.com");
        assert!(!out.contains("<svg"), "feed readers sanitize svg — none may remain: {out}");
        assert!(!out.contains("moss-math-scroll"), "display wrapper must go too: {out}");
        assert!(out.contains(&format!(
            "https://example.com/_moss/math/{}.png",
            content_hash("a_i", false)
        )));
        assert!(out.contains(&format!(
            "https://example.com/_moss/math/{}.png",
            content_hash(r"\sum_{i=1}^n x_i", true)
        )));
        assert!(out.contains("<p>before "), "surrounding html preserved");
        assert!(out.contains("<p>after</p>"));
    }

    #[test]
    fn swap_round_trips_escaped_tex_in_aria_label() {
        use crate::build::markdown::math::render_math;
        // `<` and `>` escape into aria-label and must round-trip so the
        // hash matches the one the build emitted for the raw source.
        let tex = r"a<b>c";
        let svg = render_math(tex, false).unwrap();
        let out = swap_math_svgs_for_email_imgs(&svg, "https://example.com");
        let expected_url = ServedPath::for_math_png(&content_hash(tex, false))
            .unwrap()
            .to_relative_url();
        assert!(
            out.contains(&expected_url),
            "unescaped aria-label must reproduce the build-side hash: {out}"
        );
    }

    #[test]
    fn unescape_attr_inverts_escape_attr_including_double_escapes() {
        for s in [r"a<b>c&d", r#"say "hi" & 'bye'"#, "&lt; literal entity text"] {
            assert_eq!(unescape_attr(&escape_attr(s)), s, "round-trip failed for {s:?}");
        }
    }

    #[test]
    fn swap_leaves_refused_math_code_nodes_and_other_svgs_alone() {
        let html = r#"<p><code class="moss-math">$\text{线性}$</code></p><svg viewBox="0 0 1 1"><path d="M0 0"/></svg>"#;
        let out = swap_math_svgs_for_email_imgs(html, "https://example.com");
        assert_eq!(out, html, "non-math svg + refused-math code must pass through");
    }

    #[test]
    fn swap_handles_math_free_html_unchanged() {
        let html = "<p>hello</p>";
        assert_eq!(swap_math_svgs_for_email_imgs(html, "https://x.com"), html);
    }
}
