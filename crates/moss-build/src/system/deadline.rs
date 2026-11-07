//! Wait for blocking work, but only so long.

use std::time::Duration;

/// Run `f` on its own thread and return its result if it arrives within
/// `budget`. `None` when the budget passes first, or when `f` panics.
///
/// The thread is never joined: blocking I/O has no cancellation, so a stuck
/// `f` keeps running until it returns or the process exits. Whatever it writes
/// after the deadline must therefore be safe to land at any moment.
pub fn wait_at_most<T: Send + 'static>(budget: Duration, f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        // A send error means the caller stopped waiting.
        let _ = tx.send(f());
    });
    rx.recv_timeout(budget).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_result_within_the_budget_is_returned() {
        assert_eq!(wait_at_most(Duration::from_secs(5), || 7), Some(7));
    }

    #[test]
    fn work_past_the_budget_is_abandoned_at_the_deadline() {
        let started = std::time::Instant::now();
        let got = wait_at_most(Duration::from_millis(50), || std::thread::sleep(Duration::from_secs(5)));
        assert_eq!(got, None);
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
