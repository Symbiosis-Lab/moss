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

pub(crate) fn current() -> (qos_class_t, libc::c_int) {
    let mut class = qos_class_t::QOS_CLASS_UNSPECIFIED;
    let mut priority = 0;
    // SAFETY: reads the calling thread's QoS into two locals.
    unsafe { libc::pthread_get_qos_class_np(libc::pthread_self(), &mut class, &mut priority) };
    (class, priority)
}
