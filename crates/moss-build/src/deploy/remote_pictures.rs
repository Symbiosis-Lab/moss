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
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct GonePicture {
    #[serde(flatten)]
    pub occurrence: ReferenceOccurrence,
    pub status: u16,
}

/// What the probe found. `offline`: no host answered at all, so nothing was
/// checked; `unchecked`: pictures the run's time limit left unasked.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
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

/// The published address of `source`'s page in a build, for the `Referer` a
/// visitor's browser would send; `None` before the site has an address.
pub fn page_address(sealed: &crate::build::manifest::SealedManifest, source: &str) -> Option<String> {
    let site = sealed.site_url()?.trim_end_matches('/');
    let page = sealed.source_to_output().get(source).map(|out| crate::build::served_path::served_address(out));
    Some(format!("{site}{}", page.as_deref().unwrap_or("/")))
}

enum Answer {
    Status(u16),
    NoAnswer,
}

pub(crate) async fn probe_all(
    candidates: Vec<ReferenceOccurrence>,
    referer: &(dyn Fn(&str) -> Option<String> + Sync),
    budget: Budget,
) -> RemotePictureReport {
    if candidates.is_empty() {
        return RemotePictureReport::default();
    }
    let client = match crate::system::proxy_reqwest::build_client(|b| b.timeout(budget.per_url).user_agent(USER_AGENT)) {
        Ok(client) => client,
        Err(_) => return RemotePictureReport { offline: true, ..Default::default() },
    };
    // One probe per URL, sent with the first page that shows it.
    let mut urls: Vec<(String, Option<String>)> = Vec::new();
    for c in &candidates {
        if !urls.iter().any(|(u, _)| *u == c.reference) {
            urls.push((c.reference.clone(), referer(&c.source_path)));
        }
    }
    let total = urls.len();
    let mut answers: Vec<(String, Answer)> = Vec::with_capacity(total);
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
                Some(answer) => answers.push(answer),
                None => break,
            },
            _ = &mut deadline => break,
        }
    }
    if !answers.iter().any(|(_, a)| matches!(a, Answer::Status(_))) {
        return RemotePictureReport { offline: true, ..Default::default() };
    }
    let gone_status = |url: &str| {
        answers.iter().find_map(|(u, a)| match a {
            Answer::Status(s @ (404 | 410)) if u == url => Some(*s),
            _ => None,
        })
    };
    RemotePictureReport {
        gone: candidates
            .into_iter()
            .filter_map(|occurrence| gone_status(&occurrence.reference).map(|status| GonePicture { occurrence, status }))
            .collect(),
        unchecked: total - answers.len(),
        offline: false,
    }
}

async fn probe(client: &reqwest::Client, url: &str, referer: Option<&str>) -> Answer {
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
    let status = match send(reqwest::Method::HEAD).await {
        // Many hosts refuse HEAD and answer GET.
        Ok(r) if matches!(r.status().as_u16(), 403 | 405 | 501) => send(reqwest::Method::GET).await.map(|r| r.status().as_u16()),
        Ok(r) => Ok(r.status().as_u16()),
        Err(e) => Err(e),
    };
    status.map_or(Answer::NoAnswer, Answer::Status)
}

#[cfg(test)]
#[path = "remote_pictures_tests.rs"]
mod tests;
