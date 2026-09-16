//! The seam between the build and whoever is running it.
//!
//! Everything that crosses the moss-build ↔ moss-app boundary crosses here, and
//! the reason this file exists as a named place rather than as a habit is that
//! **NORTH-STAR's abort threshold is defined over its contents**: if this seam
//! ever needs a fourth channel-shaped port, or if either named value struct
//! grows past roughly a dozen direct members, the crate line is drawn too low
//! and the answer is an in-crate module boundary instead of a crate
//! (`docs/reference/target/NORTH-STAR.md`, the five-port seam and the threshold
//! beneath it).
//!
//! That threshold is a falsifier, and a falsifier is only worth anything if it
//! is committed before the bet. It was not: this file did not exist while the
//! first three ports were written, so the count it is supposed to catch had
//! nowhere to be counted. Writing it down is the whole point of landing this
//! ahead of the pipeline move
//! (`docs/archive/2026-08-17-open-cli-without-tauri-plan.md`, step 1.5).
//!
//! ## What is here, and what is not
//!
//! Three trait ports are implemented and in use:
//!
//! - [`reporter::BuildReporter`] — everything the build *says*: progress, and
//!   the three shell signals that are not progress.
//! - [`spawner::Spawner`] — where background work *runs*, and — by its absence
//!   — whether there is a runtime to run it on at all.
//! - [`announcer::SealAnnouncer`] (2026-08-24, ADR-010 extension) — the seal
//!   tail's three state handoffs that are not `PipelineEvent`-shaped. Its
//!   module doc argues why it is a new port rather than a fold onto
//!   `BuildReporter`; the row-(o) raise it caused is accepted by name.
//!
//! One more is implemented here and is closure-shaped rather than a trait:
//!
//! - [`LivePortResolver`] — what port a preview server is serving this folder
//!   from, asked when the answer is used. The app half captures the
//!   `AppHandle`; `build/` only calls the closure.
//!
//! Another exists but not yet as a port: `SlotResolver` is a closure type alias
//! in `build/pipeline.rs`, threaded from `build.rs`. It is already the right
//! shape (data in, data out, no `AppHandle`) and becomes a member of this module
//! when the caller splits.
//!
//! `BuildInputs`, `BuildRecord` and `ProjectConfig` are named by the target
//! architecture and are **deliberately not declared here yet**. moss's rule is
//! that a module needs a consumer before it ships — a test proves a type works,
//! a caller proves it matters — and declaring empty structs today would make the
//! seam look finished while putting the threshold's own inputs beyond
//! measurement. They land with the call sites that fill them. `CacheKeyInputs`
//! is the one that already has a filler, so it is declared below.
//!
//! ## The count, as of 2026-08-24
//!
//! Ports: **4** implemented (3 traits + 1 closure), 1 shaped, 2 named.
//! Channel-shaped concerns crossing the seam: **4** — progress, `stage_ready`,
//! `cloud_sync`, `download_progress` — all four carried on **one** trait,
//! `BuildReporter`, because the last three were folded onto it rather than
//! given a trait of their own, which would have been the fourth port the
//! threshold forbids.
//!
//! Value-struct members, the threshold's other clause (~a dozen is the ceiling):
//! `CacheKeyInputs` has **1**, down from 2 when ADR-055 retired the `enhance`
//! capability and took the plugin digest with it. The live reading of this
//! clause is `HostPorts` at 12 — see the ADR-068 note below.
//!
//! ## The reading that was missing (2026-08-27)
//!
//! `CacheKeyInputs` was never the struct the threshold was written about.
//! [`host::HostPorts`] is — it is the seam itself, the thing every host must
//! answer — and it was declared in `build.rs`, one directory above everything
//! row (o) scans. So the clause about "either named value struct" was reading
//! the smaller of the two and reporting 2 while the real one stood at
//! **sixteen** direct members, four past the ceiling, with no way for the row
//! to say so. It is declared in [`host`] as of 2026-08-27 and the row now reads
//! it; the jump is measurement catching up, not surface being added.
//!
//! Six of those sixteen were `Arc<dyn Fn…>` fields that are now the
//! [`host::HostStore`] trait. That fold moves the row's number by zero on
//! purpose — ADR-058 counts a trait method exactly as it counts a struct
//! member, so a fold can never be a way to get under the ceiling. What it is
//! for is that the seam's shapes now have names.
//!
//! Both numbers are now read by ratchet row (o) `seam_surface` rather than by
//! hand — 14 on the day [ADR-058] ruled on it, shrink-only from there. The
//! prose above is the explanation; the row is the test, and it is the one that
//! notices a fourth concern arriving as a fifth method.
//!
//! [ADR-058]: ../../../../../docs/decisions/ADR-058-the-abort-threshold-counts-seam-surface.md
//!
//! NORTH-STAR:125 ends that clause with "a field only one caller sets … fires
//! the falsifier". `CacheKeyInputs` has exactly one filler and treats that as a
//! *defence*, which reads like a contradiction and is not: the clause is about
//! grab-bag config structs, where a single-setter field is evidence the struct
//! is a dumping ground. A two-field value whose whole correctness argument is
//! "computed once, so two binaries cannot disagree" is the opposite case. If a
//! later reader disagrees, that is the ADR's business, not a silent reinterpretation.
//!
//! **That fold is why the threshold currently cannot fire, and it is the open
//! question this seam owes an ADR.** The threshold counts *ports*; folding a
//! concern onto an existing trait moves no count, so any future shell signal can
//! arrive as one more default method forever. Either the threshold counts
//! concerns rather than ports, or the fold needs a rule about when it stops
//! being legitimate. Recorded in the 3c amendment of
//! `docs/reference/target/MIGRATION-STATE.md`.
//!
//! [`LivePortResolver`] found the same edge from the other side: row (o) reads
//! trait methods and struct members, so a **closure** port — the shape this
//! module's own prose calls "already the right shape" — adds real seam width
//! and moves the number by zero. The row is unmoved at 14 with three ports
//! implemented rather than two, which is under-counting, not a clean bill.
//! moss#1066 carries it to the same ADR.

pub mod announcer;
pub mod deploy;
pub mod host;
pub mod reporter;
pub mod spawner;

/// What port a preview server is serving this build's folder from — asked at
/// the moment the answer is used, not before.
///
/// The third closure-shaped port, alongside `SlotResolver`: data out, no
/// `AppHandle`, so `build/` can hold one and call it while the thing that can
/// answer it lives entirely app-side (`build_shell::live_port_resolver`, which
/// reads the `Mutex<ServerState>` the preview server registers itself in).
///
/// ## Why a closure and not an `Option<u16>`
///
/// That is what moss#1061 was. `PipelineConfig::server_port` was a value the
/// caller resolved *before* dispatching the build, and a build carries it for
/// as long as it runs. The cloud supervisor dispatched an arrival rebuild one
/// second after a folder-open — fourteen seconds before `start_preview_server`
/// bound :8080 — resolved `None`, correctly, and then spent nineteen seconds
/// waiting on the stage-write lock. Its `complete` told the frontend there was
/// no preview server while :8080 had been serving the user's site for nineteen
/// seconds, and `preview-state` read that as "the server failed to start" and
/// painted an error over a live download screen.
///
/// A value answers "what was serving when I was dispatched". This answers
/// "what is serving now", and *now* is the only moment a completion is about.
/// A build that started its own server composes a closure that never asks —
/// it already knows, and the lookup is a global lock plus a `canonicalize`.
///
/// ## Why a completion without a port is worth this much trouble
///
/// The cloud gate's way home (moss#964). A build that stops `Deferred` returns
/// `Err` before emitting `complete`, so the frontend never gets a port and
/// `preview-state` has no `serverUrl`. The rebuild that lifts the gate is its
/// last chance to get one, and a portless completion there makes `preview-state`
/// read that rebuild as "the preview server failed to start" — a permanent
/// error over a site that just built. `PipelineConfig::server_port` exists for
/// that case alone; this port is what makes it answerable by every build rather
/// than only by the one whose caller happened to resolve a port first.
///
/// Not counted by ratchet row (o) `seam_surface`, which counts trait methods
/// and value-struct members; a closure port is a type alias, exactly like
/// `SlotResolver`. That is a gap in ADR-058's collector rather than a licence,
/// and it is recorded as one in moss#1066.
pub type LivePortResolver = std::sync::Arc<dyn Fn() -> Option<u16> + Send + Sync>;

/// The resolver a build asks about its own preview port.
///
/// `own` is what the build itself knows — the port `start_preview_server` bound
/// for it, or the one its caller resolved before dispatching it. `live` is the
/// app half's lookup, and it is consulted only when `own` is empty: a build that
/// started the server already knows, and asking costs a global lock plus a
/// `canonicalize` syscall.
///
/// Composed here rather than in `build.rs` because this is the seam's own rule
/// about its own port, and `build.rs` is the half that travels into moss-build.
pub fn port_of_this_build(own: Option<u16>, live: Option<LivePortResolver>) -> LivePortResolver {
    std::sync::Arc::new(move || own.or_else(|| live.as_ref().and_then(|ask| ask())))
}

/// The two values that decide whether the last build's output is still usable.
///
/// Both are computed by the caller and handed in. Neither is *about* the site,
/// which is why they are not part of the build's inputs proper: one describes
/// the moss that ran, the other describes the plugins installed beside it.
///
/// - `builder` — a memoized stat (size + mtime) of the running binary. When it
///   differs from the value in the previous build's `hashes.json`, every HTML,
///   CSS and JS output is stale: embedded assets, class names and layout all
///   live inside the binary, and none of them touch a source file when they
///   change.
/// A second member, `plugin`, stood here until 2026-08-29: a digest over the
/// installed *enhance* plugins' files. ADR-055 retired that capability, which
/// left the digest scanning an empty set — it would have returned `None`
/// forever — so it went with the hook rather than being kept as a field nobody
/// could make non-`None`.
///
/// ## Why this is riskier than it looks
///
/// Turning a computed value into an injected one moves the decision to the
/// caller, and **two callers that disagree compute different cache keys for the
/// same folder** — one rebuilds, the other serves stale pages, and both look
/// fine. `build_parity_test.rs` cannot catch it: it byte-compares *output*, and
/// a divergence here changes what was recomputed rather than what was written.
///
/// The defence is structural rather than a test: `build.rs`'s `run_pipeline` is
/// the single place both the GUI and the CLI enter, so there is exactly one
/// site that fills this struct. Keep it that way. A second producer needs a
/// direct test that the two agree, because nothing else will notice.
#[derive(Clone)]
pub struct CacheKeyInputs {
    /// Fingerprint of the running moss binary.
    pub builder: String,
}
