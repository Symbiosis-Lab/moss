//! Typed records: what one build did, as counts, durations and words from a
//! fixed list. A [`Record`] is the one value behind both the log's
//! `<kind>.summary key=value…` line and the profile's JSON.
//!
//! [`KINDS`] is the whole vocabulary. A reader that sends records off the
//! machine accepts exactly these keys through [`parse`], so a key added here is
//! accepted there with nothing else to update, and a key missing here is
//! never written: the record counts it under `unlisted` instead.

use std::fmt::Write;

#[derive(Debug, Clone, Copy)]
pub enum Type {
    /// A non-negative integer.
    Count,
    /// A duration, written `<n>ms`.
    Ms,
    /// A duration that may be negative, written `<n>ms`.
    SignedMs,
    /// A build phase's time, written `<n>ms`; samples of one phase add up.
    /// Past the budget, the phase's end logs a performance note.
    Phase { budget_ms: Option<u64> },
    /// One of these words.
    Word(Words),
}

#[derive(Debug, Clone, Copy)]
pub enum Words {
    Fixed(&'static [&'static str]),
    /// A list built from another table, such as the build reasons.
    Computed(fn() -> Vec<String>),
}

impl Words {
    pub fn contains(&self, word: &str) -> bool {
        match self {
            Words::Fixed(list) => list.contains(&word),
            Words::Computed(list) => list().iter().any(|w| w == word),
        }
    }

    pub fn list(&self) -> Vec<String> {
        match self {
            Words::Fixed(list) => list.iter().map(|w| (*w).to_string()).collect(),
            Words::Computed(list) => list(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Key {
    pub name: &'static str,
    pub ty: Type,
    /// A count written as 0 when the record never set it, so a build that did
    /// none of that work says so instead of leaving the key out.
    pub always: bool,
}

#[derive(Debug)]
pub struct Kind {
    /// The line is `<name>.summary …`.
    pub name: &'static str,
    pub keys: &'static [Key],
}

impl Kind {
    pub fn key(&self, name: &str) -> Option<&'static Key> {
        self.keys.iter().find(|k| k.name == name)
    }
}

const fn key(name: &'static str, ty: Type) -> Key {
    Key { name, ty, always: false }
}

const fn count(name: &'static str) -> Key {
    key(name, Type::Count)
}

const fn always(name: &'static str) -> Key {
    Key { name, ty: Type::Count, always: true }
}

const fn phase(name: &'static str) -> Key {
    key(name, Type::Phase { budget_ms: None })
}

const fn budgeted(name: &'static str, budget_ms: u64) -> Key {
    key(name, Type::Phase { budget_ms: Some(budget_ms) })
}

/// Why a build ran, as the record writes it.
pub const TRIGGERS: &[&str] = &["full", "unobserved", "structural", "content", "images-settled", "videos-settled"];

pub fn trigger_word(trigger: &super::BuildTrigger) -> &'static str {
    use super::BuildTrigger;
    match trigger {
        BuildTrigger::Full => "full",
        BuildTrigger::Unobserved => "unobserved",
        BuildTrigger::Structural(_) => "structural",
        BuildTrigger::ContentOnly(_) => "content",
        BuildTrigger::ImagesSettled { .. } => "images-settled",
        BuildTrigger::VideosSettled => "videos-settled",
    }
}

fn reasons() -> Vec<String> {
    super::BuildReason::every().iter().map(ToString::to_string).collect()
}

const TOTAL: Key = key("total", Type::Ms);
// Below zero, phases on the main lane overlapped.
const UNATTRIBUTED: Key = key("unattributed", Type::SignedMs);
const VIDEO: [Key; 4] = [always("videos_cache_hit"), always("video_blobs_validated"), always("videos_converted"), always("videos_detached")];

pub static BUILD: Kind = Kind {
    name: "build",
    keys: &[
        key("outcome", Type::Word(Words::Fixed(&["ok", "error"]))),
        key("trigger", Type::Word(Words::Fixed(TRIGGERS))),
        // The watcher's paths for this build, after its filter; a path the OS
        // reported twice counts twice, so it is a size hint.
        count("changed"),
        key("reason", Type::Word(Words::Computed(reasons))),
        TOTAL,
        UNATTRIBUTED,
        // CPU the whole process used while the build ran, so a preview served
        // or a picture encoded meanwhile counts too.
        count("cpu_ms"),
        // The process's memory footprint when the build ended (macOS only).
        count("footprint_mb"),
        // Names this table lacks, which the line leaves out.
        count("unlisted"),
        count("pages"), always("pages_rendered"), always("pages_carried"), count("publishable"),
        count("missing_references"), count("cancelled"), count("render"), count("output_unchanged"),
        count("served_staging"), count("parked_current"), count("swept"),
        always("slot_html_scanned"), always("slot_html_rewritten"),
        always("background_assets_copied"), always("background_assets_hits"), always("background_assets_misses"),
        always("background_assets_linked"), always("background_assets_stat_only"),
        always("sized_raster_blob_checks"), always("images_unchanged"), always("images_dispatched"),
        always("images_joined"), count("image_encodes"), count("fallback_encodes"),
        always("image_variants_stat_only"), always("image_variants_blob_checks"),
        always("image_transform_records_loaded"), always("files_sealed"),
        always("hash_index_hits_process"), always("hash_index_rehashed_process"),
        always("proved_from_seal"), always("unchanged_at_entry"), count("cache_publication_failures"),
        // Reads a build waited on while the files were still in the cloud.
        count("cloud_reads_waiting"), count("cloud_reads_in_flight"), count("cloud_oldest_read_ms"),
        count("cloud_reads_fetched"), count("cloud_reads_still_in_cloud"), count("cloud_reads_failed"),
        count("cloud_parked"), count("cloud_read_ms"),
        count("search_pages"), count("search_resegmented"), count("search_files_laid"), count("search_bundle_reused"),
        VIDEO[0], VIDEO[1], VIDEO[2], VIDEO[3],
        // The whole pipeline; every other phase nests inside it.
        budgeted("run_pipeline", 120_000),
        // A build that exits fetches its link cards; the fetch stops at 60 s.
        budgeted("link_meta_build_fetch", 30_000),
        budgeted("scan", 2_000),
        // Plugin process hooks, such as a content import, can run tens of
        // seconds; the budget catches a multi-minute hang.
        budgeted("process_hooks_await", 60_000),
        // A spawn is meant not to block; slower means it grew blocking work.
        budgeted("native_process_spawn", 100),
        // Render and encode of the whole site.
        budgeted("build", 30_000),
        budgeted("media_prelude", 5_000),
        budgeted("video_handling", 30_000),
        budgeted("background_asset_copy", 10_000),
        budgeted("stage_source_html", 10_000),
        budgeted("search_read", 2_000),
        budgeted("search_key", 500),
        budgeted("search_index", 10_000),
        // The bundle's own IO, inside `search_index` (`search_lay` also inside
        // `search_read` when a held bundle is served).
        budgeted("search_publish", 2_000),
        budgeted("search_gc", 2_000),
        budgeted("search_lay", 2_000),
        budgeted("seal_wait", 10_000),
        budgeted("seal_persist", 10_000),
        budgeted("seal_promote", 10_000),
        // The seal's own passes, inside `seal_persist`: drop outputs whose blob is
        // gone and repair the pages that named them, keep addresses the build no
        // longer writes, the whole-site link check, the removed addresses, and the
        // manifest's own write.
        budgeted("seal_stamp", 2_000),
        budgeted("seal_repair", 5_000),
        budgeted("seal_kept", 2_000),
        budgeted("seal_reclaim", 2_000),
        budgeted("seal_link_audit", 5_000),
        budgeted("seal_removed", 2_000),
        budgeted("seal_write", 2_000),
        // Removing what the last build left in staging that its manifest no longer names.
        budgeted("staging_sweep", 2_000),
        // Waiting for another build's hold on the folder lock or the stage-write
        // guard. `seal_promote` starts after its waits; `seal_persist` still
        // contains theirs, so subtract `lock_wait` to read its work. The lock
        // itself costs well under a millisecond, so the budget is a tripwire.
        budgeted("lock_wait", 200),
        // A build with no events walking the folder against the last seal: one
        // stat per file, plus a hash for each file whose stat cannot vouch for it.
        budgeted("change_proof", 2_000),
        // Reading the last sealed manifest and measuring `.moss/` against its
        // record, once per build at the entry point.
        budgeted("seal_read", 1_000),
        // Native and plugin slot collection before finalization, then filling
        // the slots into every staged page.
        budgeted("slot_resolution", 10_000),
        budgeted("slot_injection", 10_000),
        budgeted("config_reads", 2_000),
        budgeted("reduce", 5_000),
        budgeted("render_site_files", 5_000),
        // The content graph, page map and per-folder lookups the render reads.
        budgeted("render_inputs", 2_000),
        // The parse cache's session, the media size lookup and the other inputs
        // the markdown loop needs, read before it starts.
        budgeted("render_parse_setup", 2_000),
        // Reading every page's authored source again for missing references and
        // members-only embeds, after the markdown loop.
        budgeted("render_source_evidence", 5_000),
        // Decoding the bundled place-map pack and hashing the map assets, for a
        // site with a place kind; writing the map files after the render.
        budgeted("place_map_decode", 2_000),
        budgeted("place_map_hash", 2_000),
        budgeted("place_map_emit", 2_000),
        // The article map, inventory, previews, media pages and math and diagram
        // pictures written after the home page.
        budgeted("render_site_records", 5_000),
        budgeted("render_redirects_sitemap", 5_000),
        budgeted("render_handoff", 5_000),
        // Parse and resolve every markdown file: the render's largest cost on an
        // image- or link-heavy folder.
        budgeted("render_markdown", 60_000),
        // HTML for every page.
        budgeted("render_html_pages", 20_000),
        budgeted("render_homepage", 2_000),
        // Background sharing cards; a diagnostic budget, not a preview deadline.
        budgeted("render_sharing_cards", 20_000),
        // The feeds, llms.txt and sitemap.xml, written after the preview paints.
        budgeted("background_site_files", 5_000),
        // The slot pass's blobs, records and staged links, stored after the preview paints.
        budgeted("background_slot_batch", 20_000),
        // In-memory hashing and diffing; it should stay far under render_html_pages.
        budgeted("incremental_facade_diff", 10_000),
        phase("entry_check"), phase("resolve_inputs"), phase("comment_refresh"),
        phase("render_auto_folder_indexes"), phase("image_join_wait"), phase("remote_cover_materialize"),
    ],
};

/// A video encode a build started and did not wait for. It writes only a
/// profile record, never a log line, so [`KINDS`] leaves it out.
pub static VIDEO_ENCODE: Kind = Kind {
    name: "video_encode",
    keys: &[key("outcome", Type::Word(Words::Fixed(&["ok", "abandoned"]))), TOTAL, UNATTRIBUTED, VIDEO[0], VIDEO[1], VIDEO[2], VIDEO[3]],
};

/// Which rule refused a publish: every `RefusalKind` in snake case, then the
/// checks that refuse before one is built.
pub const REFUSALS: &[&str] = &[
    "unresolved_inputs", "missing_references", "drafts_linked", "links_in_cloud", "links_in_flight",
    "pending_derivations", "build_unfinished", "removals", "stale_content", "output_collision",
    "no_environment", "foreign_inputs", "not_owner", "stale_copy", "not_signed_in", "moderation_unverified",
    "access", "conflict", "in_progress", "incomplete_generation", "mass_removal", "missing_plugin",
    "needs_setup", "output_collisions", "place_map_pack", "plugin_setup", "site_limit",
    "size_limit", "stale_generation", "subpath_hosting", "unsealed",
];

/// Where a deploy sends the site, as `deploy::profile::target` names it.
pub const DEPLOY_TARGETS: &[&str] = &["seta", "prebuilt", "plugin", "direct"];

/// The deploy's phases, written as 0 in the profile when not entered.
pub const DEPLOY_PHASES: [&str; 10] = ["build", "local_checks", "manifest", "whoami",
    "live_generation", "sync", "upload", "publish", "after_publish", "history_snapshot"];

/// One deploy. The CDN edge and the start times stay in the local profile.
pub static DEPLOY: Kind = Kind {
    name: "deploy",
    keys: &[
        key("outcome", Type::Word(Words::Fixed(&["ok", "refused", "error"]))),
        key("target", Type::Word(Words::Fixed(DEPLOY_TARGETS))),
        key("refusal_kind", Type::Word(Words::Fixed(REFUSALS))),
        TOTAL,
        count("cpu_ms"), count("footprint_mb"), count("unlisted"),
        // Seta uploads; a plugin or dry-run deploy leaves them 0.
        always("manifest_files"), always("files_needed"), always("files_uploaded"), always("bytes_uploaded"),
        always("body_requests"), always("retries"), always("chunked_files"), always("final_concurrency"),
        always("achieved_bytes_per_s"), always("server_files_updated"), always("removed_files"), always("manifest_kb"),
        // The seal's change set; `pages_classified` 0 means no baseline to compare.
        always("pages_added"), always("pages_edited"), always("pages_deleted"), always("pages_restyled"),
        always("pages_classified"),
        // Every request of the deploy, from the seta client's tally.
        always("requests"), always("requests_failed"), always("sent_kb"), always("resent_kb"),
        phase(DEPLOY_PHASES[0]), phase(DEPLOY_PHASES[1]), phase(DEPLOY_PHASES[2]), phase(DEPLOY_PHASES[3]),
        phase(DEPLOY_PHASES[4]), phase(DEPLOY_PHASES[5]), phase(DEPLOY_PHASES[6]), phase(DEPLOY_PHASES[7]),
        phase(DEPLOY_PHASES[8]), phase(DEPLOY_PHASES[9]),
    ],
};

pub static KINDS: &[&Kind] = &[&BUILD, &DEPLOY];

/// A phase's budget, from whichever kind names it.
pub fn budget_ms(phase: &str) -> Option<u64> {
    KINDS.iter().flat_map(|k| k.keys)
        .find(|k| k.name == phase)
        .and_then(|k| match k.ty { Type::Phase { budget_ms } => budget_ms, _ => None })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Count(u64),
    Ms(i64),
    Word(String),
}

/// One record, its fields in the order they were set.
#[derive(Debug)]
pub struct Record {
    kind: &'static Kind,
    fields: Vec<(&'static str, Value)>,
    unlisted: u64,
}

impl Record {
    pub fn new(kind: &'static Kind) -> Self {
        Self { kind, fields: Vec::new(), unlisted: 0 }
    }

    /// Sets `name`, or adds to it when it is a phase taken more than once. A
    /// name the table lacks, a value of the wrong type or a word off the list
    /// is only counted.
    pub fn set(&mut self, name: &str, value: Value) {
        let fits = self.kind.key(name).filter(|key| match (key.ty, &value) {
            (Type::Count, Value::Count(_)) => true,
            (Type::Ms | Type::Phase { .. }, Value::Ms(ms)) => *ms >= 0,
            (Type::SignedMs, Value::Ms(_)) => true,
            (Type::Word(list), Value::Word(w)) => list.contains(w),
            _ => false,
        });
        let Some(key) = fits else {
            self.unlisted += 1;
            return;
        };
        match self.fields.iter_mut().find(|(n, _)| *n == key.name) {
            Some((_, Value::Ms(sum))) if matches!(key.ty, Type::Phase { .. }) => {
                if let Value::Ms(ms) = value { *sum = sum.saturating_add(ms); }
            }
            Some((_, old)) => *old = value,
            None => self.fields.push((key.name, value)),
        }
    }

    pub fn count(&mut self, name: &str, n: u64) {
        self.set(name, Value::Count(n));
    }

    pub fn ms(&mut self, name: &str, ms: i64) {
        self.set(name, Value::Ms(ms));
    }

    pub fn word(&mut self, name: &str, word: &str) {
        self.set(name, Value::Word(word.to_string()));
    }

    pub fn kind(&self) -> &'static Kind {
        self.kind
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.fields.iter().find(|(n, _)| *n == name).map(|(_, v)| v)
    }

    /// What both renderings write: the fields in the order they were set, then
    /// each `always` count left unset as 0, then `unlisted` when names were
    /// left out.
    fn rendered(&self) -> impl Iterator<Item = (&'static str, Value)> + '_ {
        let set = self.fields.iter().map(|(n, v)| (*n, v.clone()));
        let unset = self.kind.keys.iter()
            .filter(|k| k.always && self.get(k.name).is_none())
            .map(|k| (k.name, Value::Count(0)));
        let unlisted = (self.unlisted > 0).then_some(("unlisted", Value::Count(self.unlisted)));
        set.chain(unset).chain(unlisted)
    }

    /// `<kind>.summary key=value…`.
    pub fn line(&self) -> String {
        let mut line = format!("{}.summary", self.kind.name);
        for (name, value) in self.rendered() {
            let _ = match value {
                Value::Count(n) => write!(line, " {name}={n}"),
                Value::Ms(ms) => write!(line, " {name}={ms}ms"),
                Value::Word(w) => write!(line, " {name}={w}"),
            };
        }
        line
    }

    /// The same fields for the profile: words at the top, durations as
    /// `<name>_ms`, counts under `counts`. Phases are left to the profile,
    /// which keeps each sample.
    pub fn json(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut out = serde_json::Map::new();
        let mut counts = serde_json::Map::new();
        for (name, value) in self.rendered() {
            let phase = matches!(self.kind.key(name).map(|k| k.ty), Some(Type::Phase { .. }));
            match value {
                Value::Count(n) => { counts.insert(name.into(), n.into()); }
                Value::Ms(_) if phase => {}
                Value::Ms(ms) => { out.insert(format!("{name}_ms"), ms.into()); }
                Value::Word(w) => { out.insert(name.into(), w.into()); }
            }
        }
        out.insert("counts".into(), counts.into());
        out
    }

    /// A record of `kind` carrying every key once, so a reader can prove it
    /// accepts the whole table.
    pub fn every_key(kind: &'static Kind) -> Self {
        let mut record = Self::new(kind);
        for key in kind.keys {
            let value = match key.ty {
                Type::Count => Value::Count(1),
                Type::Ms | Type::Phase { .. } => Value::Ms(1),
                Type::SignedMs => Value::Ms(-1),
                Type::Word(list) => Value::Word(list.list().pop().unwrap_or_default()),
            };
            record.set(key.name, value);
        }
        record
    }
}

/// The fields of a `<kind>.summary` line when every token is a key of that
/// kind with a value of its type, each key at most once; any other line is
/// `None`, never trimmed.
pub fn parse(line: &str) -> Option<(&'static Kind, Vec<(&'static str, Value)>)> {
    let (head, rest) = line.split_once(' ').unwrap_or((line, ""));
    let kind = *KINDS.iter().find(|k| head.strip_suffix(".summary") == Some(k.name))?;
    let mut fields: Vec<(&'static str, Value)> = Vec::new();
    for token in rest.split(' ') {
        let (name, raw) = token.split_once('=')?;
        let key = kind.key(name)?;
        if fields.iter().any(|(n, _)| *n == key.name) {
            return None;
        }
        let value = match key.ty {
            Type::Count => Value::Count(integer(raw)?.parse().ok()?),
            Type::Ms | Type::Phase { .. } => Value::Ms(integer(raw.strip_suffix("ms")?)?.parse().ok()?),
            Type::SignedMs => {
                let ms = raw.strip_suffix("ms")?;
                let digits = integer(ms.strip_prefix('-').unwrap_or(ms))?;
                let n: i64 = digits.parse().ok()?;
                Value::Ms(if ms.starts_with('-') { -n } else { n })
            }
            Type::Word(list) => Value::Word(list.contains(raw).then(|| raw.to_string())?),
        };
        fields.push((key.name, value));
    }
    Some((kind, fields))
}

fn integer(s: &str) -> Option<&str> {
    (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())).then_some(s)
}

/// What the process has used so far.
pub(crate) struct Usage {
    pub cpu_ms: u64,
    /// The most resident memory it has held.
    pub peak_rss_mb: f64,
}

pub(crate) fn usage() -> Option<Usage> {
    #[cfg(unix)]
    {
        // SAFETY: rusage is plain integers, valid zeroed; getrusage fills it.
        let mut u: libc::rusage = unsafe { std::mem::zeroed() };
        // SAFETY: `u` is a valid, writable rusage.
        if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut u) } != 0 {
            return None;
        }
        let ms = |t: libc::timeval| (t.tv_sec as u64) * 1000 + (t.tv_usec as u64) / 1000;
        // macOS reports ru_maxrss in bytes, Linux in KiB.
        let bytes_per_unit = if cfg!(target_os = "macos") { 1.0 } else { 1024.0 };
        Some(Usage { cpu_ms: ms(u.ru_utime) + ms(u.ru_stime), peak_rss_mb: u.ru_maxrss as f64 * bytes_per_unit / (1024.0 * 1024.0) })
    }
    #[cfg(not(unix))]
    None
}

/// The process's memory footprint, the figure Activity Monitor shows.
pub(crate) fn footprint_mb() -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        // SAFETY: rusage_info_v2 is plain integers, valid zeroed.
        let mut info: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
        // SAFETY: `info` is the struct the V2 flavor writes.
        let rc = unsafe {
            libc::proc_pid_rusage(libc::getpid(), libc::RUSAGE_INFO_V2, &mut info as *mut _ as *mut libc::rusage_info_t)
        };
        (rc == 0).then(|| info.ri_phys_footprint / (1024 * 1024))
    }
    #[cfg(not(target_os = "macos"))]
    None
}

/// `line` without the counts its kind writes as 0 when unset.
#[cfg(test)]
pub(crate) fn without_zero_fill(line: &str) -> String {
    let filled = |t: &str| t.strip_suffix("=0").is_some_and(|name| KINDS.iter().any(|k| k.key(name).is_some_and(|k| k.always)));
    line.split(' ').filter(|t| !filled(t)).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
#[path = "record_tests.rs"]
mod tests;
