//! LaTeX → inline SVG: the single, defended gateway to RaTeX.
//!
//! P1 renders every `$…$` / `$$…$$` as its own escaped LaTeX source in a
//! `<code class="moss-math">` node — honest, never blank. P2 (this module)
//! *typesets* that source to an inline SVG with baked glyph `<path>`s, so the
//! reader sees the equation instead of the source. Anything that cannot be
//! typeset **safely** falls back to the P1 node: never blank, never a crash.
//!
//! ## Why this is the only caller of RaTeX, and why it is so defensive
//!
//! The 2026-07-21 robustness spike (see
//! `tests/fixtures/math-fuzz-corpus/README.md`) found two *uncatchable* stack
//! overflows in RaTeX 0.1.13 — combining-mark recursion in `layout_accent`
//! ([#143]) and a stack-blind depth guard ([#144]) that aborts on the 2 MiB
//! stacks rayon build workers run with. moss builds under `panic = "abort"`,
//! so a stack overflow does not unwind — it **aborts the whole app**
//! (`catch_unwind` is proven useless here, exit 134). The defense is layered
//! and every layer is load-bearing:
//!
//! - **A — input envelope** ([`guard`]): length ≤ [`MAX_TEX_BYTES`], nesting ≤
//!   [`MAX_NESTING`], and a codepoint allowlist that rejects the crash- and
//!   blank-inducing inputs *before RaTeX sees a byte*. Alone this closes T1/T2.
//! - **B — output validation** ([`validate_svg`]): the emitted SVG must parse
//!   to a rooted `<svg>` with a sane `viewBox`, stay under a byte cap, carry at
//!   least one glyph, and contain **zero** `<text>`/`<use>`/`<image>`/`id=`.
//!   The last is the engine-drift tripwire: a mis-built RaTeX (wrong features)
//!   ships `<text>`, which resvg would later rasterize blank.
//! - **C — 16 MiB spawn-join thread** ([`render_on_big_stack`]): every render
//!   runs on an explicit-stack thread spawned from inside the rayon worker, so
//!   the depth at which #144 would abort moves ~60× past what the length cap
//!   admits. RaTeX has zero rayon deps, so there is no worker-nesting hazard.
//! - **D — fallback**: every [`MathRefusal`] returns to the caller, which emits
//!   the P1 escaped-source node. The floor. Ships in P1, exercised from day one.
//!
//! ## CJK inside math — verify + pin a system font
//!
//! CJK is no longer a blanket refusal. When a system CJK font is present and
//! **verified**, [`mod@font`] pins it via `RATEX_UNICODE_FONT` (once, before the
//! first render) and hashes it into the content address; CJK equations then
//! traverse the SAME guard → 16 MiB thread → `validate_svg` path as Latin math
//! (the probe confirmed CJK renders to path-only outlines that `validate_svg`
//! accepts unchanged). When no font is verified, or the pinned font's cmap does
//! not cover an equation's CJK, that equation refuses to P1 source — the v1
//! floor. Emoji, RTL/unshaped scripts, combining marks and control chars stay
//! refused regardless: those blocks are outside [`font::needs_cjk_font`].
//!
//! [#143]: https://github.com/erweixin/RaTeX/issues/143
//! [#144]: https://github.com/erweixin/RaTeX/issues/144

pub(crate) mod font;

/// Hard cap on the LaTeX source length, in **bytes**. The combining-mark abort
/// ([#143]) needs ~4,173 marks on an 8 MiB stack; at 2 bytes/mark that is
/// ~8 KiB, and the 16 MiB render thread (layer C) pushes the abort threshold
/// several× higher again — so 4 KiB sits comfortably below it with the thread,
/// and still bounds worst-case output amplification (~25 MB RSS). Real
/// equations are tens of bytes; the linear-attention article's longest is 78.
const MAX_TEX_BYTES: usize = 4096;

/// Hard cap on bracket / `\left`-`\right` / `\begin`-`\end` nesting depth. The
/// shallowest measured stack-overflow was `\sqrt` at depth 255 on a 2 MiB
/// stack (`\frac` at 285) — see the fuzz corpus `bisect.py`. 32 is an 8× margin
/// *before* the 16 MiB thread, which moves the real failure point past ~2,000.
/// No legitimate equation nests brackets 32 deep.
const MAX_NESTING: usize = 32;

/// Stack size for the render thread (layer C). RaTeX renders the spike's
/// deepest admissible input in well under this; a 2 MiB (Rust default spawned-
/// thread) stack aborts on inputs an 8 MiB stack renders fine (#144), so we
/// give it 16 MiB and let the length + nesting caps bound the rest.
const RENDER_STACK_BYTES: usize = 16 * 1024 * 1024;

/// RaTeX user-units per em. The layout metrics ([`RawRender`]) are already in
/// em and font-size-independent, so this value only scales the SVG's internal
/// coordinate space; the element is sized in `em` via inline `style`, so any
/// positive constant renders identically. 40 matches the spike's proportions.
const RENDER_FONT_SIZE: f64 = 40.0;

/// Byte cap on the emitted SVG (layer B). Guards the `\rule{99999999pt}` /
/// deep-matrix amplification class. The article's largest display SVG is ~40 KB;
/// 512 KiB is generous headroom that still refuses a pathological blow-up.
const MAX_SVG_BYTES: usize = 512 * 1024;

/// `viewBox` dimension sanity cap, in RaTeX user units (layer B). A normal
/// equation is a few hundred units wide; `\rule` amplification produces
/// millions. 200,000 units ≈ 5,000 em, far beyond any real equation.
const MAX_VIEWBOX_DIM: f64 = 200_000.0;

/// Why a render was refused. Every variant routes the caller to the P1
/// escaped-source fallback — never blank, never abort. `#[non_exhaustive]`-free
/// on purpose: this is an internal type, matched only within this crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MathRefusal {
    /// Source exceeded [`MAX_TEX_BYTES`].
    TooLong { bytes: usize },
    /// Bracket / group nesting exceeded [`MAX_NESTING`].
    NestingTooDeep { depth: usize },
    /// A macro that can expand unboundedly or read the filesystem
    /// (`\csname`, `\def`, `\input`, …) — rejected before the engine.
    DangerousMacro { macro_name: &'static str },
    /// A combining mark (the [#143] recursion class).
    CombiningMark,
    /// A codepoint outside the KaTeX-covered allowlist AND outside the CJK
    /// blocks: emoji, RTL, control characters, unshaped scripts. Carries the
    /// offending codepoint for `moss doctor --math`.
    DisallowedCodepoint { codepoint: u32 },
    /// The equation carries CJK but no system CJK font is verified/pinned on
    /// this machine (§3.6 "else degrade"). Refuse to P1 source, as v1 did.
    NoCjkFont,
    /// A CJK codepoint the pinned font's cmap does not cover — refuse to P1
    /// source rather than emit a tofu/blank glyph. Carries the first uncovered
    /// codepoint for `moss doctor --math`.
    CjkGlyphMissing { codepoint: u32 },
    /// CJK in *bare math mode* — not inside a `\text{}`-family group. Almost
    /// always an accidental `$…$` wrapping CJK prose (the currency false
    /// positive `一个$5，两个$10。`, where `$…$` spans `5，两个`); refuse so it
    /// stays literal source instead of typesetting into a nonsensical image.
    /// Deliberate CJK math is written `\text{…}`, which is permitted.
    CjkOutsideTextMode { codepoint: u32 },
    /// RaTeX's parser rejected the source.
    ParseError,
    /// The render thread aborted or panicked (should be unreachable behind the
    /// guard; the join is the last-ditch net).
    RenderFailed,
    /// The emitted SVG failed [`validate_svg`] (engine-drift tripwire, blank
    /// output, or output amplification).
    InvalidOutput { reason: &'static str },
}

impl MathRefusal {
    /// Whether this refusal is a *surprising* engine/output failure worth a
    /// build warning, versus an *expected* guard rejection (CJK currency false
    /// positive, deliberately-adversarial input) that `moss doctor --math`
    /// already surfaces and that would only spam a normal build's log.
    pub fn is_unexpected(&self) -> bool {
        matches!(
            self,
            MathRefusal::ParseError | MathRefusal::RenderFailed | MathRefusal::InvalidOutput { .. }
        )
    }
}

/// Typeset `tex` to a ready-to-emit HTML string, or refuse.
///
/// On success: an inline `<svg role="img" class="moss-math" …>` (display math
/// additionally wrapped in `<div class="moss-math-scroll">`). On any refusal,
/// the caller emits the P1 fallback. This is the whole public surface — the one
/// place RaTeX is reached, with the guard/thread/validation envelope around it.
pub fn render_math(tex: &str, display: bool) -> Result<String, MathRefusal> {
    let raw = typeset(tex)?;
    Ok(assemble(&raw, tex, display))
}

/// A validated raw typeset: the engine's SVG plus the layout metrics, before
/// any surface-specific assembly. The email/RSS PNG projection
/// (`build::emit::math_png`) rasterizes exactly these bytes so the equation a
/// feed reader or inbox shows is the one the website inlines.
pub struct TypesetMath {
    /// The engine's raw SVG, already past [`validate_svg`] (path-only, sane
    /// viewBox, byte-capped). Root carries `width`/`height` in pt + `viewBox`.
    pub svg: String,
    /// Ink width in em.
    pub width_em: f64,
    /// Ascent above the baseline, in em.
    pub height_em: f64,
    /// Descent below the baseline, in em. Drives `vertical-align` on both the
    /// web `<svg>` and the email `<img>` (px at 1× there).
    pub depth_em: f64,
}

/// Guard → render → validate, returning the raw SVG + metrics instead of the
/// assembled web element. The full crash-prevention envelope applies — this is
/// NOT a bypass: every layer (`guard`, big-stack thread, `validate_svg`) runs
/// exactly as in [`render_math`]; only the final surface assembly differs.
pub fn typeset(tex: &str) -> Result<TypesetMath, MathRefusal> {
    guard(tex)?;
    let raw = render_on_big_stack(tex)?;
    validate_svg(&raw.svg)?;
    Ok(TypesetMath {
        svg: raw.svg,
        width_em: raw.width,
        height_em: raw.height,
        depth_em: raw.depth,
    })
}

/// Layer A — the input envelope. Reject, before RaTeX sees a byte, everything
/// the spike proved can abort the process or render blank. Iterates
/// `char_indices` — **never** byte-indexes (the multibyte-slicing crash class).
fn guard(tex: &str) -> Result<(), MathRefusal> {
    if tex.len() > MAX_TEX_BYTES {
        return Err(MathRefusal::TooLong { bytes: tex.len() });
    }

    // Filesystem / unbounded-expansion macros. Substring match is deliberately
    // broad: these never appear in a legitimate inline equation.
    const DANGEROUS: &[&str] = &[
        "\\csname",
        "\\def",
        "\\newcommand",
        "\\renewcommand",
        "\\input",
        "\\include",
        "\\expandafter",
        "\\catcode",
        "\\write",
        "\\openin",
        "\\read",
    ];
    for m in DANGEROUS {
        if tex.contains(m) {
            return Err(MathRefusal::DangerousMacro { macro_name: m });
        }
    }

    let mut has_cjk = false;
    for ch in tex.chars() {
        if is_combining_mark(ch) {
            return Err(MathRefusal::CombiningMark);
        }
        if is_allowed_codepoint(ch) {
            continue;
        }
        // CJK is now permitted — but only when a verified font is pinned AND it
        // covers the glyph (checked once, after the loop). Defer the decision so
        // the emoji/RTL/control refusal below stays independent of font state.
        if font::needs_cjk_font(ch) {
            has_cjk = true;
            continue;
        }
        return Err(MathRefusal::DisallowedCodepoint {
            codepoint: ch as u32,
        });
    }

    // CJK present. First: it must be inside a `\text{}`-family group (deliberate
    // CJK math). Bare CJK in math mode is almost always an accidental `$…$`
    // around prose — the currency false positive `$5，两个$` — so refuse it to P1
    // source rather than typeset a nonsensical image. THEN: require a
    // verified+pinned font whose cmap covers every CJK codepoint; absence or a
    // miss also degrades to P1 source (§3.6).
    if has_cjk {
        if let Some(cp) = first_cjk_outside_text_mode(tex) {
            return Err(MathRefusal::CjkOutsideTextMode { codepoint: cp });
        }
        match font::pinned() {
            None => return Err(MathRefusal::NoCjkFont),
            Some(f) if !f.covers_all_cjk(tex) => {
                let first = tex
                    .chars()
                    .find(|&c| font::needs_cjk_font(c) && !f.covers_all_cjk(&c.to_string()))
                    .map(|c| c as u32)
                    .unwrap_or(0);
                return Err(MathRefusal::CjkGlyphMissing { codepoint: first });
            }
            _ => {}
        }
    }

    let depth = max_nesting_depth(tex);
    if depth > MAX_NESTING {
        return Err(MathRefusal::NestingTooDeep { depth });
    }

    Ok(())
}

/// `\text{}`-family commands whose braced argument is *text mode* — the only
/// place CJK is permitted. CJK anywhere else is bare math mode.
const TEXT_MODE_CMDS: &[&str] = &[
    "\\text",
    "\\textnormal",
    "\\textrm",
    "\\textbf",
    "\\textit",
    "\\textsf",
    "\\texttt",
    "\\mathrm",
    "\\mathbf",
    "\\mathit",
    "\\mathsf",
    "\\mathtt",
    "\\mathnormal",
    "\\operatorname",
];

/// The first CJK codepoint NOT inside a `\text{}`-family group, or `None` if
/// every CJK char is in text mode. Deliberate CJK math nests its ideographs in
/// `\text{…}`; an accidental `$…$` around CJK prose (the currency false
/// positive `$5，两个$`) has bare CJK and is refused. Scans `char_indices` so a
/// multibyte codepoint never splits a control-word match.
fn first_cjk_outside_text_mode(tex: &str) -> Option<u32> {
    // Per open brace: is this group text mode — its own `\text…` command, or
    // inherited from an enclosing text-mode group?
    let mut stack: Vec<bool> = Vec::new();
    let mut next_brace_text = false;
    for (idx, ch) in tex.char_indices() {
        match ch {
            '\\' => {
                let rest = tex.get(idx..).unwrap_or_default();
                if TEXT_MODE_CMDS.iter().any(|cmd| {
                    rest.strip_prefix(cmd).is_some_and(|after| {
                        !after.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                    })
                }) {
                    next_brace_text = true;
                }
            }
            '{' => {
                let inherited = *stack.last().unwrap_or(&false);
                stack.push(next_brace_text || inherited);
                next_brace_text = false;
            }
            '}' => {
                stack.pop();
            }
            _ if font::needs_cjk_font(ch) => {
                if !*stack.last().unwrap_or(&false) {
                    return Some(ch as u32);
                }
            }
            _ => {}
        }
    }
    None
}

/// Maximum simultaneous nesting depth across bracket groups (`{` `[`),
/// `\left`…`\right`, and `\begin`…`\end`. Scans `char_indices` so multibyte
/// codepoints never split a match. A single running counter over all three
/// families is intentional: they compound the same recursive layout descent.
fn max_nesting_depth(tex: &str) -> usize {
    let bytes = tex.as_bytes();
    let mut depth: usize = 0;
    let mut max: usize = 0;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'{' | b'[' => {
                depth += 1;
                max = max.max(depth);
                i += 1;
            }
            b'}' | b']' => {
                depth = depth.saturating_sub(1);
                i += 1;
            }
            b'\\' => {
                // A backslash byte only occurs at a char boundary, so the tail
                // from here is always a valid `&str`.
                let rest = tex.get(i..).unwrap_or_default();
                if rest.starts_with("\\left") || rest.starts_with("\\begin") {
                    depth += 1;
                    max = max.max(depth);
                } else if rest.starts_with("\\right") || rest.starts_with("\\end") {
                    depth = depth.saturating_sub(1);
                }
                // Skip the backslash; the following control-word letters are
                // ordinary allowlisted chars, so advancing by 1 is safe and
                // keeps the scan O(n).
                i += 1;
            }
            _ => i += 1,
        }
    }
    max
}

/// Combining marks — the [#143] `layout_accent` unbounded-recursion class. A
/// run of these on a base glyph recurses per mark; the guard rejects any.
fn is_combining_mark(ch: char) -> bool {
    matches!(ch as u32,
        0x0300..=0x036F   // Combining Diacritical Marks
        | 0x1AB0..=0x1AFF // Combining Diacritical Marks Extended
        | 0x1DC0..=0x1DFF // Combining Diacritical Marks Supplement
        | 0x20D0..=0x20FF // Combining Diacritical Marks for Symbols
        | 0xFE20..=0xFE2F // Combining Half Marks
    )
}

/// The codepoint allowlist: `true` for codepoints RaTeX's *embedded* KaTeX
/// fonts cover (Latin, Greek, and the math symbol blocks), `false` for
/// everything else — CJK, emoji, RTL/unshaped scripts, control characters,
/// private-use, surrogates. Rejecting the rest is what keeps RaTeX off its
/// silent system-font probe (§3.6) and off the emoji→blank path; the spike
/// verified `\alpha`, literal `α`, `∑`, `×`, `≤` all render from the embedded
/// outlines, while `🎉` renders blank and `线性` triggers the probe.
fn is_allowed_codepoint(ch: char) -> bool {
    let c = ch as u32;
    // Whitespace RaTeX tolerates in source.
    if ch == '\t' || ch == '\n' || ch == '\r' || ch == ' ' {
        return true;
    }
    matches!(c,
        0x0021..=0x007E   // ASCII printable
        | 0x00A0..=0x00FF // Latin-1 Supplement (± × ÷, accented Latin)
        | 0x0370..=0x03FF // Greek and Coptic
        | 0x2000..=0x206F // General Punctuation (primes, spaces, dashes)
        | 0x2070..=0x209F // Superscripts and Subscripts
        | 0x20A0..=0x20BF // Currency Symbols
        | 0x2100..=0x214F // Letterlike Symbols (ℓ ℏ ℝ ℕ …)
        | 0x2150..=0x218F // Number Forms
        | 0x2190..=0x21FF // Arrows
        | 0x2200..=0x22FF // Mathematical Operators
        | 0x2300..=0x23FF // Miscellaneous Technical (⌈⌉⌊⌋ …)
        | 0x25A0..=0x25FF // Geometric Shapes
        | 0x2600..=0x26FF // Misc Symbols (a few math; blank-safe via validation)
        | 0x27C0..=0x27EF // Miscellaneous Mathematical Symbols-A
        | 0x2980..=0x29FF // Miscellaneous Mathematical Symbols-B
        | 0x2A00..=0x2AFF // Supplemental Mathematical Operators
        | 0x1D400..=0x1D7FF // Mathematical Alphanumeric Symbols
    )
}

/// The raw render output that crosses back from the [`render_on_big_stack`]
/// thread: the SVG string and the layout metrics (em, font-size-independent)
/// used to size and baseline-align the element.
struct RawRender {
    svg: String,
    /// Width in em.
    width: f64,
    /// Ascent above the baseline, in em.
    height: f64,
    /// Descent below the baseline, in em.
    depth: f64,
}

/// Layer C — run RaTeX on a spawned thread with an explicit 16 MiB stack, from
/// inside the rayon worker, and join it. RaTeX has no rayon dependency, so
/// there is no worker-pool nesting corruption. A panic inside (parse `unwrap`,
/// or an unforeseen path) surfaces as [`MathRefusal::RenderFailed`] rather than
/// propagating; a *stack overflow* still aborts (uncatchable under
/// `panic="abort"`), which is why layer A must keep it from ever reaching here.
fn render_on_big_stack(tex: &str) -> Result<RawRender, MathRefusal> {
    let owned = tex.to_string();
    let handle = std::thread::Builder::new()
        .name("moss-math-render".into())
        .stack_size(RENDER_STACK_BYTES)
        .spawn(move || render_inner(&owned))
        .map_err(|_| MathRefusal::RenderFailed)?;

    match handle.join() {
        Ok(result) => result,
        Err(_) => Err(MathRefusal::RenderFailed),
    }
}

/// The four RaTeX calls, isolated so the thread closure is trivial. Runs on the
/// 16 MiB render thread only.
fn render_inner(tex: &str) -> Result<RawRender, MathRefusal> {
    use ratex_layout::{layout, to_display_list, LayoutOptions};
    use ratex_parser::parser::parse;
    use ratex_svg::{render_to_svg, SvgOptions};

    let nodes = parse(tex).map_err(|_| MathRefusal::ParseError)?;
    let boxed = layout(&nodes, &LayoutOptions::default());
    let dl = to_display_list(&boxed);
    let opts = SvgOptions {
        font_size: RENDER_FONT_SIZE,
        // No padding: the element is sized to the ink box in em, so padding
        // would offset the baseline. Glyph edges are filled paths, not strokes,
        // so nothing clips.
        padding: 0.0,
        stroke_width: 1.5,
        // The pin that makes the output path-only. If a mis-built RaTeX ever
        // flips this, layer B's zero-`<text>` assertion catches it.
        embed_glyphs: true,
        font_dir: String::new(),
    };
    let svg = render_to_svg(&dl, &opts);
    Ok(RawRender {
        svg,
        width: dl.width,
        height: dl.height,
        depth: dl.depth,
    })
}

/// Layer B — validate the emitted SVG before it is allowed near the page.
fn validate_svg(svg: &str) -> Result<(), MathRefusal> {
    if svg.len() > MAX_SVG_BYTES {
        return Err(MathRefusal::InvalidOutput {
            reason: "svg exceeds byte cap",
        });
    }
    // Must be a rooted <svg …> element.
    let (before_root, _) = svg
        .split_once("<svg")
        .ok_or(MathRefusal::InvalidOutput { reason: "no <svg> root" })?;
    if !before_root.trim().is_empty() {
        return Err(MathRefusal::InvalidOutput {
            reason: "content before <svg> root",
        });
    }
    // Engine-drift tripwire: path-only output carries none of these. `<text>`
    // is what a wrong-featured build ships (rasterizes blank under resvg);
    // `<use>`/`<image>`/`id=` would break morph-inertness and self-containment.
    for forbidden in ["<text", "<use", "<image", " id=", "xlink:"] {
        if svg.contains(forbidden) {
            return Err(MathRefusal::InvalidOutput {
                reason: "svg carries a non-path element",
            });
        }
    }
    // Must carry at least one glyph — catches the emoji→empty-body render and
    // any allowed-but-uncovered codepoint that slipped the allowlist.
    if !svg.contains("<path") && !svg.contains("<rect") {
        return Err(MathRefusal::InvalidOutput {
            reason: "svg has no glyphs",
        });
    }
    // viewBox present and dimensionally sane (output-amplification cap).
    let (w, h) = parse_viewbox(svg).ok_or(MathRefusal::InvalidOutput {
        reason: "unparseable viewBox",
    })?;
    if !w.is_finite() || !h.is_finite() || w <= 0.0 || h <= 0.0 {
        return Err(MathRefusal::InvalidOutput {
            reason: "viewBox not positive-finite",
        });
    }
    if w > MAX_VIEWBOX_DIM || h > MAX_VIEWBOX_DIM {
        return Err(MathRefusal::InvalidOutput {
            reason: "viewBox exceeds sanity cap",
        });
    }
    Ok(())
}

/// HTML-escape for an attribute value (`aria-label`): the five chars that can
/// break out of a double-quoted attribute or inject markup.
fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// HTML-escape for element text (`<title>`): `&`, `<`, `>`.
fn escape_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Extract `(width, height)` from an SVG's `viewBox="minx miny width height"`.
fn parse_viewbox(svg: &str) -> Option<(f64, f64)> {
    let (_, after_attr) = svg.split_once("viewBox=\"")?;
    let (value, _) = after_attr.split_once('"')?;
    let mut it = value.split_ascii_whitespace();
    let _minx = it.next()?;
    let _miny = it.next()?;
    let w: f64 = it.next()?.parse().ok()?;
    let h: f64 = it.next()?.parse().ok()?;
    Some((w, h))
}

/// Rewrite the validated raw SVG into the served element: theme-adaptive
/// (`fill="currentColor"`), accessible (`role="img"`, `aria-label`, `<title>`),
/// stably classed (`moss-math` + `data-moss-math`), and sized/baseline-aligned
/// in em from the layout metrics. Display math is wrapped in the scroll box.
fn assemble(raw: &TypesetMath, tex: &str, display: bool) -> String {
    // Body = everything after the root open tag; the ratex root tag carries a
    // `viewBox` (kept) plus `width`/`height` in `pt` (dropped — the element is
    // sized in em). Recolor the baked-black glyph fills to inherit text color.
    let after_root = raw
        .svg
        .split_once('>')
        .map(|(_, after)| after)
        .unwrap_or(raw.svg.as_str());
    let body = after_root
        .trim_end()
        .strip_suffix("</svg>")
        .unwrap_or(after_root)
        .replace("\"rgba(0,0,0,1)\"", "\"currentColor\"");

    let viewbox = parse_viewbox(&raw.svg)
        .map(|(w, h)| format!("0 0 {w} {h}"))
        .unwrap_or_else(|| "0 0 1 1".to_string());

    let kind = if display { "display" } else { "inline" };
    let label = escape_attr(tex);
    let title = escape_text(tex);
    let em_h = raw.height_em + raw.depth_em;
    let em_w = raw.width_em;

    // Inline style carries the per-equation metrics (data, not theme): size in
    // em so it scales with surrounding text, and — for inline — a negative
    // vertical-align of the descent so the equation's baseline lands on the
    // text baseline. Team CSS's `.moss-math` rules add max-width/overflow.
    let style = if display {
        format!("width:{em_w:.4}em;height:{em_h:.4}em")
    } else {
        format!(
            "width:{em_w:.4}em;height:{em_h:.4}em;vertical-align:{:.4}em",
            -raw.depth_em
        )
    };

    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{viewbox}" role="img" class="moss-math" data-moss-math="{kind}" fill="currentColor" style="{style}" aria-label="{label}"><title>{title}</title>{body}</svg>"#
    );

    if display {
        format!(r#"<div class="moss-math-scroll">{svg}</div>"#)
    } else {
        svg
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Layer A: the caps sit below their measured failure thresholds ----

    #[test]
    fn length_cap_is_below_the_combining_mark_abort_threshold() {
        // #143 aborts at ~4,173 combining marks (≈8 KiB at 2 bytes each) on an
        // 8 MiB stack; the 16 MiB render thread pushes that higher still. The
        // 4 KiB byte cap is comfortably under it.
        assert!(MAX_TEX_BYTES < 8 * 1024);
    }

    #[test]
    fn nesting_cap_is_below_the_shallowest_measured_overflow() {
        // Shallowest spike overflow: `\sqrt` at depth 255 on a 2 MiB stack.
        assert!(MAX_NESTING < 255, "nesting cap must stay under 255");
    }

    #[test]
    fn over_length_source_refuses_not_aborts() {
        let long = "x".repeat(MAX_TEX_BYTES + 1);
        assert_eq!(
            render_math(&long, false),
            Err(MathRefusal::TooLong {
                bytes: MAX_TEX_BYTES + 1
            })
        );
    }

    #[test]
    fn deep_nesting_refuses_not_aborts() {
        // 40 nested \frac — over the cap of 32, well under the ~285 abort.
        let n = 40;
        let deep = format!("{}a{}", r"\frac{".repeat(n), "}{b}".repeat(n));
        match render_math(&deep, true) {
            Err(MathRefusal::NestingTooDeep { depth }) => assert!(depth > MAX_NESTING),
            other => panic!("expected NestingTooDeep, got {other:?}"),
        }
    }

    #[test]
    fn combining_mark_run_refuses_not_aborts() {
        // The #143 crash input, kept far below any abort — the guard must stop
        // it regardless of count.
        let input = format!("a{}", "\u{301}".repeat(50));
        assert_eq!(render_math(&input, false), Err(MathRefusal::CombiningMark));
    }

    #[test]
    fn dangerous_macros_refuse() {
        for (src, name) in [
            (r"\def\x{x}", "\\def"),
            (r"\csname foo\endcsname", "\\csname"),
            (r"\input{/etc/passwd}", "\\input"),
        ] {
            assert_eq!(
                render_math(src, false),
                Err(MathRefusal::DangerousMacro { macro_name: name }),
                "for {src:?}"
            );
        }
    }

    #[test]
    fn emoji_refuses_at_the_guard() {
        // Emoji renders to an empty SVG (the spike proved it) and is outside
        // the CJK blocks, so it must still refuse as DisallowedCodepoint — the
        // verify+pin path (§3.6) does NOT relax emoji/RTL/control.
        for src in [r"\text{🎉}", "🔥"] {
            match render_math(src, false) {
                Err(MathRefusal::DisallowedCodepoint { .. }) => {}
                other => panic!("expected DisallowedCodepoint for {src:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn cjk_typesets_when_a_font_is_pinned_else_degrades_to_source() {
        // §3.6: with a verified system CJK font present, CJK math typesets to
        // path-only SVG (the same guard → 16 MiB thread → validate_svg path);
        // with none, it degrades to NoCjkFont (P1 source). Never blank, never
        // DisallowedCodepoint (that stays for emoji/RTL/control).
        for src in [r"\text{线性注意力}", r"S_t = \text{状态}"] {
            match render_math(src, false) {
                Ok(html) => assert!(
                    html.contains("<path"),
                    "typeset CJK must carry glyph outlines: {src:?} → {html}"
                ),
                Err(MathRefusal::NoCjkFont) => {} // no verified system font here
                other => panic!("expected typeset or NoCjkFont for {src:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn bare_cjk_in_math_mode_refuses_but_text_mode_is_allowed() {
        // The currency false positive `一个$5，两个$10。` makes pulldown emit the
        // math span "5，两个" — bare CJK. It must refuse (→ literal source), NOT
        // typeset into a nonsensical image, regardless of whether a font exists.
        for src in ["5，两个", "统计", r"x + 波动"] {
            match render_math(src, false) {
                Err(MathRefusal::CjkOutsideTextMode { .. }) => {}
                other => panic!("bare CJK {src:?} must refuse as CjkOutsideTextMode, got {other:?}"),
            }
        }
        // CJK inside \text{} is deliberate math — it typesets or degrades to
        // NoCjkFont, but is NEVER refused as CjkOutsideTextMode.
        for src in [r"\text{统计}", r"S_t = \text{状态}"] {
            assert!(
                !matches!(render_math(src, false), Err(MathRefusal::CjkOutsideTextMode { .. })),
                "\\text{{}}-wrapped CJK must be allowed into the engine: {src:?}"
            );
        }
    }

    // ---- Happy path: pure Latin/Greek/symbol math typesets ----

    #[test]
    fn simple_inline_equation_typesets_to_currentcolor_svg() {
        let html = render_math("E=mc^2", false).expect("should typeset");
        assert!(html.starts_with("<svg"));
        assert!(html.contains(r#"role="img""#));
        assert!(html.contains(r#"class="moss-math""#));
        assert!(html.contains(r#"data-moss-math="inline""#));
        assert!(html.contains(r#"fill="currentColor""#));
        assert!(html.contains("<path"));
        assert!(html.contains(r#"aria-label="E=mc^2""#));
        // No baked-black fills leak past the recolor.
        assert!(!html.contains("rgba(0,0,0,1)"));
    }

    #[test]
    fn greek_and_math_operators_typeset_from_embedded_fonts() {
        for src in [r"\alpha+\beta", r"\sum_{i=1}^n x_i", r"\sqrt{x}", "α", "∑"] {
            let html = render_math(src, false)
                .unwrap_or_else(|e| panic!("expected typeset for {src:?}, got {e:?}"));
            assert!(html.contains("<path"), "no glyphs for {src:?}");
        }
    }

    #[test]
    fn display_math_is_wrapped_in_the_scroll_box() {
        let html = render_math(r"\sum_{i=1}^n x_i", true).expect("should typeset");
        assert!(html.starts_with(r#"<div class="moss-math-scroll">"#));
        assert!(html.contains(r#"data-moss-math="display""#));
        // Display math is not baseline-shifted.
        assert!(!html.contains("vertical-align"));
    }

    #[test]
    fn inline_math_carries_a_baseline_vertical_align() {
        // a_i has a descender, so its depth (and thus vertical-align) is nonzero.
        let html = render_math("a_i", false).expect("should typeset");
        assert!(html.contains("vertical-align:-"), "got {html}");
    }

    // ---- Determinism (build caching depends on it) ----

    #[test]
    fn same_equation_renders_byte_identical() {
        let a = render_math(r"\mathrm{Attn}(Q,K,V)", false).unwrap();
        let b = render_math(r"\mathrm{Attn}(Q,K,V)", false).unwrap();
        let c = render_math(r"\mathrm{Attn}(Q,K,V)", false).unwrap();
        assert_eq!(a, b);
        assert_eq!(b, c);
    }

    // ---- Layer B: output validation ----

    #[test]
    fn validation_rejects_text_elements() {
        // A hand-built <text>-bearing SVG stands in for a mis-built engine.
        let bad = r#"<svg viewBox="0 0 10 10"><text x="0" y="0">x</text></svg>"#;
        assert!(matches!(
            validate_svg(bad),
            Err(MathRefusal::InvalidOutput { .. })
        ));
    }

    #[test]
    fn validation_rejects_blank_output() {
        let blank = r#"<svg viewBox="0 0 10 10"></svg>"#;
        assert!(matches!(
            validate_svg(blank),
            Err(MathRefusal::InvalidOutput {
                reason: "svg has no glyphs"
            })
        ));
    }

    #[test]
    fn validation_rejects_amplified_viewbox() {
        let huge = r#"<svg viewBox="0 0 9999999 9999999"><path d="M0 0"/></svg>"#;
        assert!(matches!(
            validate_svg(huge),
            Err(MathRefusal::InvalidOutput {
                reason: "viewBox exceeds sanity cap"
            })
        ));
    }

    #[test]
    fn a_real_render_passes_validation_and_is_path_only() {
        let raw = render_on_big_stack(r"\frac{QK^\top}{\sqrt{d}}").unwrap();
        validate_svg(&raw.svg).expect("real render must validate");
        assert!(!raw.svg.contains("<text"));
        assert!(!raw.svg.contains(" id="));
        assert!(!raw.svg.contains("<use"));
    }

    #[test]
    fn only_unexpected_refusals_warn() {
        assert!(!MathRefusal::CombiningMark.is_unexpected());
        assert!(!MathRefusal::DisallowedCodepoint { codepoint: 0x7EBF }.is_unexpected());
        assert!(!MathRefusal::TooLong { bytes: 5000 }.is_unexpected());
        assert!(MathRefusal::ParseError.is_unexpected());
        assert!(MathRefusal::InvalidOutput { reason: "x" }.is_unexpected());
    }
}
