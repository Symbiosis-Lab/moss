//! Derived work waits while nobody is looking.
//!
//! Some of what a watch-mode build sets off feeds only what is on screen or
//! what a later publish reads: materializing the sealed generation, the
//! generation GC behind it, and the search re-index the seal's tail requests.
//! While the host reports that nobody is looking (`Cadence::Background`),
//! running that after every edit spends CPU on output nobody reads. So the
//! seal's debounce lane asks the folder session's [`DerivedWorkGate`] first.
//! While the gate is closed it keeps its latest request pending (the lane is
//! already latest-wins), and when the cadence turns `Live` the gate opens and
//! the seal runs once over everything that happened in between, then requests
//! one index of the generation it promoted.
//!
//! Forced paths never wait. A publish, a quit or a folder switch settles the
//! lane on its own task, which does not pass through the gate. A process
//! without a visibility signal (the CLI, `moss build --serve --watch`) either
//! attaches no cadence or a constant `Live` one, so its gate is always open
//! and it behaves exactly as it did before the gate existed.

use std::sync::Arc;

use tokio::sync::watch;

use super::FolderSession;
use crate::ops::watch::cadence::Cadence;

/// Whether derived work may run now. Cheap to clone; a clone follows the same
/// cadence. The default gate is always open.
#[derive(Clone, Default, Debug)]
pub struct DerivedWorkGate {
    cadence: Option<watch::Receiver<Cadence>>,
}

impl DerivedWorkGate {
    pub(crate) fn following(cadence: watch::Receiver<Cadence>) -> Self {
        Self { cadence: Some(cadence) }
    }

    /// The gate of `session`, or an open one when there is no session.
    pub fn of(session: Option<&Arc<FolderSession>>) -> Self {
        session.map(|s| s.derived_work_gate()).unwrap_or_default()
    }

    /// True while derived work should wait. A cadence whose sender is gone
    /// can never flip again, so it counts as open rather than strand the work.
    pub fn is_closed(&self) -> bool {
        self.cadence
            .as_ref()
            .is_some_and(|rx| rx.has_changed().is_ok() && *rx.borrow() == Cadence::Background)
    }

    /// Resolve once derived work may run: immediately when the gate is open,
    /// otherwise on the flip to `Live` or when the cadence's sender goes.
    pub async fn opened(&self) {
        let Some(mut rx) = self.cadence.clone() else { return };
        let _ = rx.wait_for(|c| *c == Cadence::Live).await;
    }
}

impl FolderSession {
    /// Gate this folder's derived work on `cadence`. Called once per watch
    /// start, so a re-opened folder follows the cadence its new watch was
    /// given.
    pub fn follow_cadence(&self, cadence: watch::Receiver<Cadence>) {
        *self.cadence.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(cadence);
    }

    /// The gate derived work for this folder waits on. Open until
    /// [`follow_cadence`](Self::follow_cadence) attaches a cadence.
    pub fn derived_work_gate(&self) -> DerivedWorkGate {
        let cadence = self.cadence.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        cadence.clone().map(DerivedWorkGate::following).unwrap_or_default()
    }
}

#[cfg(test)]
#[path = "derived_work_tests.rs"]
mod tests;
