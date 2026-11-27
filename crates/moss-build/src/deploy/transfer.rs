//! A publish's files, through seta's deploy resource: open the deploy the
//! manifest names, send what the server lacks, and commit if no send did.
//!
//! The request count follows the server's limits, not the file count: files
//! up to `request_bytes` are packed into sends of at most that many bytes,
//! `concurrency_hint` at a time; a larger file goes by the chunked routes and
//! a link by `PUT /files`, each carrying the deploy id. Sends are idempotent
//! on the server (a part already staged is ignored), so a send cut off is
//! sent again whole, and a deploy the server lost is opened again.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::StreamExt as _;

use crate::deploy::{profile, progress, upload, PublishError};
use crate::seta::client::{MossSetaClient, SetaError};
use crate::seta::deploys::{DeployAnswer, DeployState, Limits, Need};
use crate::seta::upload_policy::Throughput;
use crate::types::content::{parse_entry, MODE_FILE, MODE_SYMLINK};

/// Opens after the first that one publish makes before it gives up: a lost
/// deploy, a file a commit found gone, or a corrected manifest each cost one.
const MAX_REOPENS: usize = 4;

/// What an answer without `limits` is held to: seta's own values.
const DEFAULT_LIMITS: Limits = Limits { request_bytes: 4 << 20, concurrency_hint: 6 };

pub(crate) struct Transfer<'a> {
    pub client: &'a MossSetaClient,
    pub site_id: &'a str,
    /// The generation directory the files are read from.
    pub dir: &'a Path,
    pub sink: &'a Arc<dyn progress::DeploySink>,
    /// Publish even when the server finds it would remove most of the site
    /// ([`super::mass_remove::removal_accepted`]).
    pub allow_removal: bool,
}

/// One request's worth of the files a deploy needs.
enum Piece {
    Send(Vec<Need>),
    Chunked(Need),
    Link(Need),
}

impl Transfer<'_> {
    /// The server's answer to `files`: live, or open with what it needs.
    pub(crate) async fn open(&self, files: &HashMap<String, String>) -> Result<DeployAnswer, SetaError> {
        let sizes = upload::regular_file_sizes(self.dir, files).await.map_err(SetaError::Io)?;
        // Something other than a file where the manifest names one would go
        // out as 0 bytes. A missing file is the server's to ask for.
        for path in files.keys().filter(|p| parse_entry(&files[*p]).0 == MODE_FILE && !sizes.contains_key(*p)) {
            if tokio::fs::try_exists(self.dir.join(path)).await.unwrap_or(false) {
                return Err(SetaError::Io(format!("Failed to stat {path}")));
            }
        }
        profile::count("manifest_kb", sizes.values().sum::<u64>().div_ceil(1024));
        profile::open(self.client.open_deploy(self.site_id, files, &sizes, self.allow_removal)).await
    }

    /// Sends what `answer` needs until the deploy is live. A file that changed
    /// on disk since the seal and settled is corrected in `files`, which names
    /// another deploy, so that one is opened instead.
    pub(crate) async fn finish(&self, files: &mut HashMap<String, String>, mut answer: DeployAnswer) -> Result<DeployAnswer, PublishError> {
        let base = tokio::fs::canonicalize(self.dir).await.map_err(|e| format!("Failed to resolve site dir: {e}"))?;
        let throughput = Throughput::new(answer.limits.unwrap_or(DEFAULT_LIMITS).concurrency_hint);
        let heal_cap = upload::self_heal_cap(answer.needed.len());
        let total: u64 = answer.needed.iter().map(|n| n.size).sum();
        let state = Arc::new(progress::UploadProgressState::new(total, answer.needed.len() as u32, 0));
        let ticker = Ticker::start(&state, self.sink);
        let (mut uploaded, mut upload_started) = (0, None);
        for _ in 0..=MAX_REOPENS {
            if answer.state == DeployState::Live {
                drop(ticker);
                self.sink.upload_sample(&state);
                if let Some(started) = upload_started {
                    record_upload(started, uploaded, &throughput);
                }
                profile::landed_counts(&answer);
                return Ok(answer);
            }
            let healed = self.settle(files, &answer.needed, &base, &throughput, heal_cap).await?;
            if !healed.is_empty() {
                log::info!("[deploy] correcting {} manifest entries to the bytes on disk", healed.len());
                files.extend(healed);
                answer = self.open(files).await.map_err(profile::profiled_refusal)?;
                continue;
            }
            upload_started.get_or_insert_with(std::time::Instant::now);
            let sending = answer.needed.len() as u64;
            let sent = self.send_all(&answer, &base, &state, &throughput).await;
            if sent.is_ok() {
                uploaded += sending;
            }
            answer = match sent {
                Ok(Some(live)) => live,
                Ok(None) => {
                    self.sink.stage(progress::DeployStage::Committing, 0, 0, "Making changes live...");
                    profile::commit(self.client.commit_deploy(self.site_id, &answer.deploy)).await?
                }
                Err(SetaError::Api { status: 404, .. }) => self.open(files).await.map_err(profile::profiled_refusal)?,
                Err(e) => return Err(profile::profiled_refusal(e)),
            };
        }
        Err(format!("The server still needed {} files after {MAX_REOPENS} attempts; publish again.", answer.needed.len()).into())
    }

    /// The corrected entry of each needed file whose bytes no longer match
    /// its entry and have stopped changing, and of each link re-pointed since
    /// the seal.
    async fn settle(
        &self,
        files: &HashMap<String, String>,
        needed: &[Need],
        base: &Path,
        throughput: &Throughput,
        heal_cap: usize,
    ) -> Result<HashMap<String, String>, String> {
        let at_once = std::thread::available_parallelism().map_or(4, |n| n.get());
        let settled: Vec<Result<Option<(&Need, String)>, String>> = futures::stream::iter(needed.iter().filter(|n| parse_entry(&n.entry).0 == MODE_FILE))
            .map(|need| async move {
                let at = contained(base, &need.path).await?;
                Ok(upload::settled_hash(&need.path, &at, parse_entry(&need.entry).1, throughput, heal_cap).await?.map(|hash| (need, hash)))
            })
            .buffer_unordered(at_once)
            .collect()
            .await;
        let mut healed = HashMap::new();
        for (need, hash) in settled.into_iter().collect::<Result<Vec<_>, _>>()?.into_iter().flatten() {
            for (path, _) in files.iter().filter(|(_, e)| **e == need.entry) {
                healed.insert(path.clone(), crate::types::content::file_entry(&hash));
            }
        }
        for need in needed.iter().filter(|n| parse_entry(&n.entry).0 == MODE_SYMLINK) {
            for (path, _) in files.iter().filter(|(_, e)| **e == need.entry) {
                let target = tokio::fs::read_link(self.dir.join(path)).await.map_err(|e| format!("Failed to read symlink {path}: {e}"))?;
                let entry = crate::types::content::symlink_entry(&target.to_string_lossy());
                if entry != need.entry {
                    upload::charge_self_heal(throughput, heal_cap, path)?;
                    log::warn!("[deploy] symlink '{path}' was re-pointed since the build; publishing its current target");
                    healed.insert(path.clone(), entry);
                }
            }
        }
        Ok(healed)
    }

    /// Sends every needed piece; the answer of a send that committed, or None
    /// when none did and the deploy must be committed. A live answer wins over
    /// a sibling's error: a send whose response was lost may still have
    /// staged the parts that made the deploy live.
    async fn send_all(
        &self,
        answer: &DeployAnswer,
        base: &Path,
        state: &Arc<progress::UploadProgressState>,
        throughput: &Throughput,
    ) -> Result<Option<DeployAnswer>, SetaError> {
        let limits = answer.limits.unwrap_or(DEFAULT_LIMITS);
        let deploy = answer.deploy.as_str();
        let results: Vec<Result<Option<DeployAnswer>, SetaError>> = futures::stream::iter(pieces(&answer.needed, limits.request_bytes))
            .map(|piece| self.send_piece(piece, deploy, base, state, throughput))
            .buffer_unordered(limits.concurrency_hint.max(1))
            .collect()
            .await;
        let mut first_error = None;
        for result in results {
            match result {
                Ok(Some(a)) if a.state == DeployState::Live => return Ok(Some(a)),
                Ok(_) => {}
                Err(e) => { first_error.get_or_insert(e); }
            }
        }
        first_error.map_or(Ok(None), Err)
    }

    async fn send_piece(
        &self,
        piece: Piece,
        deploy: &str,
        base: &Path,
        state: &Arc<progress::UploadProgressState>,
        throughput: &Throughput,
    ) -> Result<Option<DeployAnswer>, SetaError> {
        let io = |e: String| SetaError::Io(e);
        match piece {
            Piece::Send(needs) => {
                let mut parts = Vec::with_capacity(needs.len());
                for need in &needs {
                    state.begin_file(&need.path, need.size);
                    // allow:raw_read built output — dataless is absent
                    let bytes = tokio::fs::read(contained(base, &need.path).await.map_err(io)?)
                        .await
                        .map_err(|e| SetaError::Io(format!("Failed to read {}: {e}", need.path)))?;
                    upload::verify_bytes(&need.path, &bytes, parse_entry(&need.entry).1).map_err(io)?;
                    parts.push((need.entry.clone(), bytes));
                }
                let bytes = needs.iter().map(|n| n.size).sum();
                let sent_at = std::time::Instant::now();
                throughput.note_request();
                let answer = self.client.send_to_deploy(self.site_id, deploy, &parts).await?;
                throughput.note_request_ok();
                throughput.observe(bytes, sent_at.elapsed());
                for need in &needs {
                    state.credit_bytes(need.size);
                    state.finish_file(&need.path);
                }
                crate::infra::liveness::bump();
                Ok(Some(answer))
            }
            Piece::Chunked(need) => {
                state.begin_file(&need.path, need.size);
                let at = contained(base, &need.path).await.map_err(io)?;
                let st = Arc::clone(state);
                self.client
                    .upload_file_chunked(self.site_id, &need.path, &at, need.size, deploy, throughput, Some(&move |n: u64| st.credit_bytes(n)))
                    .await?;
                state.finish_file(&need.path);
                Ok(None)
            }
            Piece::Link(need) => {
                state.begin_file(&need.path, 0);
                let target = tokio::fs::read_link(self.dir.join(&need.path))
                    .await
                    .map_err(|e| SetaError::Io(format!("Failed to read symlink {}: {e}", need.path)))?;
                let target = target.to_string_lossy().into_owned();
                if crate::types::content::symlink_entry(&target) != need.entry {
                    return Err(SetaError::Io(format!("Deploy integrity error: symlink '{}' changed since the build; publish again", need.path)));
                }
                throughput.note_request();
                self.client.upload_symlink(self.site_id, &need.path, target, deploy).await?;
                throughput.note_request_ok();
                state.finish_file(&need.path);
                crate::infra::liveness::bump();
                Ok(None)
            }
        }
    }
}

/// The upload's span and counts in the deploy profile, and its summary line.
fn record_upload(started: std::time::Instant, files: u64, throughput: &Throughput) {
    drop(crate::build::phase::PhaseTrace::since("upload", started));
    profile::count("files_uploaded", files);
    for (name, value) in throughput.profile_counts(started.elapsed()) {
        profile::count(name, value);
    }
    log::info!("{}", throughput.deploy_summary(started.elapsed()));
}

/// `needed` as requests: files up to `request_bytes` packed in order into
/// sends of at most that many bytes, larger files chunked, links each alone.
fn pieces(needed: &[Need], request_bytes: u64) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut batch: Vec<Need> = Vec::new();
    let mut bytes = 0;
    for need in needed {
        match parse_entry(&need.entry).0 {
            MODE_SYMLINK => out.push(Piece::Link(need.clone())),
            _ if need.size > request_bytes => out.push(Piece::Chunked(need.clone())),
            _ => {
                if !batch.is_empty() && bytes + need.size > request_bytes {
                    out.push(Piece::Send(std::mem::take(&mut batch)));
                    bytes = 0;
                }
                bytes += need.size;
                batch.push(need.clone());
            }
        }
    }
    if !batch.is_empty() {
        out.push(Piece::Send(batch));
    }
    out
}

/// `path` under `base`, refusing one a link inside the build leads out of.
async fn contained(base: &Path, path: &str) -> Result<PathBuf, String> {
    let at = tokio::fs::canonicalize(base.join(path)).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            format!("Manifest claims '{path}' exists but it's missing on disk. Your build record (hashes.json) may be stale. Try rebuilding the site. Underlying error: {e}")
        } else {
            format!("Failed to resolve {path}: {e}")
        }
    })?;
    if !at.starts_with(base) {
        return Err(format!("Path traversal detected: {path}"));
    }
    Ok(at)
}

/// Emits the upload's progress at 4 Hz until dropped. Sampling is not a
/// liveness bump: it fires whether or not bytes moved.
struct Ticker(tokio::task::JoinHandle<()>);

impl Ticker {
    fn start(state: &Arc<progress::UploadProgressState>, sink: &Arc<dyn progress::DeploySink>) -> Self {
        let (state, sink) = (Arc::clone(state), Arc::clone(sink));
        Self(tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_millis(250));
            loop {
                tick.tick().await;
                sink.upload_sample(&state);
            }
        }))
    }
}

impl Drop for Ticker {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn need(path: &str, size: u64) -> Need {
        Need { entry: format!("100644:{:016x}", size), path: path.into(), size }
    }

    #[test]
    fn needed_files_pack_into_sends_up_to_request_bytes_and_a_larger_one_goes_chunked() {
        let needed = [need("a", 3), need("b", 3), need("c", 5), need("d", 11), need("e", 10)];
        let shape: Vec<String> = pieces(&needed, 10)
            .iter()
            .map(|p| match p {
                Piece::Send(n) => n.iter().map(|n| n.path.as_str()).collect::<Vec<_>>().join("+"),
                Piece::Chunked(n) => format!("chunked {}", n.path),
                Piece::Link(n) => format!("link {}", n.path),
            })
            .collect();
        assert_eq!(shape, ["a+b", "chunked d", "c", "e"]);
    }
}
