//! The Live/Background cadence signal, and the shared ticker every
//! cadence-aware loop re-arms against.
//!
//! `Cadence` is plain data — no `specta`/`serde` derive, since IPC-facing
//! translation is a host concern (whatever host chooses to expose this over
//! Tauri). What lives here is the one thing every consumer needs regardless
//! of host: how long to wait at each cadence, and a re-armable sleep that
//! races that wait against a value change so a caller never has to wait out
//! a stale interval after a flip.

use std::pin::Pin;
use std::time::Duration;

/// Whether the process should run its background loops at full rate (`Live`)
/// or a slow, low-CPU rate (`Background`) — driven by whatever signal the
/// host has for "is anyone looking at this right now."
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cadence {
    Live,
    Background,
}

impl Cadence {
    /// How often a cadence-aware loop re-checks its periodic work. Two
    /// seconds at `Live` keeps "drop a folder in and see events flow from
    /// it" feeling immediate without making a `read_dir`-class poll
    /// noticeable; thirty seconds at `Background` is slow enough to stop
    /// burning a fanless machine while nobody is looking, and short enough
    /// that a folder left open for a while still self-heals promptly once
    /// looked at again.
    pub fn interval(self) -> Duration {
        match self {
            Cadence::Live => Duration::from_secs(2),
            Cadence::Background => Duration::from_secs(30),
        }
    }
}

/// What woke a [`CadenceTicker::next_tick`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tick {
    /// The armed interval elapsed — do the periodic work.
    Elapsed,
    /// The cadence changed to this value. The caller decides whether a
    /// flip to `Live` should run its periodic work immediately (both of
    /// this crate's callers do; a flip to `Background` does not — the
    /// freshly-armed 30s sleep governs from here).
    CadenceChanged(Cadence),
}

/// A re-armable sleep that races its own deadline against a `Cadence`
/// change, so a cadence-aware loop never has to hand-roll the same
/// `tokio::select!` against `tokio::time::sleep`/`watch::Receiver::changed`.
///
/// One `CadenceTicker` per loop. `next_tick()` always leaves the ticker
/// re-armed for whatever the CURRENT cadence's interval is — the caller
/// never resets it by hand, which is what made the two hand-rolled copies
/// of this shape (open-half reconcile loop, desktop sweep) drift in the
/// first draft of the plan this type replaces.
pub struct CadenceTicker {
    cadence: tokio::sync::watch::Receiver<Cadence>,
    sleep: Pin<Box<tokio::time::Sleep>>,
    /// Set once `cadence`'s sender has dropped. `watch::Receiver::changed()`
    /// resolves immediately (with an error) on every call after that point —
    /// `tokio::select!`'s `_ = ... =>` arm does not distinguish that from a
    /// real change — so racing it forever would busy-loop `next_tick` at
    /// 100% CPU. Once set, `next_tick` stops polling `changed()` and just
    /// waits out the last-known interval on every later call: a sender that
    /// will never send again is exactly "the cadence is fixed from here."
    closed: bool,
}

impl CadenceTicker {
    pub fn new(cadence: tokio::sync::watch::Receiver<Cadence>) -> Self {
        let interval = cadence.borrow().interval();
        Self { cadence, sleep: Box::pin(tokio::time::sleep(interval)), closed: false }
    }

    /// Wait for the next tick: either the armed interval elapses, or the
    /// cadence changes. Either way, the sleep is re-armed for the interval
    /// the (possibly new) cadence wants before this returns, so a caller
    /// that loops on `next_tick()` never has to remember to reset it.
    pub async fn next_tick(&mut self) -> Tick {
        loop {
            if self.closed {
                (&mut self.sleep).await;
                self.rearm();
                return Tick::Elapsed;
            }
            tokio::select! {
                _ = &mut self.sleep => {
                    self.rearm();
                    return Tick::Elapsed;
                }
                changed = self.cadence.changed() => {
                    if changed.is_err() {
                        // The sender is gone. Not a real change — don't
                        // report a spurious CadenceChanged; latch `closed`
                        // and loop back to wait out the sleep already
                        // armed (untouched by this select — see the
                        // `closed` field doc).
                        self.closed = true;
                        continue;
                    }
                    let now = *self.cadence.borrow();
                    self.rearm();
                    return Tick::CadenceChanged(now);
                }
            }
        }
    }

    fn rearm(&mut self) {
        let interval = self.cadence.borrow().interval();
        self.sleep.as_mut().reset(tokio::time::Instant::now() + interval);
    }

    /// The underlying receiver, for a caller that needs the current value
    /// outside of a `next_tick()` result — the desktop sweep's per-tick
    /// `Background` poke reads it this way; the open-half reconcile loop
    /// does not need it.
    pub fn cadence(&self) -> &tokio::sync::watch::Receiver<Cadence> {
        &self.cadence
    }
}

#[cfg(test)]
#[path = "cadence_tests.rs"]
mod tests;
