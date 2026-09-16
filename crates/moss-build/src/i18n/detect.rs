//! Language detection from content text
//!
//! Uses the whatlang crate to detect the language of document content,
//! and maps results to our Language enum.

use super::Language;
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;
use whatlang::{detect, Lang};

/// Detect the language of a text string.
///
/// Returns None if detection confidence is too low or language is unsupported.
///
/// Length is deliberately NOT a condition here: "what language is this string"
/// is answerable for a title, and answering it is what stops an OG card
/// rendering a Traditional title in Simplified glyph forms. The floor that
/// stops a stub speaking for a whole folder or site lives on the VOTE
/// ([`MIN_PROSE_CHARS`]), which is the question length actually bears on.
pub fn detect_language(text: &str) -> Option<Language> {
    // Detect on PROSE, not raw markdown. A real article's body is often
    // majority markup by byte count — shortcode directives, wikilink/image
    // targets, slugs in link destinations — and whatlang has no notion of
    // "this token is a filename, not a word". Feeding it the stripped prose
    // is what the sentence "what language is this document written in"
    // actually means; see `strip_non_prose` for exactly what is removed.
    let prose = strip_non_prose(text);

    // A CJK-majority prose sample settles this outright, whatever whatlang's
    // confidence over the (possibly short) sample says — checked BEFORE
    // whatlang, not only as an override on an `Eng` verdict. A short Chinese
    // sentence wrapped in heavy markup shrinks to almost nothing once the
    // markup is gone, often too short to clear whatlang's own reliability
    // threshold on its own; settling it here means that document still
    // resolves correctly instead of falling through to the raw-text
    // fallback below, which would reintroduce the bug this function exists
    // to close. See docs/archive/2026-08-20-rebuild-loop-incrementality.md
    // ("The leaf, explained on the instrument's first use: `lang`"), where
    // appending 35 ASCII characters flipped a real 1,254-byte Traditional
    // Chinese article from `None` to `Some(En)`.
    if is_cjk_dominant(&prose) {
        return Some(if is_traditional_chinese(&prose) {
            Language::ZhHant
        } else {
            Language::ZhHans
        });
    }

    if let Some(lang) = detect_reliable(&prose) {
        return Some(lang);
    }

    // Stripping can shrink genuinely short, non-CJK prose below whatlang's
    // reliability threshold on its own — a brief English post padded by a
    // fenced code sample, a landing page that's mostly shortcode directives
    // and wikilinks. Falling back to the raw text recovers the pre-fix
    // behavior for exactly that shape. This can't reintroduce the bug above:
    // a CJK-dominant document already returned above, so this fallback only
    // ever runs for documents that were never a CJK-majority-outvoted-by-
    // markup case to begin with.
    detect_reliable(text)
}

/// whatlang's verdict on `text`, mapped to [`Language`], or `None` if
/// unreliable or an unsupported language. Traditional/Simplified Chinese are
/// distinguished by script analysis on the SAME text passed in, since
/// whatlang detects "Mandarin" without splitting the two scripts.
fn detect_reliable(text: &str) -> Option<Language> {
    let info = detect(text)?;
    if !info.is_reliable() {
        return None;
    }
    match info.lang() {
        Lang::Eng => Some(Language::En),
        Lang::Cmn => Some(if is_traditional_chinese(text) {
            Language::ZhHant
        } else {
            Language::ZhHans
        }),
        _ => None, // Unsupported language
    }
}

static FENCED_CODE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)```.*?```").expect("valid regex"));
static INLINE_CODE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`[^`]*`").expect("valid regex"));
static WIKILINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"!?\[\[([^\]|]*)(?:\|([^\]]*))?\]\]").expect("valid regex"));
// `\S*\)` rather than `[^)]*\)`: a destination can legitimately contain a
// balanced `)` (a Wikipedia disambiguation URL,
// `.../Rust_(programming_language)`), and greedy `\S*` backtracks to the
// LAST `)` before whitespace, so it still closes on the real one rather
// than stopping at the URL's own embedded paren.
static MD_LINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([^\]]*)\]\(\S*\)").expect("valid regex"));
static BARE_URL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"https?://\S+").expect("valid regex"));
static HEADING_LIST_MARKER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^(?:#{1,6}|[-*+])[ \t]+").expect("valid regex"));

/// Reduce markdown source to something closer to what a reader actually
/// reads, for the purpose of language detection.
///
/// A real vault article is often majority markup by byte count: shortcode
/// directives (`:::hero {image=…}`), wikilink/image targets
/// (`![[assets/…jpg]]`), slugs in link destinations
/// (`/awards/writing/s4/…`). whatlang scores trigrams over the whole input
/// with no notion that a filename or URL isn't a word, so a CJK article
/// whose markup happens to outweigh its prose reads as English — not
/// because the prose is English, but because the plumbing is Latin script.
/// Stripping the plumbing before detection asks the right question: what
/// language is this document's PROSE in.
///
/// Deliberately shape-based rather than a full AST parse (unlike
/// `build::page::meta::first_paragraph_excerpt`, which needs render-exact
/// fidelity because its output is published): detection only needs the
/// gross composition of the text, so a false-positive strip here just
/// removes a few more Latin characters from the sample, never publishable
/// content.
fn strip_non_prose(text: &str) -> String {
    let s = FENCED_CODE_RE.replace_all(text, "");
    let s = INLINE_CODE_RE.replace_all(&s, "");
    let s = strip_html_comments(&s);

    // Shortcode directive lines (`:::name {attrs}`, bare `:::` closers) and
    // grid-cell dividers (`+++`) carry no prose at all — drop the whole line.
    let s: String = s
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.starts_with(":::") && trimmed != "+++"
        })
        .collect::<Vec<_>>()
        .join("\n");

    // `![[assets/x.jpg]]` / `[[page]]` → drop, keeping a `|alt` pipe-alias
    // when present. `[text](url)` → keep the text, drop the destination.
    let s = WIKILINK_RE.replace_all(&s, "$2");
    let s = MD_LINK_RE.replace_all(&s, "$1");
    let s = BARE_URL_RE.replace_all(&s, "");
    let s = HEADING_LIST_MARKER_RE.replace_all(&s, "");

    s.into_owned()
}

/// Strip `<!-- ... -->` comments from markdown SOURCE text.
///
/// Written as a manual char scan rather than `Regex::new`/`.replace_all`
/// with a `<!--` needle on purpose: ratchet row (p) (`html_string_mutation`,
/// ADR-034) counts any `Regex::new`/`.replace*`/`.find`-family call whose
/// string argument contains `<` as evidence of a tag-aware pass over
/// ALREADY-EMITTED HTML bypassing the structural `Vec<Block>` pipeline. This
/// function does the opposite: it runs on the raw markdown an author typed,
/// before any rendering, purely to sample prose for language detection, and
/// its output is discarded immediately after — never emitted. The count
/// can't distinguish the two cases, so this stays off its needle set rather
/// than inflating a gate that exists to catch a real risk this isn't.
/// An unterminated comment (author mid-keystroke) drops the remainder of
/// the text, which only shrinks the detection sample — never wrong content.
fn strip_html_comments(text: &str) -> String {
    const OPEN: [char; 4] = ['<', '!', '-', '-'];
    const CLOSE: [char; 3] = ['-', '-', '>'];

    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if i + OPEN.len() <= chars.len() && chars[i..i + OPEN.len()] == OPEN {
            let mut j = i + OPEN.len();
            let mut closed_at = None;
            while j + CLOSE.len() <= chars.len() {
                if chars[j..j + CLOSE.len()] == CLOSE {
                    closed_at = Some(j + CLOSE.len());
                    break;
                }
                j += 1;
            }
            match closed_at {
                Some(end) => i = end,
                None => break,
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// Whether CJK characters make up an outright MAJORITY of the text — strong
/// enough evidence that a whatlang call of `Eng` is an artifact of a short
/// Latin-script fragment rather than a real language switch.
///
/// Deliberately a strict majority, not a low percentage: an English essay
/// that quotes CJK material at length (translation notes, literary
/// criticism, a glossary) can legitimately carry a large minority share of
/// CJK characters while still being an English document, and a low
/// threshold would misclassify it. The real vault article this override was
/// built for is ~92% CJK by character count even after the ASCII marker
/// that triggered the bug — nowhere near this boundary — so requiring a
/// majority still closes the reported bug with wide margin while leaving
/// CJK-quoting English content alone.
fn is_cjk_dominant(text: &str) -> bool {
    let mut cjk_count = 0usize;
    let mut total_chars = 0usize;
    for c in text.chars() {
        if c.is_whitespace() {
            continue;
        }
        total_chars += 1;
        if is_cjk(c) {
            cjk_count += 1;
        }
    }
    if total_chars == 0 {
        return false;
    }
    (cjk_count as f64 / total_chars as f64) > 0.5
}

/// Distinguish Traditional Chinese from Simplified Chinese.
///
/// Counts the discriminating evidence on BOTH sides and takes the majority.
/// It used to count only Traditional-specific characters against a 5%-of-CJK
/// threshold, which made the test asymmetric: it could prove Traditional and
/// never Simplified, so everything it failed to prove was reported as
/// Simplified. A real user's vault was pinned to `zh-hans` that way — none of
/// 測, 試 or 場, the discriminating characters in their own text, were on the
/// one-sided list (`docs/archive/2026-08-31-site-lang-derived-state.md`).
///
/// A tie — including the no-evidence case — is Simplified, matching the
/// shorthand [`Language::from_code`] already applies to a bare `"zh"`.
fn is_traditional_chinese(text: &str) -> bool {
    let (traditional, simplified) = script_evidence(text);
    traditional > simplified
}

/// `(traditional_hits, simplified_hits)` over [`SCRIPT_PAIRS`].
///
/// One pass, and a character counts at most once: the pairs are one-to-one, so
/// no character is on both sides.
fn script_evidence(text: &str) -> (usize, usize) {
    let mut traditional = 0usize;
    let mut simplified = 0usize;
    for c in text.chars() {
        if SCRIPT_PAIRS.iter().any(|(t, _)| *t == c) {
            traditional += 1;
        } else if SCRIPT_PAIRS.iter().any(|(_, s)| *s == c) {
            simplified += 1;
        }
    }
    (traditional, simplified)
}

/// `(traditional, simplified)` forms of the same word, the discriminating
/// evidence [`is_traditional_chinese`] weighs.
///
/// ONE table feeding both counters, rather than two lists that would drift the
/// moment someone extended one of them: adding a pair improves both verdicts at
/// once, and a missing pair weakens both symmetrically instead of biasing the
/// answer one way.
///
/// Every entry is a genuine one-to-one mapping. Deliberately absent: pairs
/// whose simplified form is also a standalone Traditional character (後/后 —
/// 后 is "empress" in Traditional; 種/种; 準/准; 裡/里), and pairs whose
/// simplified form stands for several traditional characters (複/复, 髮/发,
/// 麵/面). Either kind would score Traditional text as Simplified, which is the
/// exact failure this table exists to end.
const SCRIPT_PAIRS: &[(char, char)] = &[
    ('個', '个'), ('們', '们'), ('來', '来'), ('這', '这'), ('與', '与'),
    ('從', '从'), ('為', '为'), ('將', '将'), ('還', '还'), ('過', '过'),
    ('開', '开'), ('對', '对'), ('學', '学'), ('會', '会'), ('點', '点'),
    ('處', '处'), ('經', '经'), ('間', '间'), ('關', '关'), ('實', '实'),
    ('現', '现'), ('發', '发'), ('問', '问'), ('題', '题'), ('讓', '让'),
    ('導', '导'), ('設', '设'), ('計', '计'), ('變', '变'), ('國', '国'),
    ('語', '语'), ('區', '区'), ('產', '产'), ('應', '应'), ('術', '术'),
    ('構', '构'), ('軟', '软'), ('體', '体'), ('網', '网'), ('頁', '页'),
    ('說', '说'), ('話', '话'), ('請', '请'), ('認', '认'), ('識', '识'),
    ('書', '书'), ('號', '号'), ('訊', '讯'), ('質', '质'), ('較', '较'),
    ('運', '运'), ('進', '进'), ('當', '当'), ('該', '该'), ('總', '总'),
    ('類', '类'), ('數', '数'), ('態', '态'), ('時', '时'), ('東', '东'),
    ('車', '车'), ('馬', '马'), ('見', '见'), ('長', '长'), ('門', '门'),
    ('風', '风'), ('飛', '飞'), ('魚', '鱼'), ('鳥', '鸟'), ('買', '买'),
    ('賣', '卖'), ('錢', '钱'), ('銀', '银'), ('誰', '谁'), ('讀', '读'),
    ('寫', '写'), ('聽', '听'), ('覺', '觉'), ('樣', '样'), ('麼', '么'),
    ('兒', '儿'), ('頭', '头'), ('臉', '脸'), ('愛', '爱'), ('樂', '乐'),
    ('藝', '艺'), ('節', '节'), ('義', '义'), ('醫', '医'), ('藥', '药'),
    ('農', '农'), ('業', '业'), ('華', '华'), ('萬', '万'), ('專', '专'),
    ('傳', '传'), ('辦', '办'), ('級', '级'), ('紙', '纸'), ('線', '线'),
    ('練', '练'), ('結', '结'), ('給', '给'), ('統', '统'), ('織', '织'),
    ('續', '续'), ('測', '测'), ('試', '试'), ('場', '场'), ('資', '资'),
    ('筆', '笔'), ('內', '内'), ('張', '张'), ('轉', '转'), ('記', '记'),
    ('夾', '夹'), ('檔', '档'), ('帶', '带'), ('選', '选'), ('舊', '旧'),
];
fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x4E00..=0x9FFF |   // CJK Unified Ideographs
        0x3400..=0x4DBF |   // CJK Extension A
        0x20000..=0x2A6DF   // CJK Extension B
    )
}

/// The sampled language, or `None` when NO sampled file yielded a confident
/// detection — an empty vault, or one with no real prose to judge.
///
/// That `None` used to be a second function, `has_content_language_basis`,
/// which re-read the same up-to-20 files just to answer "did anything
/// classify?" and carried a doc comment promising its sampling mirrored this
/// one's "EXACTLY … the lockstep is structural, not copied". It was neither
/// structural nor free: every site-language resolve read the vault twice. The
/// vote already knows the answer — its tally is empty in exactly that case —
/// so the predicate is the return type.
fn detect_project_language(
    markdown_files: &[crate::types::content::FileInfo],
    root_path: &str,
    is_evicted: &dyn Fn(&std::path::Path) -> bool,
) -> Option<Language> {
    let texts: Vec<String> = markdown_files
        .iter()
        .take(20)
        .filter_map(|f| {
            let abs_path = std::path::Path::new(root_path).join(&f.path);
            if is_evicted(&abs_path) {
                crate::build::cloud_readiness::request_download(&abs_path);
                return None;
            }
            let content = std::fs::read_to_string(abs_path).ok()?;
            // Strip frontmatter to avoid YAML polluting detection
            Some(strip_frontmatter(&content))
        })
        .collect();

    let refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
    site_language_vote(&refs)
}

/// Strip YAML frontmatter (--- delimited) from markdown content.
///
/// `pub(crate)` so folder-level language inference
/// (`build::scan::page_map::folder_lang`) can sample the same
/// frontmatter-free prose this module's own site-level detectors sample,
/// rather than re-deriving the split.
pub(crate) fn strip_frontmatter(content: &str) -> String {
    // Language is detected from BODY text. Leaked field lines skew the vote, so
    // the boundary is `frontmatter_span`'s call for both dialects — this used to
    // see the YAML dialect only, and match the first `\n---` anywhere in the
    // file rather than a standalone delimiter line (moss#937).
    match moss_core::frontmatter::frontmatter_span(content) {
        #[allow(clippy::string_slice)]
        // Char-aligned: `body` is a line-boundary offset from the splitter.
        Some(span) => content[span.body..].to_string(),
        None => content.to_string(),
    }
}

/// The least prose a set of documents must hold BETWEEN them before moss will
/// say what language they are collectively in, in non-whitespace characters
/// after markup is stripped.
///
/// The expensive failure is the confident-but-wrong one: a first run over a
/// vault holding one short page produced a full site verdict, and back when
/// that verdict was written into `config.toml` it outranked every real page the
/// author went on to write — a Traditional Chinese vault serving Simplified
/// chrome, decided by a file that was never evidence of anything.
///
/// It is a floor on the TOTAL, not an admission test each document has to pass.
/// A per-document gate discards evidence that is overwhelming in aggregate: a
/// folder of thirty twenty-character Chinese link posts is not ambiguous, and
/// gating each one individually would have left it with no verdict at all and
/// fallen the whole folder through to the site default. Weighing each document
/// by its prose length (below) is what makes one stub lose to one real page
/// without also making thirty stubs lose to nothing.
///
/// 24 is about one Chinese sentence and a short English one. It bounds the
/// SMALLEST corpus that gets an answer, not the smallest document that counts.
const MIN_PROSE_CHARS: usize = 24;

/// The language of `text`, if any, and how much prose the question rests on.
///
/// The weight is the size of the same stripped prose [`detect_language`]
/// judges: a page that is 3kB of shortcode and eight words weighs eight words.
/// It is returned even when nothing classified, because "how much writing is
/// here" and "what language is it" are different questions.
fn classify(text: &str) -> (Option<Language>, usize) {
    let weight = strip_non_prose(text).chars().filter(|c| !c.is_whitespace()).count();
    (detect_language(text), weight)
}

/// Which language a set of documents is written in, weighed by how much prose
/// each one brings — or `None` when there is not enough prose between them all
/// to be worth an answer ([`MIN_PROSE_CHARS`]), or when none of them
/// classified.
///
/// `None` is the point: both callers are deciding a DEFAULT, and a folder or
/// site with no evidence must fall through to the rung below it rather than to
/// English. This used to be `detect_site_language`, whose `unwrap_or(En)`
/// threw that distinction away; nothing calls it any more.
///
/// `pub(crate)` for the folder-level inference in
/// `build::scan::page_map::folder_lang`, which asks exactly this question.
pub(crate) fn site_language_vote(texts: &[&str]) -> Option<Language> {
    let mut counts: HashMap<Language, usize> = HashMap::new();
    let mut total_prose = 0usize;

    for text in texts {
        // Prose from a document that did NOT classify still counts toward the
        // floor: it is evidence the corpus is real writing, just not evidence
        // of WHICH language.
        let (lang, weight) = classify(text);
        total_prose += weight;
        if let Some(lang) = lang {
            *counts.entry(lang).or_insert(0) += weight;
        }
    }

    if total_prose < MIN_PROSE_CHARS {
        return None;
    }

    // `HashMap`'s iteration order is randomized per-process (SipHash seed), so
    // a plain `max_by_key` on the weight alone picks an arbitrary winner among
    // ties — different each run. Weighing by prose length makes an exact tie
    // rarer than the old one-vote-per-file count did, but not impossible (two
    // documents of the same length in different languages, or one file per
    // language in a folder of equal-length stubs), so this must be a total
    // order rather than "whichever the hasher visited last". Break ties on the
    // language code: arbitrary, but the SAME arbitrary choice every build.
    counts
        .into_iter()
        .max_by(|(lang_a, weight_a), (lang_b, weight_b)| {
            weight_a.cmp(weight_b).then_with(|| lang_b.code().cmp(lang_a.code()))
        })
        .map(|(lang, _)| lang)
}

/// Resolve a site's DEFAULT language, as the BCP-47 code an author wrote where
/// they wrote one.
///
/// Priority: explicit `[site] lang` → a homepage `lang:` declaration →
/// content-detected language (only when the vault has real prose to base it on)
/// → the user's SYSTEM/OS `system_lang`.
///
/// Why this exists: the vote bottoms out at English when no file yields a
/// confident detection, so a signal-less vault silently became English
/// regardless of the user's locale — that is why the "Published with moss"
/// colophon rendered English (not 青苔发布) on an empty/ambiguous Chinese site.
/// An empty vote distinguishes "no real prose" from "confident English",
/// letting us substitute the system language only in the genuinely-unclear case.
///
/// ## Why a code and not a `Language`
///
/// The two declared rungs return the author's own code, including one moss has
/// no `Language` for. `[site] lang = "fr"` is an authoring statement about the
/// site, and the readers that carry it off-machine — the email audience list,
/// the syndication payload — must not silently relabel a French site English.
/// The code must still be one moss recognizes (`is_known_language_code`, the
/// same allowlist the per-page rungs use); anything else falls through to
/// detection. That is not tidiness: this string lands unescaped in
/// `<html lang="…">` on every published page, so an unconstrained declaration
/// would be an injection point, and `lang = "english"` would ship an invalid
/// tag to every reader instead of quietly meaning what it says.
/// Callers that need the enum (the render path, deciding `<html lang>`) map it
/// themselves and fall back to English there, where a missing translation is
/// what the fallback actually means. Returning `Language` here collapsed both
/// jobs onto the three languages moss ships and lost `fr` on the way out.
///
/// `system_lang` is INJECTED (not read from the `app_language()` global) so this
/// is deterministically unit-testable; the build call site passes
/// `crate::i18n::build_default_language()`.
pub fn resolve_site_default_lang(
    explicit: Option<&str>,
    homepage_file: Option<&str>,
    markdown_files: &[crate::types::content::FileInfo],
    root_path: &str,
    system_lang: Language,
) -> String {
    let is_evicted = &crate::build::icloud::is_evicted;
    // Trimmed AND lowercased, because the consumers compare this code as a
    // string: `Language::from_code` and `is_known_language_code` both fold
    // case, so ` zh-hant ` and `ZH-Hant` sail through validation and then ride
    // verbatim into site-languages.json, where the Matters plugin's
    // `startsWith("zh")` fails on a live post. Whitespace was that bug once
    // already; case is the same bug with a different keystroke.
    if let Some(code) =
        explicit.map(str::trim).filter(|c| moss_core::home::is_known_language_code(c))
    {
        return code.to_lowercase();
    }
    if let Some(code) = homepage_frontmatter_lang(homepage_file, root_path, is_evicted) {
        return code.to_lowercase();
    }
    detect_project_language(markdown_files, root_path, is_evicted)
        .unwrap_or(system_lang)
        .code()
        .to_string()
}

/// A `lang:` declared in the homepage's frontmatter — an authoring statement,
/// so it outranks detection exactly as `[site] lang` does.
///
/// It used to reach the render path indirectly: the build copied it into
/// `[site] lang` and every consumer read it back from there. With that write
/// gone (docs/archive/2026-08-31-site-lang-derived-state.md) the ladder reads
/// the declaration where the author wrote it.
fn homepage_frontmatter_lang(
    homepage_file: Option<&str>,
    root_path: &str,
    is_evicted: &dyn Fn(&std::path::Path) -> bool,
) -> Option<String> {
    let path = std::path::Path::new(root_path).join(homepage_file.unwrap_or("index.md"));
    crate::i18n::declared_lang_in_file(&path, is_evicted)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The eviction predicate for tests that are not about eviction.
    fn never_evicted(_: &std::path::Path) -> bool {
        false
    }

    /// The floor: a stub is not evidence about anything but itself. Both
    /// halves matter — the stub still classifies, and it still does not get to
    /// decide what the folder around it is written in.
    #[test]
    fn a_stub_classifies_itself_but_does_not_vote() {
        let stub = "第二遍測試";
        assert_eq!(
            detect_language(stub),
            Some(Language::ZhHant),
            "the string is still Traditional Chinese, and the OG card needs to know"
        );
        assert_eq!(site_language_vote(&[stub]), None, "but it is not evidence about a site");

        let english = "This is an article about software engineering and web development.";
        assert_eq!(
            site_language_vote(&[stub, english]),
            Some(Language::En),
            "so a folder holding one stub and one real page is the real page's language, \
             not a tie broken by whichever code sorts first"
        );
    }

    /// The failure the per-document form of this floor would have caused, and
    /// the reason the floor is on the corpus instead: thirty twenty-character
    /// posts are not ambiguous about their language, and a gate each document
    /// had to pass individually would have left the folder with no answer and
    /// fallen every page in it through to the site default.
    #[test]
    fn many_short_documents_are_evidence_even_though_none_is_alone() {
        let posts: Vec<&str> = vec!["第二遍測試，記一下這個。"; 30];
        assert_eq!(site_language_vote(&posts[..1]), None, "one of them is not evidence");
        assert_eq!(site_language_vote(&posts), Some(Language::ZhHant), "thirty of them are");
    }

    /// And the weighting, which is what lets both of the above be true at once:
    /// one real page outweighs one stub rather than tying with it.
    #[test]
    fn a_real_page_outweighs_a_stub_beside_it() {
        let stub = "第二遍測試";
        let page = "This is an article about software engineering and web development, \
                    covering the techniques used to build modern applications.";
        assert_eq!(site_language_vote(&[stub, page]), Some(Language::En));
    }

    /// The other half of the floor: markup is not prose, however much of it
    /// there is. A page that is almost entirely shortcodes and link targets
    /// has whatever words are left as its evidence, and that is a stub.
    #[test]
    fn a_page_of_markup_does_not_vote_on_its_byte_count() {
        let markup = "![](a.jpg)\n[link](/some/very/long/path/to/somewhere.html)\n![](b.jpg)";
        assert!(markup.len() > 60, "the fixture must be long in BYTES, or it proves nothing");
        assert_eq!(site_language_vote(&[markup]), None);
    }

    #[test]
    fn test_detect_english() {
        let text = "This is a blog post about software engineering and web development. \
                    We explore various techniques for building modern applications.";
        assert_eq!(detect_language(text), Some(Language::En));
    }

    #[test]
    fn test_detect_chinese_simplified() {
        let text = "这是一篇关于软件工程和网页开发的博客文章。我们探讨了构建现代应用程序的各种技术。";
        assert_eq!(detect_language(text), Some(Language::ZhHans));
    }

    #[test]
    fn test_detect_chinese_traditional() {
        let text = "這是一篇關於軟體工程和網頁開發的部落格文章。我們探討了構建現代應用程式的各種技術。";
        assert_eq!(detect_language(text), Some(Language::ZhHant));
    }

    #[test]
    fn test_detect_too_short_returns_none() {
        assert_eq!(detect_language("hi"), None);
        assert_eq!(detect_language(""), None);
    }

    /// Realistic Traditional Chinese body text (~1,254 bytes in the real
    /// vault article that surfaced this bug), including a couple of ASCII
    /// URLs — exactly the mix a real article carries.
    const REALISTIC_ZH_HANT_ARTICLE: &str = "戰火下的文學抉擇：在動盪的年代裡，作家們面臨著前所未有的挑戰與困境。他們必須在生存與創作之間做出艱難的抉擇，究竟是選擇沉默以求自保，還是繼續執筆記錄下這個時代的真相。許多作家選擇了後者，即使明知這樣的選擇可能為他們帶來危險，他們依然堅持用文字對抗遺忘。這段歷史提醒著我們，文學從來不只是消遣，而是見證與抵抗的重要方式。相關資料可參考 https://example.com/archive/war-literature 與 https://example.com/archive/writers-in-exile 這兩個典藏網站，裡面收錄了大量珍貴的第一手訪談與手稿掃描檔案，對於研究這段歷史的讀者而言，是不可多得的重要資源，值得反覆閱讀與深入探討，也期盼未來有更多學者投入相關領域的研究工作。";

    /// The bug this fix closes: appending a short English sentence to a real
    /// Traditional Chinese article must not flip its detected language.
    /// Before the fix, whatlang's whole-text confidence let 35 ASCII
    /// characters override ~1,250 bytes of Chinese body text.
    #[test]
    fn appending_ascii_marker_does_not_flip_traditional_chinese_article() {
        assert_eq!(detect_language(REALISTIC_ZH_HANT_ARTICLE), Some(Language::ZhHant));

        let marker = " Edited via bench at 2026-08-20T00:00:00Z."; // 35+ ASCII chars
        let edited = format!("{REALISTIC_ZH_HANT_ARTICLE}{marker}");
        assert_eq!(
            detect_language(&edited),
            Some(Language::ZhHant),
            "an appended ASCII marker must not change a CJK-dominant article's detected language"
        );
    }

    /// Inverse of the above: a genuinely English page with no CJK content at
    /// all must still resolve sensibly to English.
    #[test]
    fn genuinely_english_article_resolves_to_english() {
        let article = "The city council met on Tuesday to discuss the new budget proposal. \
            Several residents spoke during the public comment period, raising concerns about \
            infrastructure spending and the timeline for the proposed road repairs. The council \
            voted to table the discussion until next month's meeting, citing the need for \
            further review of the engineering reports submitted by the public works department.";
        assert_eq!(detect_language(article), Some(Language::En));
    }

    /// Regression for the CJK-dominance override itself: an English essay
    /// that quotes CJK material at length (translation notes, literary
    /// criticism) is still an English document even though CJK characters
    /// are a large MINORITY of its text. A low CJK-share threshold would
    /// misclassify this; the override requires an outright majority.
    #[test]
    fn english_essay_with_substantial_cjk_quotation_stays_english() {
        let essay = "In her 1987 essay on translating classical Chinese poetry, \
            the critic argues that no rendering can fully capture the compression \
            of the original line 春眠不覺曉，處處聞啼鳥，夜來風雨聲，花落知多少. \
            She contrasts several English translations against the source text \
            人生得意須盡歡，莫使金樽空對月，天生我材必有用，千金散盡還復來, \
            noting how each translator makes different tradeoffs between literal \
            meaning and poetic rhythm, and concludes that the English reader loses \
            something no footnote can restore.";
        assert_eq!(
            detect_language(essay),
            Some(Language::En),
            "a CJK minority (quoted poetry) must not override a document that is otherwise English"
        );
    }

    /// The real article's `resolve_document_language`-level regression test
    /// lives in `i18n::tests::a_trivial_edit_to_the_real_ukraine_article_does_not_flip_its_language`
    /// (uses the same fixture bytes, exercised through the full priority
    /// chain). This one is the `detect_language`-only inverse: an English page
    /// carrying the same
    /// kind of heavy markup (shortcode directives, wikilink targets,
    /// markdown links with long slugs) must still resolve to English —
    /// stripping markup before detection must not manufacture a Chinese
    /// verdict out of an English page just because it has few prose words.
    #[test]
    fn english_page_with_heavy_markup_still_resolves_to_english() {
        let page = ":::hero {image=cover.jpg}\n:::\n\n\
            The city council met on Tuesday to discuss the new budget proposal \
            and infrastructure spending for the coming fiscal year.\n\n\
            :::grid 3 {.fs-parts}\n\
            ![[assets/photo-one.jpg]]\n\n\
            Part one\n\n\
            ### [Council debates road repair funding](/news/2024/road-repair-funding/)\n\
            +++\n\
            ![[assets/photo-two.jpg]]\n\n\
            Part two\n\n\
            ### [Residents raise concerns at public hearing](/news/2024/public-hearing/)\n\
            :::\n\n\
            - [Meeting minutes](/news/2024/minutes/)\n\
            - [Budget summary](/news/2024/budget-summary/)\n";
        assert_eq!(detect_language(page), Some(Language::En));
    }

    /// A short English post padded by a fenced code block that gets
    /// entirely stripped: without a raw-text fallback, the remaining prose
    /// ("Config… Set this up… Done.") is too short to clear whatlang's
    /// reliability threshold on its own, and this page would silently
    /// regress from `Some(En)` to `None` — which `resolve_document_language`
    /// turns into a silent fall-through to the site's default language.
    /// `detect_language` must fall back to raw-text detection for this
    /// shape, since it is not the CJK-majority-outvoted-by-markup case this
    /// module exists to fix.
    #[test]
    fn short_english_post_with_fenced_code_still_resolves_to_english() {
        let page = "# Config\n\nSet this up.\n\n```yaml\nserver:\n  host: 0.0.0.0\n  port: 8080\ndatabase:\n  url: postgres://localhost/app\n  pool_size: 10\nlogging:\n  level: info\n```\n\nDone.";
        assert_eq!(detect_language(page), Some(Language::En));
    }

    /// A landing page that is almost entirely shortcode/wikilink markup,
    /// with only a couple of short link-text fragments as prose. The same
    /// regression as above, from the opposite direction (near-zero prose
    /// rather than prose drowned by code).
    #[test]
    fn near_zero_prose_markup_only_page_still_resolves_to_english() {
        let page = ":::hero {image=cover.jpg}\n:::\n\n\
            :::grid 3 {.fs-parts}\n\
            ![[assets/photo-one.jpg]]\n\
            ![[assets/photo-two.jpg]]\n\
            ![[assets/photo-three.jpg]]\n\
            :::\n\n\
            - [Meeting minutes](/news/2024/minutes/)\n\
            - [Budget summary](/news/2024/budget-summary/)\n";
        assert_eq!(detect_language(page), Some(Language::En));
    }

    /// A markdown link whose destination is a Wikipedia-style URL with a
    /// balanced parenthetical must not leave a stray `)` in the stripped
    /// prose, and must not affect the resolved language either way.
    #[test]
    fn markdown_link_with_parenthesized_url_strips_cleanly() {
        let page = "Rust is a systems programming language. See \
            [Rust](https://en.wikipedia.org/wiki/Rust_(programming_language)) for details \
            on its ownership model and memory safety guarantees.";
        assert_eq!(detect_language(page), Some(Language::En));
        assert_eq!(
            strip_non_prose(page),
            "Rust is a systems programming language. See \
            Rust for details \
            on its ownership model and memory safety guarantees."
        );
    }

    #[test]
    fn a_majority_english_corpus_votes_english() {
        let texts = vec![
            "This is an English article about programming and software development techniques.",
            "Another English post about web development, covering modern frameworks and best practices.",
            "Yet another post in English discussing various software engineering topics and methodologies.",
            "这是一篇关于软件工程和网页开发的中文文章。",
        ];
        assert_eq!(site_language_vote(&texts), Some(Language::En));
    }

    #[test]
    fn a_majority_chinese_corpus_votes_chinese() {
        let texts = vec![
            "这是一篇关于编程的中文文章，讨论了软件工程的各种技术和方法。",
            "另一篇关于网页开发的中文文章，涵盖了现代框架和最佳实践。",
            "第三篇中文文章探讨了各种软件工程主题和方法论。",
            "This is an English article about programming and software development.",
        ];
        assert_eq!(site_language_vote(&texts), Some(Language::ZhHans));
    }

    /// The bug this closed: a genuine tie between two languages was resolved by
    /// `max_by_key` on the tally alone, which breaks ties by `HashMap`
    /// iteration order — randomized per process — so the SAME two texts picked
    /// a different winner from one build to the next
    /// (`build_parity_test::parity_cjk_unicode_filenames`, discovered via
    /// ADR-065's per-folder inference, which ties far more often than the
    /// whole-site sample this function was written for).
    ///
    /// The fixtures are length-matched ON PURPOSE, and the test asserts that
    /// before it asserts anything else. Votes are weighed by prose length, so
    /// "two documents in different languages" is no longer enough to tie —
    /// without the weight check this test would keep passing while quietly
    /// becoming a one-voter election that proves nothing about tie-breaking.
    /// That is exactly what happened to its previous form.
    #[test]
    fn a_tied_vote_resolves_the_same_way_every_build() {
        let a = "這是一篇繁體中文的文章，談的是資料夾的處理與影像方向的問題，也談到了拍攝的日期跟地點，還有檔案名稱的命名規則跟很多其他相關細節一部分的事情喔。";
        let b = "This is an English article about programming and software development techniques.";
        let (lang_a, weight_a) = classify(a);
        let (lang_b, weight_b) = classify(b);
        assert_ne!(lang_a, lang_b, "the fixtures must be in different languages");
        assert!(lang_a.is_some() && lang_b.is_some(), "and both must classify");
        assert_eq!(weight_a, weight_b, "and carry EQUAL weight, or there is no tie to break");

        let forward = site_language_vote(&[a, b]);
        // Reversed insertion order perturbs `HashMap` bucket layout within the
        // SAME process, which is the cheapest way to catch an order-dependent
        // tie-break without needing two process runs.
        let reversed = site_language_vote(&[b, a]);
        assert_eq!(
            forward, reversed,
            "a tied vote must resolve the same way regardless of insertion order"
        );
    }

    /// The reported vault's filename shape (client name stood in for). Both
    /// strings are short, and NEITHER contains a character from the old
    /// one-sided 60-char list — which is exactly why that vault was pinned
    /// to `zh-hans` for four days. The paired table scores 測/試 and 場
    /// against zero Simplified evidence, so these resolve to Traditional
    /// rather than merely "not Simplified".
    #[test]
    fn short_traditional_titles_are_traditional_not_simplified() {
        assert_eq!(detect_language("第二遍測試"), Some(Language::ZhHant));
        assert_eq!(detect_language("青苔-現場文件"), Some(Language::ZhHant));
    }

    /// The other direction, and the reason the table is paired: Simplified
    /// evidence has to be able to WIN, not merely fail to lose. A stray
    /// Traditional character in Simplified prose (a quoted name, a title) no
    /// longer flips the whole document.
    #[test]
    fn simplified_prose_outvotes_a_stray_traditional_character() {
        assert_eq!(
            detect_language("这是一篇关于软件开发的中文文章，讨论了国内的实现问题。"),
            Some(Language::ZhHans)
        );
        assert_eq!(
            detect_language("这是关于软件开发的中文文章，引用了《個人》這本書的一句話。"),
            Some(Language::ZhHans)
        );
    }

    /// No discriminating character either way. Simplified is the documented
    /// shorthand a bare `zh` already resolves to (`Language::from_code`), and
    /// since nothing persists the verdict any more, a longer document later
    /// simply overrules it.
    #[test]
    fn chinese_with_no_discriminating_evidence_falls_to_simplified() {
        assert_eq!(detect_language("山水人木火土金"), Some(Language::ZhHans));
    }

    /// No evidence is `None`, not English. The callers are choosing a DEFAULT,
    /// and "we could not tell" has to reach them so they can fall through to
    /// the next rung — an English answer here is the bug that served a
    /// "Published with moss" colophon to an empty Chinese vault.
    #[test]
    fn nothing_to_go_on_is_no_answer_rather_than_english() {
        assert_eq!(site_language_vote(&[]), None, "no documents at all");
        assert_eq!(site_language_vote(&["hi", "ok", ""]), None, "documents with no prose in them");
    }

    #[test]
    fn test_strip_frontmatter_removes_yaml() {
        // The body starts AFTER the closing delimiter's newline. The old local
        // scan split on the first `\n---` anywhere and handed back a stray
        // leading newline with it.
        let content = "---\ntitle: Hello\nlang: en\n---\nThis is the body text.";
        assert_eq!(strip_frontmatter(content), "This is the body text.");
    }

    #[test]
    fn strip_frontmatter_removes_the_simplified_dialect_too() {
        // Language is a majority vote over body text. These field lines used to
        // survive into it and skew the count (moss#937).
        let content = "nav\ntitle: Hello\n---\nThis is the body text.";
        assert_eq!(strip_frontmatter(content), "This is the body text.");
    }

    #[test]
    fn test_strip_frontmatter_no_frontmatter() {
        let content = "Just plain text with no frontmatter.";
        assert_eq!(strip_frontmatter(content), content);
    }

    #[test]
    fn test_detect_project_lang_english() {
        let dir = tempfile::tempdir().unwrap();
        let file1 = dir.path().join("post1.md");
        let file2 = dir.path().join("post2.md");
        std::fs::write(&file1, "---\ntitle: Hello\n---\nThis is a blog post about software engineering and web development.").unwrap();
        std::fs::write(&file2, "Another English post about web development, covering modern frameworks and best practices.").unwrap();

        let files = vec![
            crate::types::content::FileInfo { path: "post1.md".to_string(), file_type: "md".to_string(), size: 0, modified: None },
            crate::types::content::FileInfo { path: "post2.md".to_string(), file_type: "md".to_string(), size: 0, modified: None },
        ];
        assert_eq!(detect_project_language(&files, dir.path().to_str().unwrap(), &never_evicted), Some(Language::En));
    }

    #[test]
    fn test_detect_project_lang_chinese() {
        let dir = tempfile::tempdir().unwrap();
        let file1 = dir.path().join("post1.md");
        let file2 = dir.path().join("post2.md");
        std::fs::write(&file1, "---\ntitle: 你好\n---\n这是一篇关于软件工程和网页开发的博客文章。我们探讨了构建现代应用程序的各种技术。").unwrap();
        std::fs::write(&file2, "另一篇关于网页开发的中文文章，涵盖了现代框架和最佳实践。这是中文内容。").unwrap();

        let files = vec![
            crate::types::content::FileInfo { path: "post1.md".to_string(), file_type: "md".to_string(), size: 0, modified: None },
            crate::types::content::FileInfo { path: "post2.md".to_string(), file_type: "md".to_string(), size: 0, modified: None },
        ];
        assert_eq!(detect_project_language(&files, dir.path().to_str().unwrap(), &never_evicted), Some(Language::ZhHans));
    }

    #[test]
    fn test_detect_project_lang_empty_has_no_basis() {
        let files: Vec<crate::types::content::FileInfo> = vec![];
        // `None`, not English: an empty vault has no evidence, and it is
        // `resolve_site_default_lang` that decides what to do with that (the
        // system language, per its own tests) — the sampler does not guess.
        assert_eq!(detect_project_language(&files, "/nonexistent", &never_evicted), None);
    }

    /// Stage 3 (docs/archive/2026-07-31-cloud-download-waiting-mode.md): a
    /// cloud-dataless file must be skipped from the sample exactly like a
    /// read error, not read/blocked on. The injectable predicate lets this
    /// be tested without real `SF_DATALESS` state.
    #[test]
    fn detect_project_language_skips_an_evicted_file() {
        let dir = tempfile::tempdir().unwrap();
        let evicted_file = dir.path().join("chinese.md");
        let readable_file = dir.path().join("english.md");
        // Content doesn't matter for the evicted file — it must never be read.
        std::fs::write(&evicted_file, "这是一篇关于软件工程和网页开发的博客文章。我们探讨了构建现代应用程序的各种技术。").unwrap();
        std::fs::write(&readable_file, "This is an English article about programming and software engineering.").unwrap();

        let files = vec![
            crate::types::content::FileInfo { path: "chinese.md".to_string(), file_type: "md".to_string(), size: 0, modified: None },
            crate::types::content::FileInfo { path: "english.md".to_string(), file_type: "md".to_string(), size: 0, modified: None },
        ];
        let is_evicted = |p: &std::path::Path| p.ends_with("chinese.md");
        assert_eq!(
            detect_project_language(&files, dir.path().to_str().unwrap(), &is_evicted),
            Some(Language::En),
            "the evicted Chinese file must be skipped, leaving only the English file's vote"
        );
    }

    #[test]
    fn an_evicted_only_vault_yields_no_language_at_all() {
        // Not "English" — the distinction the whole system-language fallback
        // rests on. A vault whose every file is cloud-dataless has no basis for
        // a language decision, exactly like an empty one.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("only.md"), "This is a perfectly detectable English sentence.").unwrap();

        let files = vec![crate::types::content::FileInfo {
            path: "only.md".to_string(),
            file_type: "md".to_string(),
            size: 0,
            modified: None,
        }];
        let is_evicted = |_: &std::path::Path| true;
        assert_eq!(
            detect_project_language(&files, dir.path().to_str().unwrap(), &is_evicted),
            None
        );
    }

    #[test]
    fn site_default_lang_falls_back_to_system_when_no_content_basis() {
        // A signal-less vault (no markdown files / no detectable prose) must NOT
        // hard-code English: it falls back to the user's system language. This is
        // the "Published with moss" → 青苔发布 fix for empty/ambiguous Chinese sites.
        let files: Vec<crate::types::content::FileInfo> = vec![];
        assert_eq!(
            resolve_site_default_lang(None, None, &files, "/nonexistent", Language::ZhHans),
            "zh-hans"
        );
        assert_eq!(
            resolve_site_default_lang(None, None, &files, "/nonexistent", Language::En),
            "en"
        );
    }

    #[test]
    fn site_default_lang_prefers_explicit_config_over_everything() {
        let files: Vec<crate::types::content::FileInfo> = vec![];
        assert_eq!(
            resolve_site_default_lang(Some("zh-hant"), None, &files, "/nonexistent", Language::En),
            "zh-hant"
        );
    }

    #[test]
    fn a_language_moss_does_not_ship_survives_the_resolver() {
        // `[site] lang = "fr"` is a statement about the site, and it leaves the
        // machine: it labels the root email audience and rides in the
        // syndication payload. Routing the declared rungs through `Language`
        // would collapse them onto moss's three UI languages and relabel a
        // French site English on the way out. The render path does that
        // collapse itself, where "no translation" is what English means.
        let files: Vec<crate::types::content::FileInfo> = vec![];
        assert_eq!(
            resolve_site_default_lang(Some("fr"), None, &files, "/nonexistent", Language::En),
            "fr"
        );

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.md"), "---\nlang: ja\n---\nbody").unwrap();
        assert_eq!(
            resolve_site_default_lang(None, Some("index.md"), &files, dir.path().to_str().unwrap(), Language::En),
            "ja"
        );
    }

    #[test]
    fn site_default_lang_uses_detected_content_over_system() {
        // When the vault HAS real prose, content detection wins — the system
        // language is only the no-basis fallback, never an override.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("post.md"),
            "This is an English blog post about software engineering and modern web development.",
        )
        .unwrap();
        let files = vec![crate::types::content::FileInfo {
            path: "post.md".to_string(),
            file_type: "md".to_string(),
            size: 0,
            modified: None,
        }];
        // System says ZhHans, but the English content must win.
        assert_eq!(
            resolve_site_default_lang(None, None, &files, dir.path().to_str().unwrap(), Language::ZhHans),
            "en"
        );
    }

    #[test]
    fn site_default_lang_uses_chinese_content_even_when_system_is_english() {
        // Symmetric to the above: a Chinese vault on an English-locale machine
        // must resolve to ZhHans from content — the system language never
        // overrides a confident content detection.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("post.md"),
            "这是一篇关于软件工程和网页开发的博客文章。我们探讨了构建现代应用程序的各种技术与最佳实践。",
        )
        .unwrap();
        let files = vec![crate::types::content::FileInfo {
            path: "post.md".to_string(),
            file_type: "md".to_string(),
            size: 0,
            modified: None,
        }];
        assert_eq!(
            resolve_site_default_lang(None, None, &files, dir.path().to_str().unwrap(), Language::En),
            "zh-hans"
        );
    }
    #[test]
    fn homepage_frontmatter_lang_outranks_content_detection() {
        // A `lang:` in the homepage is an authoring declaration. It used to
        // reach the render path by being copied into `[site] lang`; with that
        // write gone the resolver reads it directly, and it must still beat a
        // confident detection of the other script.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("index.md"),
            "---\ntitle: 首页\nlang: zh-hant\n---\n这是一篇关于软件工程和网页开发的博客文章。我们探讨了构建现代应用程序的各种技术与最佳实践。",
        )
        .unwrap();
        let files = vec![crate::types::content::FileInfo {
            path: "index.md".to_string(),
            file_type: "md".to_string(),
            size: 0,
            modified: None,
        }];
        let root = dir.path().to_str().unwrap();
        assert_eq!(
            resolve_site_default_lang(None, Some("index.md"), &files, root, Language::En),
            "zh-hant"
        );
        // …and `[site] lang` still outranks the declaration, so an author who
        // sets it in config is not overruled by an old homepage line.
        assert_eq!(
            resolve_site_default_lang(Some("en"), Some("index.md"), &files, root, Language::ZhHans),
            "en"
        );
    }

    #[test]
    fn homepage_without_a_lang_declaration_falls_through_to_detection() {
        // Guards the rung against swallowing the ladder: no `lang:` means the
        // homepage says nothing, not that it says English.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("index.md"),
            "---\ntitle: 首页\n---\n这是一篇关于软件工程和网页开发的博客文章。我们探讨了构建现代应用程序的各种技术与最佳实践。",
        )
        .unwrap();
        let files = vec![crate::types::content::FileInfo {
            path: "index.md".to_string(),
            file_type: "md".to_string(),
            size: 0,
            modified: None,
        }];
        assert_eq!(
            resolve_site_default_lang(None, Some("index.md"), &files, dir.path().to_str().unwrap(), Language::En),
            "zh-hans"
        );
    }
}
