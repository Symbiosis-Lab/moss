//! Pictures a page shows from other sites, probed when a publish runs.
//!
//! The build lists them with no network (`PublishPreflightProjection::remote_pictures`);
//! only a publish asks the hosts. A picture is marked gone on a 404 or 410 and
//! nothing else: a timeout, a 5xx, a refusal or a failed lookup says the host
//! or the network is unwell, not that the picture is gone. A mark never stops
//! a publish, since a gone picture is already broken on the live site.

use crate::build::types::ReferenceOccurrence;
use futures::stream::{self, StreamExt};
use std::time::Duration;

/// A picture whose host answered that it is gone.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct GonePicture {
    #[serde(flatten)]
    pub occurrence: ReferenceOccurrence,
    pub status: u16,
}

/// What the probe found. `offline`: no host answered at all, so nothing was
/// checked; `unchecked`: pictures the run's time limit left unasked.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct RemotePictureReport {
    pub gone: Vec<GonePicture>,
    pub unchecked: usize,
    pub offline: bool,
}

impl RemotePictureReport {
    /// One line per gone picture, then a line for what went unchecked; empty
    /// when there is nothing to say.
    pub fn lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .gone
            .iter()
            .map(|g| format!("{}:{}  {}  no longer loads ({})", g.occurrence.source_path, g.occurrence.source_span.line, g.occurrence.reference, g.status))
            .collect();
        if self.offline {
            lines.push("Pictures from other sites were not checked: no host answered.".into());
        } else if self.unchecked > 0 {
            lines.push(format!("{} pictures from other sites were not checked in time.", self.unchecked));
        }
        lines
    }
}

/// How hard a run may try.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Budget {
    pub at_once: usize,
    pub per_url: Duration,
    pub whole_run: Duration,
}

const BUDGET: Budget = Budget { at_once: 8, per_url: Duration::from_secs(5), whole_run: Duration::from_secs(15) };

// What a browser sends for an <img>; a hotlink-protected host refuses a bare probe.
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15";
const ACCEPT: &str = "image/avif,image/webp,image/apng,image/*,*/*;q=0.8";

/// Probe the remote pictures `folder`'s last build listed. `referer` gives the
/// published address of a page's source file, when the site has one.
pub async fn check(folder: &str, referer: &(dyn Fn(&str) -> Option<String> + Sync)) -> RemotePictureReport {
    let candidates = crate::system::build_records::records()
        .publish_preflight(folder)
        .map(|p| p.remote_pictures)
        .unwrap_or_default();
    probe_all(candidates, referer, BUDGET).await
}

/// [`check`] after a publish to `site` landed: each page's `Referer` is its
/// address in the publish record just written.
pub async fn check_published(folder: &std::path::Path, site: &str) -> RemotePictureReport {
    let mp = crate::moss_paths::MossPaths::new(folder);
    let outputs = crate::build::manifest::published_record::load_for(&mp, crate::build::manifest::backfill::publish_target(&mp).as_deref())
        .map(|record| record.source_to_output)
        .unwrap_or_default();
    let referer = |source: &str| Some(page_address(site, outputs.get(source).map(String::as_str)));
    check(&folder.to_string_lossy(), &referer).await
}

/// The published address of a page whose build output is `output`, for the
/// `Referer` a visitor's browser would send; the site root when the page has
/// no output of its own.
pub fn page_address(site: &str, output: Option<&str>) -> String {
    let page = output.map(crate::build::served_path::served_address);
    format!("{}{}", site.trim_end_matches('/'), page.as_deref().unwrap_or("/"))
}

pub(crate) async fn probe_all(
    candidates: Vec<ReferenceOccurrence>,
    referer: &(dyn Fn(&str) -> Option<String> + Sync),
    budget: Budget,
) -> RemotePictureReport {
    use std::collections::HashMap;
    if candidates.is_empty() {
        return RemotePictureReport::default();
    }
    let client = match crate::system::proxy_reqwest::build_client(|b| b.timeout(budget.per_url).user_agent(USER_AGENT)) {
        Ok(client) => client,
        Err(_) => return RemotePictureReport { offline: true, ..Default::default() },
    };
    // One probe per URL, sent with the first page that shows it.
    let mut seen = std::collections::HashSet::new();
    let mut urls: Vec<(String, Option<String>)> = candidates
        .iter()
        .filter(|c| seen.insert(c.reference.as_str()))
        .map(|c| (c.reference.clone(), referer(&c.source_path)))
        .collect();
    let total = urls.len();
    // A site over budget starts somewhere else each run, so with no cache the
    // same first pictures are not the only ones ever checked.
    let offset = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as usize);
    urls.rotate_left(offset % total);
    let mut answers: HashMap<String, Option<u16>> = HashMap::with_capacity(total);
    let mut probes = stream::iter(urls)
        .map(|(url, referer)| {
            let client = client.clone();
            async move {
                let answer = probe(&client, &url, referer.as_deref()).await;
                (url, answer)
            }
        })
        .buffer_unordered(budget.at_once);
    let deadline = tokio::time::sleep(budget.whole_run);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            next = probes.next() => match next {
                Some((url, answer)) => { answers.insert(url, answer); }
                None => break,
            },
            _ = &mut deadline => break,
        }
    }
    if answers.values().all(Option::is_none) {
        return RemotePictureReport { offline: true, ..Default::default() };
    }
    RemotePictureReport {
        unchecked: total - answers.len(),
        gone: candidates
            .into_iter()
            .filter_map(|occurrence| match answers.get(&occurrence.reference) {
                Some(Some(status @ (404 | 410))) => Some(GonePicture { status: *status, occurrence }),
                _ => None,
            })
            .collect(),
        offline: false,
    }
}

/// The status a picture's host answers, `None` when it does not answer. Only a
/// GET answer can say a picture is gone: some hosts and signed-URL CDNs refuse
/// or 404 a HEAD that a GET serves.
async fn probe(client: &reqwest::Client, url: &str, referer: Option<&str>) -> Option<u16> {
    let send = |method: reqwest::Method| {
        let mut request = client.request(method.clone(), url).header(reqwest::header::ACCEPT, ACCEPT);
        if method == reqwest::Method::GET {
            request = request.header(reqwest::header::RANGE, "bytes=0-0");
        }
        if let Some(referer) = referer {
            request = request.header(reqwest::header::REFERER, referer);
        }
        request.send()
    };
    let head = send(reqwest::Method::HEAD).await.ok()?.status().as_u16();
    if !matches!(head, 403 | 404 | 405 | 410 | 501) {
        return Some(head);
    }
    send(reqwest::Method::GET).await.ok().map(|r| r.status().as_u16())
}

#[cfg(test)]
#[path = "remote_pictures_tests.rs"]
mod tests;
