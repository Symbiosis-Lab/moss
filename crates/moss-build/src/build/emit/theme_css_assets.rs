//! Content-hashed addresses for the files a theme stylesheet points at.
//!
//! A host can let browsers keep a file indefinitely when its name changes
//! whenever its bytes do. The theme's own `style.<hash>.css` already has such
//! a name; the fonts and pictures its `url()` references did not, so a file
//! replaced at the same address could still be answered from an old copy.
//!
//! This rewrites each `url()` in `.moss/theme/style.css` that names a file
//! inside the theme folder to `name.<hash>.ext` (the author's relative path
//! and quoting are kept), and hands back the bytes to publish at
//! `/_moss/theme/<folders>/<stem>.<16 hex>.<ext>`. A stylesheet brought in by
//! `@import` is rewritten the same way and hashed afterwards, so a font
//! changed behind an import renames every sheet on the way to it. The plain copies under
//! `/_moss/theme/…` are still written by the asset mirror: pages and authored
//! HTML may point at them. The hash is the same xxh3 the stylesheet's own name
//! uses (`compute_binary_hash`), and the stylesheet is hashed AFTER this
//! rewrite, so changing a font also changes the stylesheet's address.

use crate::build::assets::paths::compute_binary_hash;
use crate::build::context::BuildContext;
use crate::build::manifest::{HashBucket, PendingManifest};
use crate::build::served_path::ServedPath;
use crate::build::theme_lint::{comment_end, string_end};
use std::collections::BTreeMap;
use std::path::Path;

/// A file larger than this keeps its plain name. Each hashed copy is read
/// whole and written as a second full copy beside the plain one (the asset
/// mirror links its copies from the content store instead), so a `url()` to a
/// video would double it in the output and the upload. Fonts and pictures are
/// far below it.
const MAX_HASHED_FILE_BYTES: u64 = 5 * 1024 * 1024;

/// The rewritten stylesheet and the hashed files it now points at.
pub struct ThemeCss {
    pub css: String,
    pub files: Vec<(ServedPath, Vec<u8>)>,
    /// The stylesheets `css` imports from the theme folder, as written.
    pub imported: Vec<String>,
}

impl ThemeCss {
    /// Write the rewritten stylesheet at its hashed address and the hashed
    /// files beside it.
    pub fn emit(&self, output_dir: &Path, pending: &mut PendingManifest) -> Result<(), String> {
        let sheet = ServedPath::for_custom_stylesheet_hashed(&compute_binary_hash(self.css.as_bytes()));
        let stylesheet = (&sheet, self.css.as_bytes());
        let hashed = self.files.iter().map(|(path, bytes)| (path, bytes.as_slice()));
        for (path, bytes) in std::iter::once(stylesheet).chain(hashed) {
            BuildContext::for_render(output_dir, pending)
                .emit(path, bytes, HashBucket::Files)
                .map_err(|e| format!("Failed to emit {}: {}", path.as_str(), e))?;
        }
        Ok(())
    }
}

/// [`rewrite`] over the site's `.moss/theme/` folder. A file that is not
/// there, is a symlink or resolves outside the folder, is over
/// [`MAX_HASHED_FILE_BYTES`], or whose bytes are still in the cloud keeps its
/// written address; a cloud one is recorded as pending so the finished build
/// waits for it. The resolved path must equal the written one exactly, so on a
/// filesystem that ignores case a reference spelled in a different case than
/// the file (`Fonts/A.woff2` for `fonts/a.woff2`) also keeps its written address.
pub fn for_site(css: &str, source_root: &Path, evidence: &crate::build::cloud_ledger::InputEvidence) -> ThemeCss {
    use crate::build::cloud_readiness::{probe_input, Found};
    let theme_dir = crate::moss_paths::MossPaths::new(source_root).theme_dir();
    let real_theme = theme_dir.canonicalize().ok();
    let mut themed = rewrite(css, |rel| {
        let path = theme_dir.join(rel);
        // The plain copy step does not follow symlinks; neither does this, or
        // a link pointing outside the folder would publish its target.
        let meta = std::fs::symlink_metadata(&path).ok()?;
        if !meta.is_file() || meta.len() > MAX_HASHED_FILE_BYTES {
            return None;
        }
        if path.canonicalize().ok()? != real_theme.as_ref()?.join(rel) {
            return None;
        }
        match probe_input(&path) {
            Ok(Found::Bytes(bytes)) => Some(bytes),
            Ok(Found::InCloud) => {
                evidence.require(&path, crate::build::cloud_ledger::InputRole::Theme);
                evidence.pending(&path);
                None
            }
            Err(_) => None,
        }
    });
    let derived = super::theme_font_weights::rules(css, &themed.imported);
    themed.css.push_str(&derived);
    // The stylesheet's recorded source hash is the rewritten sheet's (see the
    // render pass), so the ledger's read of the file advances with it.
    if themed.css != css {
        evidence.owned_rewrite(&theme_dir.join("style.css"), css.as_bytes(), themed.css.as_bytes());
    }
    themed
}

/// Rewrite every in-scope reference in `css`. `read` returns the bytes of a
/// theme-relative file (`fonts/a.woff2`), or `None` to leave the reference as
/// written. A stylesheet it points at (`@import`) is rewritten the same way
/// before it is hashed, its own references read from its own folder.
pub fn rewrite(css: &str, read: impl FnMut(&str) -> Option<Vec<u8>>) -> ThemeCss {
    let mut rewriter = Rewriter { read, by_rel: BTreeMap::new(), files: Vec::new(), open: Vec::new(), imported: Vec::new() };
    let css = rewriter.sheet(css, "");
    ThemeCss { css, files: rewriter.files, imported: rewriter.imported }
}

struct Rewriter<R> {
    read: R,
    /// Theme-relative path to its hash, or `None` for one left as written.
    by_rel: BTreeMap<String, Option<String>>,
    files: Vec<(ServedPath, Vec<u8>)>,
    /// The imported stylesheets being rewritten, outermost first.
    open: Vec<String>,
    imported: Vec<String>,
}

impl<R: FnMut(&str) -> Option<Vec<u8>>> Rewriter<R> {
    /// `css` with its references rewritten; `dir` is the theme-relative folder
    /// the sheet sits in, which its relative references start from.
    fn sheet(&mut self, css: &str, dir: &str) -> String {
        let b = css.as_bytes();
        let mut out = String::with_capacity(css.len());
        let mut copied = 0;
        let mut i = 0;
        // A bare string before this index names a file: inside `image-set()`,
        // or straight after `@import`.
        let mut strings_name_files_until = 0;

        while i < b.len() {
            match b[i] {
                b'/' if b.get(i + 1) == Some(&b'*') => i = comment_end(b, i),
                q @ (b'"' | b'\'') => {
                    let end = string_end(b, i, q);
                    if i < strings_name_files_until && end < b.len() {
                        if let Some(new) = self.reference(&css[i + 1..end], dir) {
                            out.push_str(&css[copied..i + 1]);
                            out.push_str(&new);
                            copied = end;
                        }
                    }
                    i = (end + 1).min(b.len());
                }
                _ if starts_with_ci(b, i, b"url(") && !ident_before(b, i, true) => {
                    let mut j = i + 4;
                    while j < b.len() && b[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    let (start, end) = match b.get(j) {
                        Some(&q @ (b'"' | b'\'')) => (j + 1, string_end(b, j, q)),
                        _ => (j, css[j..].find(')').map_or(b.len(), |p| j + p)),
                    };
                    if end >= b.len() {
                        break;
                    }
                    // Unquoted: trailing space before `)` is not part of the name.
                    let inner = if matches!(b[j], b'"' | b'\'') { &css[start..end] } else { css[start..end].trim_end() };
                    if let Some(new) = self.reference(inner, dir) {
                        out.push_str(&css[copied..start]);
                        out.push_str(&new);
                        copied = start + inner.len();
                    }
                    i = (end + 1).min(b.len());
                }
                _ if starts_with_ci(b, i, b"image-set(") && !ident_before(b, i, false) => {
                    strings_name_files_until = paren_end(b, i + 9);
                    i += 10;
                }
                _ if starts_with_ci(b, i, b"@import") => {
                    i += 7;
                    while i < b.len() && b[i].is_ascii_whitespace() {
                        i += 1;
                    }
                    strings_name_files_until = strings_name_files_until.max(i + 1);
                }
                _ => i += 1,
            }
        }
        out.push_str(&css[copied..]);
        out
    }

    /// The bytes to publish for the theme file `rel`: a stylesheet's are its
    /// rewritten text, anything else's are the file's own. `None` leaves the
    /// reference as written, as does a stylesheet that imports itself.
    fn published_bytes(&mut self, rel: &str, dir: &str, ext: &str) -> Option<Vec<u8>> {
        let bytes = (self.read)(rel)?;
        if !ext.eq_ignore_ascii_case(".css") {
            return Some(bytes);
        }
        if self.open.iter().any(|o| o == rel) {
            return None;
        }
        let Ok(css) = String::from_utf8(bytes) else { return None };
        self.open.push(rel.to_string());
        let rewritten = self.sheet(&css, dir);
        self.open.pop();
        self.imported.push(css);
        Some(rewritten.into_bytes())
    }

    fn reference(&mut self, raw: &str, sheet_dir: &str) -> Option<String> {
        let raw = raw.trim();
        if raw.is_empty() || raw.starts_with(['/', '#']) || raw.contains(['\\', '(', ')']) || has_scheme(raw) {
            return None;
        }
        let cut = raw.find(['?', '#']).unwrap_or(raw.len());
        let (written, suffix) = raw.split_at(cut);
        // An encoded separator would be carried into the new name as a path.
        let lower = written.to_ascii_lowercase();
        if lower.contains("%2f") || lower.contains("%5c") {
            return None;
        }
        let decoded = percent_encoding::percent_decode_str(written).decode_utf8().ok()?;
        let rel = normalize(&format!("{sheet_dir}/{decoded}"))?;
        // The entry stylesheet and script are emitted by the render pass under
        // their own hashed names.
        if rel == "style.css" || rel == "script.js" {
            return None;
        }
        let written_base = written.rsplit('/').next()?;
        let (dir, name) = rel.rsplit_once('/').unwrap_or(("", &rel));
        // Both spellings need an extension to put the hash before.
        let (written_stem, written_ext) = split_ext(written_base)?;
        let (stem, ext) = split_ext(name)?;
        let hash = match self.by_rel.get(&rel) {
            Some(known) => known.clone()?,
            None => {
                let resolved = self.published_bytes(&rel, dir, ext).and_then(|bytes| {
                    let hash = compute_binary_hash(&bytes);
                    let hashed = format!("{stem}.{hash}{ext}");
                    let hashed_rel = if dir.is_empty() { hashed } else { format!("{dir}/{hashed}") };
                    self.files.push((ServedPath::for_theme_asset(&hashed_rel).ok()?, bytes));
                    Some(hash)
                });
                self.by_rel.insert(rel.clone(), resolved.clone());
                resolved?
            }
        };
        Some(format!("{}{written_stem}.{hash}{written_ext}{suffix}", &written[..written.len() - written_base.len()]))
    }
}

/// `("a", ".png")` for `a.png`; `None` without an extension or for `.hidden`.
fn split_ext(name: &str) -> Option<(&str, &str)> {
    let dot = name.rfind('.').filter(|&d| d > 0)?;
    Some(name.split_at(dot))
}

/// `a/./b/../c` to `a/c`; `None` when it climbs out of the folder.
fn normalize(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// `scheme:` before any `/`, per RFC 3986.
fn has_scheme(s: &str) -> bool {
    match s.find(':') {
        Some(c) => {
            let head = &s[..c];
            head.starts_with(|ch: char| ch.is_ascii_alphabetic())
                && head.chars().all(|ch| ch.is_ascii_alphanumeric() || "+-.".contains(ch))
        }
        None => false,
    }
}

fn starts_with_ci(b: &[u8], i: usize, needle: &[u8]) -> bool {
    b.len() >= i + needle.len() && b[i..i + needle.len()].eq_ignore_ascii_case(needle)
}

/// Is the byte before `i` part of a longer identifier (`my-url(`)? A `-`
/// counts for `url(` but not for `-webkit-image-set(`.
fn ident_before(b: &[u8], i: usize, dash_counts: bool) -> bool {
    i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_' || (dash_counts && b[i - 1] == b'-'))
}

/// Index of the `)` matching the `(` at `open`, or the end.
fn paren_end(b: &[u8], open: usize) -> usize {
    let mut depth = 0;
    let mut j = open;
    while j < b.len() {
        match b[j] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return j;
                }
            }
            q @ (b'"' | b'\'') => j = string_end(b, j, q),
            _ => {}
        }
        j += 1;
    }
    b.len()
}

#[cfg(test)]
#[path = "theme_css_assets_tests.rs"]
mod tests;
