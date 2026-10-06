//! CJK-in-math font verification + pinning.
//!
//! RaTeX's `ratex-unicode-font` resolves a CJK glyph font by **silently
//! probing system fonts** and caching the result in a process-global
//! `OnceLock` on the *first* render. A probed-on-first-render font is
//! non-deterministic across machines (Linux CI has no Arial Unicode) and moss
//! cannot know which font produced a given equation's bytes. This module takes
//! that decision away from the engine:
//!
//! 1. **Probe** a fixed candidate list (user override first), in priority order.
//! 2. **Verify** each candidate parses as a real font (`ttf-parser`).
//! 3. **Pin** the first that verifies via `RATEX_UNICODE_FONT`, set exactly
//!    once (serialized by [`pinned`]'s `OnceLock`) BEFORE any render reads the
//!    var, so `ratex-unicode-font` caches *our* choice.
//! 4. **Hash** the whole font file (sha256) so the content address
//!    (`build::emit::math_png`) changes when the pinned font drifts across
//!    machines — a different URL, never different bytes at a cached URL.
//! 5. **cmap coverage** ([`covers_all_cjk`]): before an equation carrying CJK
//!    is rendered, confirm the pinned face's cmap covers *every* CJK codepoint
//!    in it — on a miss, refuse that equation to P1 source (never a tofu glyph).
//!
//! No font available / no coverage → the equation refuses to P1 source, exactly
//! as v1 did. The crash-prevention envelope in the parent module is unchanged:
//! CJK equations traverse the SAME guard → 16 MiB thread → `validate_svg` path.

use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

/// A verified, pinned system CJK font. Constructed only by [`probe_and_verify`]
/// after the file reads and `ttf-parser` accepts it; the raw bytes are retained
/// so per-equation cmap coverage ([`covers_all_cjk`]) needs no re-read.
pub struct PinnedFont {
    #[allow(dead_code)]
    path: PathBuf,
    face_index: u32,
    sha256: String,
    bytes: Arc<Vec<u8>>,
}

impl PinnedFont {
    /// The sha256 (64 lowercase hex) of the whole font file — the component the
    /// content address threads in so cross-machine font drift re-keys the URL.
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

/// The CJK codepoint ranges moss now permits inside math when a verified font
/// is pinned. Kept SEPARATE from `is_allowed_codepoint` so the guard's other
/// refusals (emoji, RTL, combining marks, control) stay independent — this
/// predicate only recognizes the blocks the pinned face is expected to cover.
pub fn needs_cjk_font(ch: char) -> bool {
    matches!(ch as u32,
        0x3000..=0x303F     // CJK Symbols and Punctuation
        | 0x3040..=0x30FF   // Hiragana + Katakana
        | 0x3400..=0x4DBF   // CJK Unified Ideographs Extension A
        | 0x4E00..=0x9FFF   // CJK Unified Ideographs
        | 0xF900..=0xFAFF   // CJK Compatibility Ideographs
        | 0xFF00..=0xFFEF   // Halfwidth and Fullwidth Forms
        | 0x1100..=0x11FF   // Hangul Jamo
        | 0xAC00..=0xD7AF   // Hangul Syllables
        | 0x20000..=0x2A6DF // CJK Unified Ideographs Extension B
    )
}

impl PinnedFont {
    /// Parse the pinned face ONCE and confirm its cmap covers every CJK
    /// codepoint in `tex`. Non-CJK codepoints (Latin/Greek/symbols) are the
    /// embedded-KaTeX font's job and are ignored here. Returns `true` when the
    /// face is unparseable-nothing-to-cover only if there are no CJK chars; a
    /// parse failure with CJK present returns `false` (refuse to source).
    pub fn covers_all_cjk(&self, tex: &str) -> bool {
        let face = match ttf_parser::Face::parse(&self.bytes, self.face_index) {
            Ok(f) => f,
            Err(_) => {
                // A face that verified at probe time but fails to re-parse: only
                // a problem if the equation actually needs CJK coverage.
                return !tex.chars().any(needs_cjk_font);
            }
        };
        tex.chars()
            .filter(|&c| needs_cjk_font(c))
            .all(|c| face.glyph_index(c).is_some())
    }
}

/// Candidate font paths in priority order: the `MOSS_MATH_CJK_FONT` override
/// first, then the macOS system CJK fonts, then common Linux Noto CJK paths.
/// All faces are index 0 (the override too); the `.ttc` collections' first face
/// is the Regular weight moss wants.
fn candidates() -> Vec<(PathBuf, u32)> {
    let mut out = Vec::new();
    if let Ok(p) = std::env::var("MOSS_MATH_CJK_FONT") {
        if !p.is_empty() {
            out.push((PathBuf::from(p), 0));
        }
    }
    for p in [
        "/Library/Fonts/Arial Unicode.ttf",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
    ] {
        out.push((PathBuf::from(p), 0));
    }
    out
}

/// Probe the candidate list and return the first font whose file reads AND
/// parses with `ttf-parser`. The sha256 is over the WHOLE file bytes (part of
/// the content address). Pure I/O — reads font files; no engine calls.
pub fn probe_and_verify() -> Option<PinnedFont> {
    for (path, face_index) in candidates() {
        let bytes = match std::fs::read(&path) {
            Ok(b) if !b.is_empty() => b,
            _ => continue,
        };
        if ttf_parser::Face::parse(&bytes, face_index).is_err() {
            continue;
        }
        let mut h = Sha256::new();
        h.update(&bytes);
        let sha256 = h.finalize().iter().map(|b| format!("{:02x}", b)).collect();
        return Some(PinnedFont {
            path,
            face_index,
            sha256,
            bytes: Arc::new(bytes),
        });
    }
    None
}

/// The process-global verified+pinned CJK font, or `None` when none is
/// available (→ CJK math degrades to P1 source). Resolved exactly once; on the
/// first `Some`, `RATEX_UNICODE_FONT` is set to the font path BEFORE this
/// returns, so the first ratex render caches *our* choice, not a probed one.
/// The `OnceLock` serializes the set — it runs before any render reads the var.
///
/// INVARIANT: this process-global pin is
/// valid ONLY while font choice is process-invariant. moss is a long-running
/// process building many sites, and math content addresses are permanent (inbox
/// proxies cache the PNGs forever). The day font selection becomes per-site or
/// per-invocation, this pin (and its sha256 fed into `content_hash`) MUST move
/// into the per-invocation build Config — otherwise this cache freezes build
/// #1's font for every later site and mints stale addresses.
pub fn pinned() -> Option<&'static PinnedFont> {
    static PINNED: OnceLock<Option<PinnedFont>> = OnceLock::new();
    PINNED
        .get_or_init(|| {
            let font = probe_and_verify();
            if let Some(f) = &font {
                // Edition 2021: `set_var` is not `unsafe`. This runs once,
                // serialized by the OnceLock, before the first ratex render
                // reads the var — so `ratex-unicode-font`'s own OnceLock caches
                // the font moss verified.
                std::env::set_var("RATEX_UNICODE_FONT", &f.path);
            }
            font
        })
        .as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needs_cjk_font_classifies_cjk_but_not_math() {
        // CJK ideographs + fullwidth punctuation need the pinned font.
        assert!(needs_cjk_font('线'));
        assert!(needs_cjk_font('。')); // fullwidth ideographic period (0x3002)
        assert!(needs_cjk_font('注'));
        assert!(needs_cjk_font('あ')); // hiragana
        // KaTeX-covered math codepoints do NOT.
        assert!(!needs_cjk_font('x'));
        assert!(!needs_cjk_font('='));
        assert!(!needs_cjk_font('α'));
        assert!(!needs_cjk_font('2'));
        assert!(!needs_cjk_font('∑'));
    }

    #[test]
    fn probe_verify_and_cover_when_a_font_is_present() {
        let Some(font) = probe_and_verify() else {
            eprintln!("skip: no system CJK font on this machine — CJK math degrades to source");
            return;
        };
        // sha256 of the whole file: 64 lowercase hex.
        assert_eq!(font.sha256().len(), 64);
        assert!(font.sha256().chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));

        // Deterministic across two probes on the same machine.
        let again = probe_and_verify().expect("second probe must also find a font");
        assert_eq!(font.sha256(), again.sha256(), "same machine ⇒ same font sha256");

        // Covers the reported live equation's glyphs.
        assert!(font.covers_all_cjk("线性注意力"), "pinned font must cover the live CJK");
        // Pure-ASCII math has no CJK to cover ⇒ trivially true.
        assert!(font.covers_all_cjk("x^2"));
    }
}
