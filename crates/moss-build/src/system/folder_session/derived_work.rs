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

    /// Run the calling thread at background priority while the gate is
    /// closed, and at its earlier priority once it opens. For long CPU work
    /// already under way (an image batch), which a hidden window slows rather
    /// than stops: call it before each item.
    ///
    /// A publish in flight is the exception: it waits on this work (and a
    /// deploy's own rebuild runs under it), and a user action is never slowed.
    ///
    /// macOS only, where a thread may lower and raise its own QoS class
    /// freely. Elsewhere this does nothing: an unprivileged thread that lowers
    /// its nice value cannot raise it again. Only the calling thread changes;
    /// the rayon pool jpeg-decoder decodes on keeps its class.
    pub fn pace_this_thread(&self) {
        #[cfg(target_os = "macos")]
        qos::set_background(self.is_closed() && !crate::deploy::freeze::publish_in_flight());
    }
}

#[cfg(target_os = "macos")]
mod qos {
    use std::cell::Cell;

    use libc::qos_class_t;

    thread_local! {
        /// The class and relative priority this thread had before it was
        /// lowered; `None` while it is not lowered.
        static LOWERED_FROM: Cell<Option<(qos_class_t, libc::c_int)>> = const { Cell::new(None) };
    }

    pub(super) fn set_background(background: bool) {
        LOWERED_FROM.with(|saved| match (background, saved.get()) {
            (true, None) => {
                let (class, priority) = current();
                // SAFETY: changes only the calling thread's own QoS.
                if unsafe { libc::pthread_set_qos_class_self_np(qos_class_t::QOS_CLASS_BACKGROUND, 0) } == 0 {
                    saved.set(Some((class, priority)));
                }
            }
            (false, Some((class, priority))) => {
                // SAFETY: as above.
                unsafe { libc::pthread_set_qos_class_self_np(class, priority) };
                saved.set(None);
            }
            _ => {}
        });
    }

    pub(super) fn current() -> (qos_class_t, libc::c_int) {
        let mut class = qos_class_t::QOS_CLASS_UNSPECIFIED;
        let mut priority = 0;
        // SAFETY: reads the calling thread's QoS into two locals.
        unsafe { libc::pthread_get_qos_class_np(libc::pthread_self(), &mut class, &mut priority) };
        (class, priority)
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
