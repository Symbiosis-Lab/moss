//! Telling macOS a build is running, so App Nap leaves it alone.
//!
//! macOS naps an idle app with no visible window: every thread drops to
//! background priority and its disk I/O is throttled. Someone editing in
//! another app hides the one building their site, so its build would run
//! napped. A build answers what they did, so it is user-initiated, but it
//! still lets the Mac idle-sleep. Work left running after it naps as before.

use objc2::rc::Retained;
use objc2::runtime::{NSObjectProtocol, ProtocolObject};
use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};

/// Held for the life of one build; the activity ends when it drops.
pub struct BuildActivity(Retained<ProtocolObject<dyn NSObjectProtocol>>);

// SAFETY: `NSProcessInfo`'s activity methods are thread-safe, and the token is
// only ever handed back to `endActivity`, from whichever thread drops it.
unsafe impl Send for BuildActivity {}

#[cfg(test)]
tokio::task_local! {
    /// Activities held on the current task, for a test that scopes it; other
    /// tests' builds run on their own tasks and cannot touch it.
    pub(crate) static HELD: std::cell::Cell<usize>;
}

pub fn begin_build() -> BuildActivity {
    let token = NSProcessInfo::processInfo().beginActivityWithOptions_reason(
        NSActivityOptions::UserInitiatedAllowingIdleSystemSleep,
        &NSString::from_str("Building the site"),
    );
    #[cfg(test)]
    let _ = HELD.try_with(|held| held.set(held.get() + 1));
    BuildActivity(token)
}

impl Drop for BuildActivity {
    fn drop(&mut self) {
        // SAFETY: the token came from `beginActivityWithOptions_reason`.
        unsafe { NSProcessInfo::processInfo().endActivity(&self.0) };
        #[cfg(test)]
        let _ = HELD.try_with(|held| held.set(held.get() - 1));
    }
}
