//! Minimal MHTML (`multipart/related`, RFC 2557) reader for local-file import.
//!
//! Browsers' "Save Page As → Web Page, Single File" (Chrome/Edge "Save by
//! Blink", Safari `.webarchive` is a different format) emit an MHTML file: one
//! MIME message whose parts are the main HTML plus every referenced asset,
//! encoded `quoted-printable` (text) or `base64` (binary). This reader pulls
//! out the main HTML document, the original page URL, and the embedded assets
//! so `moss import` can turn a saved snapshot into a markdown note without any
//! network access.
//!
//! Scope: the subset browsers actually emit — 7-bit-clean bodies (QP/base64),
//! folded headers, CRLF or LF line endings. Not a general MIME parser.

use std::collections::HashMap;

use base64::Engine;
use url::Url;

/// A decoded MHTML archive.
pub struct MhtmlDoc {
    /// The page's original URL, from the `Snapshot-Content-Location` top header
    /// (falls back to the main HTML part's `Content-Location`). `None` if absent.
    pub source_url: Option<String>,
    /// The decoded main `text/html` document.
    pub html: String,
    /// Embedded assets, keyed by BOTH their `Content-Location` (absolute URL)
    /// and their `Content-ID` (`cid:` reference, angle brackets stripped) so a
    /// lookup by either form used in the HTML resolves.
    pub resources: HashMap<String, MhtmlResource>,
}

/// One embedded, fully-decoded asset (image, css, …).
pub struct MhtmlResource {
    pub bytes: Vec<u8>,
    pub content_type: String,
}

/// Parse an MHTML byte stream into its main HTML document + embedded assets.
///
/// Returns `Err` if the input is not a `multipart/*` MIME message or has no
/// usable `text/html` part.
pub fn parse_mhtml(input: &[u8]) -> Result<MhtmlDoc, String> {
    // Whole-file ASCII: MHTML bodies are QP/base64 (7-bit). Lossy decode is
    // lossless here and lets us parse headers as text; binary only ever appears
    // *inside* a base64 body, which we decode to bytes explicitly.
    let text = String::from_utf8_lossy(input).replace("\r\n", "\n");

    let (top_headers_raw, body) = split_headers_body(&text)
        .ok_or_else(|| "MHTML: no header/body separator (blank line) found".to_string())?;
    let top = parse_headers(top_headers_raw);

    let content_type = top
        .get("content-type")
        .ok_or_else(|| "MHTML: missing top-level Content-Type".to_string())?;
    if !content_type.to_ascii_lowercase().contains("multipart/") {
        return Err(format!(
            "MHTML: top-level Content-Type is not multipart/*: {content_type}"
        ));
    }
    let boundary = extract_boundary(content_type)
        .ok_or_else(|| "MHTML: no boundary in multipart Content-Type".to_string())?;

    let source_url = top
        .get("snapshot-content-location")
        .map(|s| s.trim().to_string());

    let mut html: Option<String> = None;
    let mut resources: HashMap<String, MhtmlResource> = HashMap::new();

    for part in split_parts(body, &boundary) {
        let Some((ph_raw, pbody)) = split_headers_body(part) else {
            continue;
        };
        let ph = parse_headers(ph_raw);
        let ctype_full = ph.get("content-type").cloned().unwrap_or_default();
        let ctype = ctype_full
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        let cte = ph
            .get("content-transfer-encoding")
            .map(|s| s.trim().to_ascii_lowercase())
            .unwrap_or_default();
        let loc = ph.get("content-location").map(|s| s.trim().to_string());
        let cid = ph
            .get("content-id")
            .map(|s| s.trim().trim_start_matches('<').trim_end_matches('>').to_string());

        let bytes = decode_body(pbody, &cte);

        let is_main_html = ctype == "text/html"
            && (html.is_none()
                || (loc.is_some() && loc == source_url));
        if is_main_html {
            html = Some(String::from_utf8_lossy(&bytes).to_string());
            continue;
        }

        // Skip empty parts (a genuinely empty asset, or a base64 body that
        // failed to decode) — otherwise the importer would write a 0-byte file
        // and rewrite the image link to point at it. Leaving it out keeps the
        // original absolute URL in the markdown, which still resolves online.
        if bytes.is_empty() {
            continue;
        }

        let resource = MhtmlResource {
            bytes,
            content_type: if ctype.is_empty() {
                "application/octet-stream".to_string()
            } else {
                ctype.clone()
            },
        };
        // A resource may be referenced by Content-Location URL or by cid; key it
        // under whichever it carries so a lookup by either resolves. Bytes are
        // cloned only when a part carries multiple keys (rare — most carry one).
        match (loc, cid) {
            (Some(l), Some(c)) => {
                resources.insert(format!("cid:{c}"), clone_resource(&resource));
                insert_by_location(&mut resources, &l, resource);
            }
            (Some(l), None) => insert_by_location(&mut resources, &l, resource),
            (None, Some(c)) => {
                resources.insert(format!("cid:{c}"), resource);
            }
            (None, None) => {}
        }
    }

    let html = html.ok_or_else(|| "MHTML: no text/html part found".to_string())?;
    let source_url = source_url.filter(|s| !s.is_empty());
    Ok(MhtmlDoc {
        source_url,
        html,
        resources,
    })
}

fn clone_resource(r: &MhtmlResource) -> MhtmlResource {
    MhtmlResource {
        bytes: r.bytes.clone(),
        content_type: r.content_type.clone(),
    }
}

/// Insert a resource keyed by its Content-Location. Markdown image URLs are
/// normalized through `Url::parse().to_string()` (see `converter::resolve_url`),
/// so the resource is keyed the same way; when the raw and normalized forms
/// differ (non-canonical host/port/percent-encoding) both are stored so a
/// lookup by either resolves.
fn insert_by_location(
    resources: &mut HashMap<String, MhtmlResource>,
    loc: &str,
    resource: MhtmlResource,
) {
    match Url::parse(loc).ok().map(|u| u.to_string()) {
        Some(normalized) if normalized != loc => {
            resources.insert(normalized, clone_resource(&resource));
            resources.insert(loc.to_string(), resource);
        }
        _ => {
            resources.insert(loc.to_string(), resource);
        }
    }
}

/// Split a MIME chunk into (headers, body) at the first blank line.
fn split_headers_body(chunk: &str) -> Option<(&str, &str)> {
    // Body starts after the first empty line. Chunk is already LF-normalized.
    chunk.split_once("\n\n")
}

/// Parse (LF-normalized) header lines into a lowercased-name → value map,
/// unfolding RFC 822 continuation lines (leading space/tab).
fn parse_headers(raw: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut current: Option<(String, String)> = None;
    for line in raw.split('\n') {
        if line.is_empty() {
            continue;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            // Continuation of the previous header value.
            if let Some((_, v)) = current.as_mut() {
                v.push_str(line.trim_start());
            }
            continue;
        }
        // Flush the previous header.
        if let Some((k, v)) = current.take() {
            map.entry(k).or_insert(v);
        }
        if let Some((name, value)) = line.split_once(':') {
            current = Some((name.trim().to_ascii_lowercase(), value.trim().to_string()));
        }
    }
    if let Some((k, v)) = current.take() {
        map.entry(k).or_insert(v);
    }
    map
}

/// Pull the boundary token out of a `multipart/*` Content-Type value.
fn extract_boundary(content_type: &str) -> Option<String> {
    // The parameter name is matched case-insensitively; `to_ascii_lowercase`
    // is length-preserving, so the offset is valid in the original too.
    let pos = content_type.to_ascii_lowercase().find("boundary=")?;
    let rest = content_type.get(pos + "boundary=".len()..)?.trim_start();
    if let Some(stripped) = rest.strip_prefix('"') {
        // Quoted: read up to the closing quote.
        stripped.split_once('"').map(|(value, _)| value.to_string())
    } else {
        // Token: read up to the next `;` or whitespace.
        Some(
            rest.split_once(|c: char| c == ';' || c.is_whitespace())
                .map_or(rest, |(value, _)| value)
                .to_string(),
        )
    }
}

/// Split a multipart body into its parts (excluding preamble/epilogue and the
/// closing delimiter).
///
/// Per RFC 2046 a boundary only delimits at the start of a line, so a
/// `--boundary` substring that appears *inside* a part's content (e.g. a page
/// quoting the archive's own boundary token) must not split the part. We match
/// the marker only at offset 0 or immediately after a `\n`.
fn split_parts<'a>(body: &'a str, boundary: &str) -> Vec<&'a str> {
    let marker = format!("--{boundary}");
    let bytes = body.as_bytes();

    // Byte offsets where a boundary marker begins a line.
    let starts: Vec<usize> = body
        .match_indices(&marker)
        .filter(|&(pos, _)| pos == 0 || bytes.get(pos - 1) == Some(&b'\n'))
        .map(|(pos, _)| pos)
        .collect();

    let mut parts = Vec::new();
    for (i, &start) in starts.iter().enumerate() {
        let Some(after) = body.get(start + marker.len()..) else {
            continue;
        };
        // `--boundary--` closes the multipart.
        if after.starts_with("--") {
            break;
        }
        // The part body starts after this boundary line's newline and runs up to
        // the next boundary; exclude the `\n` that precedes that next boundary.
        let Some((_boundary_line, rest)) = after.split_once('\n') else {
            continue;
        };
        let part_start = body.len() - rest.len();
        let part_end = starts.get(i + 1).copied().unwrap_or(body.len());
        let Some(part) = body.get(part_start..part_end) else {
            continue;
        };
        parts.push(part.strip_suffix('\n').unwrap_or(part));
    }
    parts
}

/// Decode a part body according to its Content-Transfer-Encoding.
fn decode_body(body: &str, cte: &str) -> Vec<u8> {
    if cte.contains("quoted-printable") {
        decode_quoted_printable(body)
    } else if cte.contains("base64") {
        let compact: String = body.split_whitespace().collect();
        base64::engine::general_purpose::STANDARD
            .decode(compact.as_bytes())
            .unwrap_or_default()
    } else {
        // 7bit / 8bit / binary / none.
        body.as_bytes().to_vec()
    }
}

/// Decode a quoted-printable body (LF-normalized) to bytes.
fn decode_quoted_printable(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'=' {
            // Soft line break: `=` at end of line.
            if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                i += 2;
                continue;
            }
            // `=XX` hex escape.
            if i + 2 < bytes.len() {
                if let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                    out.push(h * 16 + l);
                    i += 3;
                    continue;
                }
            }
            out.push(b'=');
            i += 1;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'A'..=b'F' => Some(b - b'A' + 10),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multipart_related_html_and_source_url() {
        let mhtml = b"From: <Saved by Blink>\r\n\
Snapshot-Content-Location: https://www.douban.com/note/1/\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/related;\r\n\ttype=\"text/html\";\r\n\tboundary=\"BOUND\"\r\n\
\r\n\
--BOUND\r\n\
Content-Type: text/html\r\n\
Content-Transfer-Encoding: quoted-printable\r\n\
Content-Location: https://www.douban.com/note/1/\r\n\
\r\n\
<html><body><p>hi=20there=\r\nand more</p></body></html>\r\n\
--BOUND--\r\n";
        let doc = parse_mhtml(mhtml).unwrap();
        assert_eq!(
            doc.source_url.as_deref(),
            Some("https://www.douban.com/note/1/")
        );
        assert!(
            doc.html.contains("<p>hi there"),
            "QP `=20` should decode to a space; got: {}",
            doc.html
        );
        assert!(
            doc.html.contains("thereand more"),
            "QP soft line break should join lines; got: {}",
            doc.html
        );
    }

    #[test]
    fn extracts_base64_image_resource_by_content_location() {
        // 1x1 transparent PNG.
        let png_b64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
        let mhtml = format!(
            "MIME-Version: 1.0\r\n\
Content-Type: multipart/related; boundary=\"B\"\r\n\
\r\n\
--B\r\n\
Content-Type: text/html\r\n\
Content-Location: https://x/\r\n\
\r\n\
<img src=\"https://x/a.png\">\r\n\
--B\r\n\
Content-Type: image/png\r\n\
Content-Transfer-Encoding: base64\r\n\
Content-Location: https://x/a.png\r\n\
\r\n\
{png_b64}\r\n\
--B--\r\n"
        );
        let doc = parse_mhtml(mhtml.as_bytes()).unwrap();
        let r = doc
            .resources
            .get("https://x/a.png")
            .expect("resource keyed by Content-Location");
        assert_eq!(r.content_type, "image/png");
        assert_eq!(
            &r.bytes[..8],
            &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a],
            "decoded bytes must start with the PNG signature"
        );
    }

    #[test]
    fn unfolds_folded_boundary_header() {
        let mhtml = b"MIME-Version: 1.0\r\n\
Content-Type: multipart/related;\r\n\tboundary=\"WRAP\"\r\n\
\r\n\
--WRAP\r\n\
Content-Type: text/html\r\n\
\r\n\
<p>ok</p>\r\n\
--WRAP--\r\n";
        let doc = parse_mhtml(mhtml).unwrap();
        assert!(doc.html.contains("<p>ok</p>"), "got: {}", doc.html);
    }

    #[test]
    fn rejects_non_multipart_input() {
        assert!(parse_mhtml(b"just some text, not a MIME message\r\n\r\nbody").is_err());
    }

    #[test]
    fn boundary_token_inside_content_does_not_split() {
        // A `--B` substring inside the HTML (mid-line, not a boundary line) must
        // not truncate the part. RFC 2046: boundaries delimit only at line start.
        let mhtml = b"MIME-Version: 1.0\r\n\
Content-Type: multipart/related; boundary=\"B\"\r\n\
\r\n\
--B\r\n\
Content-Type: text/html\r\n\
\r\n\
<p>the marker --B appears inline and must survive</p>\r\n\
--B--\r\n";
        let doc = parse_mhtml(mhtml).unwrap();
        assert!(
            doc.html.contains("marker --B appears inline and must survive"),
            "html was truncated at an inline boundary token: {}",
            doc.html
        );
    }

    #[test]
    fn base64_decode_failure_skips_resource_instead_of_writing_empty() {
        // A part whose base64 body is invalid must not become a 0-byte resource
        // (which the importer would write to disk and link to). It's dropped, so
        // the original URL stays in the HTML.
        let mhtml = b"MIME-Version: 1.0\r\n\
Content-Type: multipart/related; boundary=\"B\"\r\n\
\r\n\
--B\r\n\
Content-Type: text/html\r\n\
Content-Location: https://x/\r\n\
\r\n\
<img src=\"https://x/bad.png\">\r\n\
--B\r\n\
Content-Type: image/png\r\n\
Content-Transfer-Encoding: base64\r\n\
Content-Location: https://x/bad.png\r\n\
\r\n\
!!! not valid base64 !!!\r\n\
--B--\r\n";
        let doc = parse_mhtml(mhtml).unwrap();
        assert!(
            !doc.resources.contains_key("https://x/bad.png"),
            "undecodable base64 part must be dropped, not stored empty"
        );
    }
}
