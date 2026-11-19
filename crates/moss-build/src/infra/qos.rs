//! Lowering and restoring the calling thread's QoS class (macOS). Background
//! QoS also throttles the thread's disk I/O, which is what long background
//! work (encodes while a window is hidden, the lock folder's sweep) wants.

use std::cell::Cell;

use libc::qos_class_t;

thread_local! {
    /// The class and relative priority this thread had before it was
    /// lowered; `None` while it is not lowered.
    static LOWERED_FROM: Cell<Option<(qos_class_t, libc::c_int)>> = const { Cell::new(None) };
}

pub(crate) fn set_background(background: bool) {
    set_class(background.then_some(qos_class_t::QOS_CLASS_BACKGROUND));
}

/// Run the calling thread at `class`, or, with `None`, at the class it had
/// before the first call that set one.
pub(crate) fn set_class(class: Option<qos_class_t>) {
    LOWERED_FROM.with(|saved| match (class, saved.get()) {
        (Some(class), earlier) => {
            let earlier = earlier.unwrap_or_else(current);
            // SAFETY: changes only the calling thread's own QoS.
            if unsafe { libc::pthread_set_qos_class_self_np(class, 0) } == 0 {
                saved.set(Some(earlier));
            }
        }
        (None, Some((class, priority))) => {
            // SAFETY: as above.
            unsafe { libc::pthread_set_qos_class_self_np(class, priority) };
            saved.set(None);
        }
        (None, None) => {}
    });
}

pub(crate) fn current() -> (qos_class_t, libc::c_int) {
    let mut class = qos_class_t::QOS_CLASS_UNSPECIFIED;
    let mut priority = 0;
    // SAFETY: reads the calling thread's QoS into two locals.
    unsafe { libc::pthread_get_qos_class_np(libc::pthread_self(), &mut class, &mut priority) };
    (class, priority)
}
