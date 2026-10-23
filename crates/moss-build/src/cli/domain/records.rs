//! The DNS records a site owner types at their registrar, and how a live
//! lookup measures them. Pure: DNS arrives through [`Lookup`], so nothing here
//! touches the network.

use crate::config::deployment::DnsRecord;
use crate::deploy::MOSSPUB_VPS_IP;
use crate::seta::cdn::CdnStatus;
use serde::Serialize;
use std::net::ToSocketAddrs;

/// Reduce what a person types (`www.Example.com.`, `https://example.com/`) to
/// the registered domain the hosting service keys its rows by. Only a name of
/// exactly two labels qualifies: telling `example.co.uk` from `blog.example.com`
/// takes a public-suffix list, which this does not carry.
pub fn normalize_domain(input: &str) -> Result<String, String> {
    let mut d = input.trim().to_ascii_lowercase();
    for scheme in ["https://", "http://"] {
        if let Some(rest) = d.strip_prefix(scheme) {
            d = rest.trim_end_matches('/').to_string();
        }
    }
    let d = d.trim_end_matches('.');
    let d = d.strip_prefix("www.").unwrap_or(d);
    let label_ok = |l: &str| !l.is_empty() && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    let labels: Vec<&str> = d.split('.').collect();
    if labels.len() != 2 || !labels.iter().all(|l| label_ok(l)) {
        return Err(format!(
            "{}: only a registered domain such as example.com is supported, not a subdomain or any other name",
            input
        ));
    }
    Ok(d.to_string())
}

/// What the owner must have in their zone: the apex address (the apex
/// redirects to www from the origin) followed by the records the service
/// listed for the edge.
pub fn required_records(service: &[DnsRecord]) -> Vec<DnsRecord> {
    let is_apex_a = |r: &DnsRecord| r.record_type == "A" && matches!(r.name.as_str(), "@" | "");
    let mut records = Vec::new();
    if !service.iter().any(is_apex_a) {
        records.push(DnsRecord {
            record_type: "A".into(),
            name: "@".into(),
            value: MOSSPUB_VPS_IP.into(),
            ttl: None,
        });
    }
    records.extend(service.iter().cloned().map(|mut r| {
        if r.name.is_empty() {
            r.name = "@".into();
        }
        r
    }));
    records
}

/// The addresses a name resolves to right now.
pub trait Lookup {
    fn addrs(&self, host: &str) -> Vec<String>;
}

/// The operating system's resolver. It answers address questions only, so
/// TXT records and CNAME chains that end without an address are confirmed
/// through the hosting service instead (see [`check_records`]).
pub struct SystemLookup;

impl Lookup for SystemLookup {
    fn addrs(&self, host: &str) -> Vec<String> {
        let mut found: Vec<String> = (host, 443)
            .to_socket_addrs()
            .map(|it| it.map(|a| a.ip().to_string()).collect())
            .unwrap_or_default();
        found.sort();
        found.dedup();
        found
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RecordCheck {
    #[serde(rename = "type")]
    pub record_type: String,
    pub name: String,
    pub value: String,
    pub done: bool,
    /// What the name resolves to now. Empty when the lookup found nothing or
    /// the record is confirmed by the hosting service instead.
    pub found: Vec<String>,
    /// `"dns"` (looked up here) or `"hosting service"` (it reads the record).
    pub checked_by: &'static str,
}

impl RecordCheck {
    fn live(&self) -> bool {
        self.checked_by == "dns"
    }
}

fn fqdn(name: &str, apex: &str) -> String {
    if name == "@" {
        apex.to_string()
    } else {
        format!("{}.{}", name, apex)
    }
}

/// The two records the resolver can judge: the apex address and the www
/// CNAME. The rest (certificate validation) it cannot see.
fn looked_up_here(r: &DnsRecord) -> bool {
    (r.record_type == "A" && r.name == "@") || (r.record_type == "CNAME" && r.name == "www")
}

/// Measure each record. The apex address and the www CNAME are judged by a
/// live lookup, always: an empty answer is "could not look up", never done.
/// Every other record is done once the service reports the certificate is
/// being issued or live, which it only does after reading those records.
pub fn check_records(
    apex: &str,
    records: &[DnsRecord],
    cdn: CdnStatus,
    lookup: &dyn Lookup,
) -> Vec<RecordCheck> {
    let service_confirmed = matches!(cdn, CdnStatus::Validating | CdnStatus::Active);
    records
        .iter()
        .map(|r| {
            let (done, found) = if looked_up_here(r) {
                let found = lookup.addrs(&fqdn(&r.name, apex));
                let done = if r.record_type == "A" {
                    found.contains(&r.value)
                } else {
                    let target = lookup.addrs(&r.value);
                    found.iter().any(|a| target.contains(a))
                };
                (done, found)
            } else {
                (service_confirmed, Vec::new())
            };
            RecordCheck {
                record_type: r.record_type.clone(),
                name: r.name.clone(),
                value: r.value.clone(),
                done,
                found,
                checked_by: if looked_up_here(r) { "dns" } else { "hosting service" },
            }
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    Live,
    WaitingForDns,
    WaitingForCertificate,
    Failed(String),
}

impl Verdict {
    pub fn word(&self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::WaitingForDns => "waiting_for_dns",
            Self::WaitingForCertificate => "waiting_for_certificate",
            Self::Failed(_) => "failed",
        }
    }

    /// 0 live, 3 still waiting, 4 failed; the contract at the top of `domain.rs`.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Live => 0,
            Self::WaitingForDns | Self::WaitingForCertificate => 3,
            Self::Failed(_) => 4,
        }
    }
}

/// `listed` is how many records the service named, before our own apex row.
pub fn verdict(checks: &[RecordCheck], listed: usize, cdn: CdnStatus, retry: &str) -> Verdict {
    if cdn == CdnStatus::Failed {
        return Verdict::Failed(format!(
            "the hosting service gave up validating the certificate; run `{}` to start over",
            retry
        ));
    }
    if listed == 0 {
        return Verdict::Failed(format!(
            "the hosting service listed no records for this domain; run `{}`",
            retry
        ));
    }
    if checks.iter().any(|c| c.live() && !c.done) {
        return Verdict::WaitingForDns;
    }
    if cdn == CdnStatus::Active {
        Verdict::Live
    } else {
        Verdict::WaitingForCertificate
    }
}

/// A domain whose zone moss manages has no records to check: the verdict
/// follows the service's own state.
pub fn managed_verdict(dns_ok: bool, cdn: CdnStatus, retry: &str) -> Verdict {
    match cdn {
        CdnStatus::Failed => Verdict::Failed(format!(
            "the hosting service could not finish setting this domain up; run `{}`",
            retry
        )),
        CdnStatus::Active => Verdict::Live,
        _ if !dns_ok => Verdict::WaitingForDns,
        _ => Verdict::WaitingForCertificate,
    }
}

pub fn render_records(records: &[DnsRecord]) -> String {
    let w = records.iter().map(|r| r.name.len()).max().unwrap_or(4).max(4);
    let mut out = format!("  {:<6} {:<w$} {}\n", "TYPE", "NAME", "VALUE", w = w);
    for r in records {
        out.push_str(&format!("  {:<6} {:<w$} {}\n", r.record_type, r.name, r.value, w = w));
    }
    out
}

pub fn render_checks(checks: &[RecordCheck]) -> String {
    let mut out = String::new();
    for c in checks {
        let (mark, now) = match (c.live(), c.done) {
            (true, done) => (
                if done { "done   " } else { "not yet" },
                if c.found.is_empty() { "could not look up".to_string() } else { c.found.join(", ") },
            ),
            (false, true) => ("done   ", "the hosting service has read it".to_string()),
            (false, false) => ("waiting", "the hosting service confirms this once it has read it".to_string()),
        };
        out.push_str(&format!(
            "  [{}] {} {} {}\n         now: {}\n",
            mark, c.record_type, c.name, c.value, now
        ));
    }
    out
}

pub fn verdict_line(v: &Verdict, checks: &[RecordCheck]) -> String {
    match v {
        Verdict::Live => "Verdict: live.".into(),
        Verdict::WaitingForDns => {
            let live = checks.iter().filter(|c| c.live()).count();
            let left = checks.iter().filter(|c| c.live() && !c.done).count();
            format!(
                "Verdict: waiting for DNS. {} of {} address records are not in place yet; they can take up to an hour to show.",
                left, live
            )
        }
        Verdict::WaitingForCertificate => {
            "Verdict: waiting for the hosting service. The address records are in place; it still has to read the validation records and issue the certificate, which can take a while after they are added.".into()
        }
        Verdict::Failed(why) => format!("Verdict: failed. {}", why),
    }
}

#[cfg(test)]
#[path = "records_tests.rs"]
mod tests;
