//! Shadow verification of the incremental carry decision (moss#968 §10 gate 4).
//!
//! `MOSS_INCREMENTAL_VERIFY=1` makes the render phase render the *carried* set
//! anyway and then asks the one question the whole narrowing rests on: **does
//! carrying a page produce the same served bytes as re-rendering it?** It is
//! the tool the first stale-page report gets bisected with, so it has to be
//! honest — a gate that cries wolf on every page is worse than no gate.
//!
//! ## Why the comparison cannot live in the render loop
//!
//! The first cut of this gate compared the freshly rendered `html_page` against
//! `fs::read(output_file_path)` inside the render closure. Those two things are
//! not comparable, and the gate reported `DIVERGES` on 428 of 428 pages of
//! `harbor/潮汐`:
//!
//! * the fresh bytes are **pre-slot-injection** — they still carry the
//!   `<!-- slot:... -->` markers;
//! * the bytes on disk are the **previous build's final output**, already
//!   rewritten by the enhance hook (`build::enhance::inject_slots_into_directory_cached`).
//!
//! The byte delta took exactly four values across the corpus (7552, 7636, 8313,
//! 8397) — the signature of a fixed injected block, not of divergent content.
//!
//! So the render phase only **snapshots**: it keeps the previous build's bytes
//! for each carried page (it already reads that file) and hands them to the
//! pipeline, which compares *after* the enhance hook has run. Final bytes vs
//! final bytes, which is the comparison the design asked for.
//!
//! Cost: one `HashMap<String, Vec<u8>>` over the carried set (~214 pages on the
//! reference vault), allocated only when the env var is set.

use std::collections::HashMap;
use std::path::Path;

/// What a verification run found. Returned (not just logged) so tests can
/// assert on it.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct VerifyReport {
    /// Pages whose final bytes matched the previous build's final bytes.
    pub identical: usize,
    /// Pages whose final bytes differ — a real carry bug.
    pub diverged: usize,
    /// Pages whose re-rendered output could not be read back.
    pub unreadable: usize,
}

impl VerifyReport {
    pub fn compared(&self) -> usize {
        self.identical + self.diverged + self.unreadable
    }
}

/// Snapshots of the previous build's final bytes, keyed by the page's
/// stage-relative path (`doc.url_path`, e.g. `writings/foo/index.html`).
#[derive(Debug, Default)]
pub struct CarryVerification {
    previous_final: HashMap<String, Vec<u8>>,
}

impl CarryVerification {
    /// `Some` only under `MOSS_INCREMENTAL_VERIFY=1`. Deliberately opt-in: the
    /// gate costs a full render, which is precisely what the verdict exists to
    /// avoid.
    pub fn start() -> Option<Self> {
        if std::env::var("MOSS_INCREMENTAL_VERIFY").as_deref() == Ok("1") {
            Some(Self::default())
        } else {
            None
        }
    }

    /// Record one carried page's previous-build bytes, read before the
    /// re-render overwrites the file.
    pub fn record(&mut self, url_path: String, previous_final_bytes: Vec<u8>) {
        self.previous_final.insert(url_path, previous_final_bytes);
    }

    pub fn is_empty(&self) -> bool {
        self.previous_final.is_empty()
    }

    /// Compare each snapshot against the same page's bytes in `stage_dir`
    /// **after** slot injection has run. Logs one `error!` per divergence and
    /// one summary line; returns the tally.
    pub fn compare_final_bytes(&self, stage_dir: &Path) -> VerifyReport {
        let mut report = VerifyReport::default();
        for (url_path, previous) in &self.previous_final {
            let path = stage_dir.join(url_path);
            match std::fs::read(&path) {
                Ok(fresh) if fresh == *previous => report.identical += 1,
                Ok(fresh) => {
                    report.diverged += 1;
                    log::error!(
                        target: "incremental",
                        "MOSS_INCREMENTAL_VERIFY: {url_path} DIVERGES — carried {} bytes, re-render is {} ({})",
                        previous.len(),
                        fresh.len(),
                        describe_first_difference(previous, &fresh),
                    );
                }
                Err(e) => {
                    report.unreadable += 1;
                    log::warn!(
                        target: "incremental",
                        "MOSS_INCREMENTAL_VERIFY: cannot read {} to compare: {e}",
                        path.display(),
                    );
                }
            }
        }
        if report.diverged == 0 && report.unreadable == 0 {
            log::info!(
                target: "incremental",
                "MOSS_INCREMENTAL_VERIFY: {} carried pages verified byte-identical after slot injection",
                report.identical,
            );
        } else {
            log::error!(
                target: "incremental",
                "MOSS_INCREMENTAL_VERIFY: {} of {} carried pages DIVERGE ({} unreadable) — carrying is NOT safe here",
                report.diverged,
                report.compared(),
                report.unreadable,
            );
        }
        report
    }
}

/// A one-line locator for the first differing byte, so a divergence report is
/// bisectable without re-running the build under a diff tool.
fn describe_first_difference(previous: &[u8], fresh: &[u8]) -> String {
    let at = previous
        .iter()
        .zip(fresh.iter())
        .position(|(a, b)| a != b)
        .unwrap_or_else(|| previous.len().min(fresh.len()));
    // Windows around the split point. The slice can land mid-UTF-8 (the
    // reference vault is CJK); `from_utf8_lossy` keeps the log line printable
    // rather than panicking on a byte offset we chose arithmetically.
    let window = |bytes: &[u8]| -> String {
        let end = (at + 60).min(bytes.len());
        let start = at.saturating_sub(20);
        String::from_utf8_lossy(&bytes[start..end]).into_owned()
    };
    format!(
        "first difference at byte {at}: carried {:?} vs re-render {:?}",
        window(previous),
        window(fresh)
    )
}

#[cfg(test)]
#[path = "carry_verify_tests.rs"]
mod tests;
