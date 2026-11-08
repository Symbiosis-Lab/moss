//! The browser UI's own files: a static table the host embeds, served
//! read-only. `/` is `index.html`.
use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    response::{IntoResponse, Response},
    Router,
};

pub struct UiAsset {
    /// Relative to `/`, with `/` separators.
    pub path: &'static str,
    pub bytes: &'static [u8],
    pub content_type: &'static str,
    /// `bytes` are gzip-compressed and go out with `Content-Encoding: gzip`.
    pub gzip: bool,
    /// The file name carries a content hash, so a browser may keep it forever.
    /// The entry page never does: it names the current hashed files.
    pub immutable: bool,
}

pub(crate) fn router(assets: &'static [UiAsset]) -> Router {
    Router::new().fallback(move |request: Request<Body>| async move {
        if !matches!(*request.method(), Method::GET | Method::HEAD) {
            return StatusCode::METHOD_NOT_ALLOWED.into_response();
        }
        let path = request.uri().path().trim_start_matches('/');
        let path = if path.is_empty() { "index.html" } else { path };
        match assets.iter().find(|asset| asset.path == path) {
            Some(asset) => response(asset),
            None => StatusCode::NOT_FOUND.into_response(),
        }
    })
}

fn response(asset: &UiAsset) -> Response {
    let cache = if asset.immutable {
        "public, max-age=31536000, immutable"
    } else {
        "no-store"
    };
    let mut response = (
        [
            ("content-type", asset.content_type),
            ("cache-control", cache),
            ("x-content-type-options", "nosniff"),
        ],
        asset.bytes,
    )
        .into_response();
    if asset.gzip {
        response
            .headers_mut()
            .insert("content-encoding", "gzip".parse().unwrap());
    }
    response
}
