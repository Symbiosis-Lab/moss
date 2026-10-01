//! `POST /__moss/upload` — land browser-uploaded bytes in the vault.
//!
//! The desktop editor inserts an image by opening a native file picker and
//! handing the absolute paths it returns to the copy-into-vault core
//! (`editor::copy_in::copy_files_into`). A browser has no native picker and no
//! absolute paths — only an `<input type="file">` and bytes — so this route
//! stages each uploaded part as a temp file and hands the SAME core the SAME
//! shape of input, so naming, collision handling and the response shape are
//! identical on both carriers. One core, two carriers, like
//! `/__moss/source/*path` (see `router.rs`).
//!
//! An infrastructure route, registered unconditionally beside health, source,
//! comments, yield and session (`router.rs`'s ordering rule), never a carrier
//! command — it has no frontend binding and is documented here rather than in
//! `invoke.rs`'s command allowlists. Unlike those read/no-auth routes, it
//! mutates the vault, so it wears the SAME per-session token gate the
//! mutation carrier does.
//!
//! ## Contract
//!
//! * **Request** — `multipart/form-data` with one or more `file` parts (each
//!   carrying a filename) and an optional text field `dir` naming the
//!   project-relative target directory (defaults to the vault root).
//! * **Auth** — the same per-vault bearer token the mutation/read carriers
//!   check ([`super::carrier_token::admit`]), read from the header or the
//!   session cookie. No token, a wrong token, or no carrier bound at all
//!   (this server was started with no [`super::invoke::InvokeCtx`]) → **401**.
//! * **400** — a `file` part with no filename; a filename containing a path
//!   separator or `..`; a `dir` that resolves outside the vault (absolute or
//!   via `..` — [`crate::editor::copy_in::validate_copy_target`], the same
//!   guard `copy_files_into`'s desktop caller validates its target through);
//!   more than [`MAX_UPLOAD_PARTS`] file parts; a body that is not valid
//!   `multipart/form-data`.
//! * **413** — a `file` part exceeds [`MAX_UPLOAD_PART_BYTES`].
//! * **200** — the same JSON shape the desktop's copy-into-vault command
//!   returns: `{"copied":[{"original_name","final_name","final_path",
//!   "relative_path"}, …], "skipped":[…]}`.
//!
//! Never GET: like the other mutating routes this is POST-only, enforced
//! structurally by `axum::routing::post` at the router. This route's own
//! whole-request body cap ([`TOTAL_MAX_UPLOAD_BYTES`], `router.rs`) replaces
//! axum's 2 MiB default for it alone; [`MAX_UPLOAD_PART_BYTES`] and
//! [`MAX_UPLOAD_PARTS`] are the limits that actually bite first, checked
//! while streaming rather than after buffering the whole request.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use axum::{
    body::Body,
    extract::{FromRequest, Multipart},
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
};

/// Ceiling on one multipart `file` part, in bytes. A constant rather than a
/// config value — a later slice may make it configurable. 64 MiB comfortably
/// covers a photo straight off a phone camera without holding an unbounded
/// amount of attacker-controlled data in memory per part.
#[cfg(not(test))]
pub(crate) const MAX_UPLOAD_PART_BYTES: usize = 64 * 1024 * 1024;
/// Test-only override, bigger than axum's 2 MiB whole-request default so a
/// test can prove that default no longer applies to this route, small enough
/// to stream through a loopback socket quickly.
#[cfg(test)]
pub(crate) const MAX_UPLOAD_PART_BYTES: usize = 3 * 1024 * 1024;

/// Ceiling on how many `file` parts one request may carry. A request this
/// route size-checks per part still has to stop somewhere on *count*, or a
/// caller sending thousands of tiny parts spends unbounded staging I/O.
pub(crate) const MAX_UPLOAD_PARTS: usize = 64;

/// Whole-request body cap for this route alone, replacing axum's 2 MiB
/// default (`router.rs`). Comfortably above [`MAX_UPLOAD_PART_BYTES`] times a
/// handful of parts; [`MAX_UPLOAD_PART_BYTES`] and [`MAX_UPLOAD_PARTS`] are
/// the limits that actually bite first.
pub(crate) const TOTAL_MAX_UPLOAD_BYTES: usize = 256 * 1024 * 1024;

/// Handler for `POST /__moss/upload`. See the module doc for the contract.
pub(crate) async fn handle_upload(
    ctx: Option<super::invoke::InvokeCtx>,
    site_dir: Arc<RwLock<PathBuf>>,
    request: Request<Body>,
) -> Response {
    let Some(ctx) = ctx else {
        super::trust_boundary::drain_body(request.into_body()).await;
        return super::carrier_token::unauthorized();
    };
    let session = match super::carrier_token::admit(&ctx, &site_dir, &request) {
        Ok(session) => session,
        Err(refusal) => {
            super::trust_boundary::drain_body(request.into_body()).await;
            return refusal;
        }
    };

    let project_root = session.vault().path().to_path_buf();

    let mut multipart = match Multipart::from_request(request, &()).await {
        Ok(m) => m,
        Err(rejection) => return rejection.into_response(),
    };

    // A bare OS-temp-dir child, not the `tempfile` crate: that crate is a
    // dev-only dependency here, pulled in for exactly the ported unit tests
    // this route also carries, and `StagingDir`'s `Drop` gives the same
    // guaranteed cleanup in a few lines.
    let staging = match StagingDir::new() {
        Ok(dir) => dir,
        Err(e) => return server_error(&format!("could not stage the upload: {e}")),
    };

    let mut dir_field: Option<String> = None;
    let mut source_paths: Vec<String> = Vec::new();

    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => return e.into_response(),
        };
        let field_name = field.name().unwrap_or("").to_string();

        if field_name == "dir" {
            match field.text().await {
                Ok(text) => dir_field = Some(text),
                Err(e) => return e.into_response(),
            }
            continue;
        }
        if field_name != "file" {
            // An unrecognized field is ignored rather than rejected, so a
            // form that also carries e.g. a CSRF-style hidden field does not
            // need this route to know its name.
            continue;
        }

        if source_paths.len() >= MAX_UPLOAD_PARTS {
            return bad_request(&format!("a single upload carries at most {MAX_UPLOAD_PARTS} files"));
        }

        let Some(filename) = field.file_name().map(str::to_string) else {
            return bad_request("a 'file' part must carry a filename");
        };
        if let Err(msg) = reject_unsafe_filename(&filename) {
            return bad_request(&msg);
        }

        // Each part gets its own numbered slot so two parts sharing a
        // filename in one request cannot overwrite each other — the basename
        // itself must stay exactly as uploaded, because `copy_files_into`
        // reads it back off the staged path to name the result.
        let slot = staging.path().join(source_paths.len().to_string());
        if let Err(e) = std::fs::create_dir_all(&slot) {
            return server_error(&format!("could not stage the upload: {e}"));
        }
        let staged_path = slot.join(&filename);
        let mut staged_file = match std::fs::File::create(&staged_path) {
            Ok(f) => f,
            Err(e) => return server_error(&format!("could not stage the upload: {e}")),
        };

        // Written to the staged file as it streams in, never buffered whole
        // in memory first — many parts each just under the per-part cap must
        // not add up to an unbounded in-memory total.
        let mut field = field;
        let mut written = 0usize;
        loop {
            match field.chunk().await {
                Ok(Some(chunk)) => {
                    written += chunk.len();
                    if written > MAX_UPLOAD_PART_BYTES {
                        return Response::builder()
                            .status(StatusCode::PAYLOAD_TOO_LARGE)
                            .header("content-type", "text/plain; charset=utf-8")
                            .header("cache-control", "no-store")
                            .body(Body::from(format!(
                                "upload part exceeds the {MAX_UPLOAD_PART_BYTES}-byte limit"
                            )))
                            .expect("static 413 response is always valid");
                    }
                    if let Err(e) = std::io::Write::write_all(&mut staged_file, &chunk) {
                        return server_error(&format!("could not stage the upload: {e}"));
                    }
                }
                Ok(None) => break,
                Err(e) => return e.into_response(),
            }
        }
        source_paths.push(staged_path.to_string_lossy().into_owned());
    }

    if source_paths.is_empty() {
        return bad_request("the upload carried no 'file' part");
    }

    let dir_rel = dir_field.unwrap_or_default();
    // `validate_copy_target` (like every first-party path guard here) expects
    // a path that already starts with the project root — `dir` on the wire is
    // project-root-relative only (the same convention `invoke.rs`'s `confine`
    // uses for its path-shaped arguments), so it is always joined onto the
    // root, never trusted as absolute. `Path::join` replaces the base
    // entirely when the joined path is itself absolute, so a caller-supplied
    // absolute `dir` still ends up checked against — and rejected by — the
    // project-root containment test below, exactly like a relative `dir` that
    // walks out via `..`: the join is textual, not lexically normalized, so a
    // `..` segment survives into the traversal check rather than being
    // silently resolved away by it.
    let dir_for_target = project_root.join(&dir_rel).to_string_lossy().into_owned();
    let target = match crate::editor::copy_in::validate_copy_target(&project_root, &dir_for_target) {
        Ok(t) => t,
        Err(e) => return bad_request(&e),
    };

    let report = crate::editor::copy_in::copy_files_into(&project_root, &source_paths, &target);
    // `staging`'s `Drop` removes every temp file, landed or not.
    drop(staging);

    let body = serde_json::to_string(&report).unwrap_or_else(|_| "{}".to_string());
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json; charset=utf-8")
        .header("cache-control", "no-store")
        .body(Body::from(body))
        .expect("a serialized CopyReport is always a valid response body")
}

/// A unique scratch directory under the OS temp dir, removed recursively on
/// drop — including on every early-return error path above, since `?`-style
/// returns run destructors on the way out.
struct StagingDir(PathBuf);

impl StagingDir {
    fn new() -> std::io::Result<Self> {
        let path = std::env::temp_dir().join(format!("moss-upload-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for StagingDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Reject an empty filename, or one carrying a path separator or `..`, before
/// it ever becomes a path component on disk.
fn reject_unsafe_filename(name: &str) -> Result<(), String> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(format!("'{name}' is not a valid upload filename"));
    }
    Ok(())
}

fn bad_request(message: &str) -> Response {
    Response::builder()
        .status(StatusCode::BAD_REQUEST)
        .header("content-type", "text/plain; charset=utf-8")
        .header("cache-control", "no-store")
        .body(Body::from(message.to_string()))
        .expect("static 400 response is always valid")
}

fn server_error(message: &str) -> Response {
    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .header("content-type", "text/plain; charset=utf-8")
        .header("cache-control", "no-store")
        .body(Body::from(message.to_string()))
        .expect("static 500 response is always valid")
}

#[cfg(test)]
mod tests {
    use super::super::carrier_token::TOKEN_HEADER;
    use super::super::invoke::InvokeCtx;
    use super::super::ownership::HostKind;
    use super::super::router::{start_server, ServeConfig};
    use std::sync::{Arc, RwLock};

    fn served_vault() -> (tempfile::TempDir, std::path::PathBuf) {
        let vault = tempfile::TempDir::new_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let site_dir = vault.path().join(".moss/build.nosync/current");
        std::fs::create_dir_all(&site_dir).unwrap();
        (vault, site_dir)
    }

    /// Hand-roll a `multipart/form-data` body: text fields first, then file
    /// parts, matching the field order a real `<form>` submits. `ureq` has no
    /// multipart support of its own, so the test is the client here.
    fn multipart_body(
        text_fields: &[(&str, &str)],
        files: &[(&str, &str, &[u8])],
    ) -> (Vec<u8>, String) {
        let boundary = format!("----moss-test-boundary-{}", uuid::Uuid::new_v4().simple());
        let mut body = Vec::new();
        for (name, value) in text_fields {
            body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
            body.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
            );
            body.extend_from_slice(value.as_bytes());
            body.extend_from_slice(b"\r\n");
        }
        for (field, filename, bytes) in files {
            body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
            body.extend_from_slice(
                format!(
                    "Content-Disposition: form-data; name=\"{field}\"; filename=\"{filename}\"\r\n"
                )
                .as_bytes(),
            );
            body.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
            body.extend_from_slice(bytes);
            body.extend_from_slice(b"\r\n");
        }
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        (body, format!("multipart/form-data; boundary={boundary}"))
    }

    /// One part lands in the attachment directory and the response names it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn one_part_upload_lands_and_is_named_in_the_response() {
        let (_vault, site_dir) = served_vault();
        let vault_root = site_dir.ancestors().nth(3).unwrap().to_path_buf();
        std::fs::create_dir_all(vault_root.join("posts")).unwrap();
        let ctx = InvokeCtx::standalone();
        let (port, shutdown_tx) = start_server(ServeConfig {
            invoke: Some(ctx.clone()),
            kind: HostKind::Cli,
            ..ServeConfig::new(Arc::new(RwLock::new(site_dir)), 64200)
        })
        .await
        .expect("server should start");
        let token = ctx.token().expect("start_server binds the served vault").to_string();

        let (body, content_type) = multipart_body(
            &[("dir", "posts")],
            &[("file", "photo.jpg", b"jpegbytes")],
        );
        let url = format!("http://localhost:{port}/__moss/upload");
        let resp = ureq::post(&url)
            .set(TOKEN_HEADER, &token)
            .set("Content-Type", &content_type)
            .timeout(std::time::Duration::from_secs(5))
            .send_bytes(&body)
            .expect("an admitted single-part upload must succeed");
        assert_eq!(resp.status(), 200);
        let report: serde_json::Value = resp.into_json().unwrap();
        assert_eq!(report["copied"][0]["original_name"], "photo.jpg");
        assert_eq!(report["copied"][0]["final_name"], "photo.jpg");
        assert!(vault_root.join("posts/photo.jpg").exists());

        let _ = shutdown_tx.send(());
    }

    /// Two file parts in one request both land.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn two_parts_in_one_request_both_land() {
        let (_vault, site_dir) = served_vault();
        let vault_root = site_dir.ancestors().nth(3).unwrap().to_path_buf();
        let ctx = InvokeCtx::standalone();
        let (port, shutdown_tx) = start_server(ServeConfig {
            invoke: Some(ctx.clone()),
            kind: HostKind::Cli,
            ..ServeConfig::new(Arc::new(RwLock::new(site_dir)), 64201)
        })
        .await
        .expect("server should start");
        let token = ctx.token().expect("start_server binds the served vault").to_string();

        let (body, content_type) = multipart_body(
            &[],
            &[
                ("file", "one.jpg", b"one"),
                ("file", "two.jpg", b"two"),
            ],
        );
        let url = format!("http://localhost:{port}/__moss/upload");
        let resp = ureq::post(&url)
            .set(TOKEN_HEADER, &token)
            .set("Content-Type", &content_type)
            .timeout(std::time::Duration::from_secs(5))
            .send_bytes(&body)
            .expect("two admitted parts must both succeed");
        assert_eq!(resp.status(), 200);
        let report: serde_json::Value = resp.into_json().unwrap();
        assert_eq!(report["copied"].as_array().unwrap().len(), 2);
        assert!(vault_root.join("one.jpg").exists());
        assert!(vault_root.join("two.jpg").exists());

        let _ = shutdown_tx.send(());
    }

    /// `..` in a filename is 400, never landed.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dotdot_in_a_filename_is_400() {
        let (_vault, site_dir) = served_vault();
        let vault_root = site_dir.ancestors().nth(3).unwrap().to_path_buf();
        let ctx = InvokeCtx::standalone();
        let (port, shutdown_tx) = start_server(ServeConfig {
            invoke: Some(ctx.clone()),
            kind: HostKind::Cli,
            ..ServeConfig::new(Arc::new(RwLock::new(site_dir)), 64202)
        })
        .await
        .expect("server should start");
        let token = ctx.token().expect("start_server binds the served vault").to_string();

        let (body, content_type) =
            multipart_body(&[], &[("file", "../escape.jpg", b"x")]);
        let url = format!("http://localhost:{port}/__moss/upload");
        match ureq::post(&url)
            .set(TOKEN_HEADER, &token)
            .set("Content-Type", &content_type)
            .timeout(std::time::Duration::from_secs(5))
            .send_bytes(&body)
        {
            Err(ureq::Error::Status(400, _)) => {}
            Ok(resp) => panic!("a '..' filename must 400; got {}", resp.status()),
            Err(e) => panic!("expected a 400, got transport error: {e}"),
        }
        assert!(!vault_root.join("escape.jpg").exists());

        let _ = shutdown_tx.send(());
    }

    /// A `dir` that resolves outside the vault is 400.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dir_outside_the_vault_is_400() {
        let (_vault, site_dir) = served_vault();
        let ctx = InvokeCtx::standalone();
        let (port, shutdown_tx) = start_server(ServeConfig {
            invoke: Some(ctx.clone()),
            kind: HostKind::Cli,
            ..ServeConfig::new(Arc::new(RwLock::new(site_dir)), 64203)
        })
        .await
        .expect("server should start");
        let token = ctx.token().expect("start_server binds the served vault").to_string();

        let (body, content_type) = multipart_body(
            &[("dir", "../../escape")],
            &[("file", "photo.jpg", b"x")],
        );
        let url = format!("http://localhost:{port}/__moss/upload");
        match ureq::post(&url)
            .set(TOKEN_HEADER, &token)
            .set("Content-Type", &content_type)
            .timeout(std::time::Duration::from_secs(5))
            .send_bytes(&body)
        {
            Err(ureq::Error::Status(400, _)) => {}
            Ok(resp) => panic!("a dir outside the vault must 400; got {}", resp.status()),
            Err(e) => panic!("expected a 400, got transport error: {e}"),
        }

        let _ = shutdown_tx.send(());
    }

    /// No token at all is 401, same as the other mutating routes.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn no_token_is_401() {
        let (_vault, site_dir) = served_vault();
        let (port, shutdown_tx) = start_server(ServeConfig {
            invoke: Some(InvokeCtx::standalone()),
            kind: HostKind::Cli,
            ..ServeConfig::new(Arc::new(RwLock::new(site_dir)), 64204)
        })
        .await
        .expect("server should start");

        let (body, content_type) = multipart_body(&[], &[("file", "photo.jpg", b"x")]);
        let url = format!("http://localhost:{port}/__moss/upload");
        match ureq::post(&url)
            .set("Content-Type", &content_type)
            .timeout(std::time::Duration::from_secs(5))
            .send_bytes(&body)
        {
            Err(ureq::Error::Status(401, _)) => {}
            Ok(resp) => panic!("an upload with no token must 401; got {}", resp.status()),
            Err(e) => panic!("expected a 401, got transport error: {e}"),
        }

        let _ = shutdown_tx.send(());
    }

    /// A part over the limit is 413. `MAX_UPLOAD_PART_BYTES` is a few KiB
    /// under `#[cfg(test)]` so this does not stream a real 64 MiB part.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn oversized_part_is_413() {
        let (_vault, site_dir) = served_vault();
        let ctx = InvokeCtx::standalone();
        let (port, shutdown_tx) = start_server(ServeConfig {
            invoke: Some(ctx.clone()),
            kind: HostKind::Cli,
            ..ServeConfig::new(Arc::new(RwLock::new(site_dir)), 64205)
        })
        .await
        .expect("server should start");
        let token = ctx.token().expect("start_server binds the served vault").to_string();

        let oversized = vec![b'x'; super::MAX_UPLOAD_PART_BYTES + 1];
        let (body, content_type) =
            multipart_body(&[], &[("file", "big.jpg", &oversized)]);
        let url = format!("http://localhost:{port}/__moss/upload");
        match ureq::post(&url)
            .set(TOKEN_HEADER, &token)
            .set("Content-Type", &content_type)
            .timeout(std::time::Duration::from_secs(5))
            .send_bytes(&body)
        {
            Err(ureq::Error::Status(413, _)) => {}
            Ok(resp) => panic!("an oversized part must 413; got {}", resp.status()),
            Err(e) => panic!("expected a 413, got transport error: {e}"),
        }

        let _ = shutdown_tx.send(());
    }

    /// An absolute `dir` is 400, same as one that walks out via `..` —
    /// `dir` is project-relative only, never trusted as a root of its own.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn absolute_dir_is_400() {
        let (_vault, site_dir) = served_vault();
        let ctx = InvokeCtx::standalone();
        let (port, shutdown_tx) = start_server(ServeConfig {
            invoke: Some(ctx.clone()),
            kind: HostKind::Cli,
            ..ServeConfig::new(Arc::new(RwLock::new(site_dir)), 64206)
        })
        .await
        .expect("server should start");
        let token = ctx.token().expect("start_server binds the served vault").to_string();

        let (body, content_type) =
            multipart_body(&[("dir", "/etc")], &[("file", "photo.jpg", b"x")]);
        let url = format!("http://localhost:{port}/__moss/upload");
        match ureq::post(&url)
            .set(TOKEN_HEADER, &token)
            .set("Content-Type", &content_type)
            .timeout(std::time::Duration::from_secs(5))
            .send_bytes(&body)
        {
            Err(ureq::Error::Status(400, _)) => {}
            Ok(resp) => panic!("an absolute dir must 400; got {}", resp.status()),
            Err(e) => panic!("expected a 400, got transport error: {e}"),
        }

        let _ = shutdown_tx.send(());
    }

    /// More than `MAX_UPLOAD_PARTS` file parts in one request is 400, not a
    /// slow 413: the count cap is a separate limit from the per-part size cap.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn too_many_parts_is_400() {
        let (_vault, site_dir) = served_vault();
        let ctx = InvokeCtx::standalone();
        let (port, shutdown_tx) = start_server(ServeConfig {
            invoke: Some(ctx.clone()),
            kind: HostKind::Cli,
            ..ServeConfig::new(Arc::new(RwLock::new(site_dir)), 64207)
        })
        .await
        .expect("server should start");
        let token = ctx.token().expect("start_server binds the served vault").to_string();

        let files: Vec<(&str, String, &[u8])> = (0..super::MAX_UPLOAD_PARTS + 1)
            .map(|i| ("file", format!("f{i}.jpg"), b"x".as_slice()))
            .collect();
        let files_ref: Vec<(&str, &str, &[u8])> =
            files.iter().map(|(f, n, b)| (*f, n.as_str(), *b)).collect();
        let (body, content_type) = multipart_body(&[], &files_ref);
        let url = format!("http://localhost:{port}/__moss/upload");
        match ureq::post(&url)
            .set(TOKEN_HEADER, &token)
            .set("Content-Type", &content_type)
            .timeout(std::time::Duration::from_secs(5))
            .send_bytes(&body)
        {
            Err(ureq::Error::Status(400, _)) => {}
            Ok(resp) => panic!("one too many parts must 400; got {}", resp.status()),
            Err(e) => panic!("expected a 400, got transport error: {e}"),
        }

        let _ = shutdown_tx.send(());
    }

    /// A part over axum's 2 MiB whole-request default — but under this
    /// route's own cap — is accepted. Proves `TOTAL_MAX_UPLOAD_BYTES`
    /// actually replaces the default for `/__moss/upload`, the regression
    /// the trailing-`route_layer` bug would otherwise have masked.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn part_over_2mib_default_is_accepted() {
        let (_vault, site_dir) = served_vault();
        let ctx = InvokeCtx::standalone();
        let (port, shutdown_tx) = start_server(ServeConfig {
            invoke: Some(ctx.clone()),
            kind: HostKind::Cli,
            ..ServeConfig::new(Arc::new(RwLock::new(site_dir)), 64208)
        })
        .await
        .expect("server should start");
        let token = ctx.token().expect("start_server binds the served vault").to_string();

        let over_2mib = vec![b'x'; 2 * 1024 * 1024 + 1024];
        assert!(over_2mib.len() < super::MAX_UPLOAD_PART_BYTES, "fixture must stay under this route's own cap");
        let (body, content_type) = multipart_body(&[], &[("file", "big.jpg", &over_2mib)]);
        let url = format!("http://localhost:{port}/__moss/upload");
        let resp = ureq::post(&url)
            .set(TOKEN_HEADER, &token)
            .set("Content-Type", &content_type)
            .timeout(std::time::Duration::from_secs(5))
            .send_bytes(&body)
            .expect("a part over 2 MiB but under this route's cap must succeed");
        assert_eq!(resp.status(), 200);

        let _ = shutdown_tx.send(());
    }

    /// `/__moss/comments` still enforces axum's 2 MiB default — the sibling
    /// route the trailing-`route_layer` bug would have silently exempted
    /// along with upload, since `route_layer` applies to every route added
    /// so far in the chain, not just the last one.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn comments_route_still_enforces_the_2mib_default() {
        let (_vault, site_dir) = served_vault();
        let (port, shutdown_tx) = start_server(ServeConfig {
            invoke: Some(InvokeCtx::standalone()),
            kind: HostKind::Cli,
            ..ServeConfig::new(Arc::new(RwLock::new(site_dir)), 64209)
        })
        .await
        .expect("server should start");

        let over_2mib = vec![b'"'; 2 * 1024 * 1024 + 1024];
        let url = format!("http://localhost:{port}/__moss/comments/api/v2/comments");
        match ureq::post(&url)
            .set("Content-Type", "application/json")
            .timeout(std::time::Duration::from_secs(5))
            .send_bytes(&over_2mib)
        {
            Err(ureq::Error::Status(413, _)) => {}
            Ok(resp) => panic!("a body over 2 MiB to /__moss/comments must 413; got {}", resp.status()),
            Err(e) => panic!("expected a 413, got transport error: {e}"),
        }

        let _ = shutdown_tx.send(());
    }
}
