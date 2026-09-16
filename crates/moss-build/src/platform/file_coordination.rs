//! Whole-file materialization through Foundation's file coordination.
//!
//! # Why this exists at all
//!
//! Reading a dataless file is what downloads it — but on a provider that
//! implements [`NSFileProviderPartialContentFetching`], *how much* it downloads
//! is decided by the shape of the read, not by the file. Apple's rule is
//! explicit:
//!
//! > To trigger a partial download, an app must use POSIX read operations to
//! > read part of the file. If you clone the entire file, or read the file
//! > using file coordination, the system requests the entire file.
//!
//! Google Drive adopts partial fetching and answers a short POSIX read with one
//! 4 MiB-aligned window, leaving the file dataless; iCloud does not adopt it and
//! always sends the whole file. Coordination removes the difference — it asks
//! for the file, not for a range, so every provider sends all of it in one
//! round trip. Measured on a real evicted Drive file, 8,594,307 bytes: the
//! coordinated call alone, **with no read inside the accessor block**, returned
//! in 4.26 s with the flag cleared and every block resident.
//!
//! # Why it is confined to one caller
//!
//! Coordination **defeats moss's process-wide dataless fail-fast policy**. xnu
//! gives per-thread Foundation coordination an override that beats even
//! `IOPOL_MATERIALIZE_DATALESS_FILES_OFF`, deliberately, to keep API contracts
//! consistent — see [`super::iopolicy`]. So the safety property moss relies on
//! everywhere else (an accidental read of a dataless file returns `EDEADLK`
//! rather than hanging) **does not hold inside a coordinated read**, on any
//! thread, whether or not it called `materialize_on_this_thread`.
//!
//! That inverts the usual risk. A stray POSIX read on the wrong thread fails
//! fast and is a bug moss survives; a stray coordinated read on the wrong
//! thread is moss#986 again — a thread gone for the life of the process, and
//! if it is the main thread, the app.
//!
//! Therefore: **the only legitimate caller is `build::cloud_prefetch`'s reader
//! pool**, whose threads exist to be lost (ADR-047) and whose wedge accounting
//! already guarantees a file that eats a thread is never handed over again. Do
//! not call this from a build thread, an async task, a Tauri command, or the
//! main thread. If you need bytes rather than materialization, use
//! `cloud_readiness::read_with_materialize_wait`.
//!
//! [`NSFileProviderPartialContentFetching`]: https://developer.apple.com/documentation/fileprovider/nsfileproviderpartialcontentfetching

use std::path::Path;
use std::ptr::NonNull;

use objc2::rc::Retained;
use objc2_foundation::{
    NSError, NSFileCoordinator, NSFileCoordinatorReadingOptions, NSString, NSURL,
};

/// Ask the file provider for the **whole** contents of `path`, and block until
/// it has them or reports why not.
///
/// Blocks with no timeout and no interrupt, exactly like the POSIX read it
/// replaces. Read the module header before adding a second call site.
pub fn materialize_whole_file(path: &Path) -> Result<(), String> {
    let Some(path_str) = path.to_str() else {
        return Err("path is not valid UTF-8".to_string());
    };

    // The accessor block deliberately does nothing. Materializing is the
    // coordinator's doing, not the block's — proven by measurement: a
    // coordinated call with an empty accessor cleared the flag on an 8.20 MiB
    // Drive file in 4.26 s. Reading inside it would only add a copy.
    let err: Option<Retained<NSError>> = {
        let url = NSURL::fileURLWithPath(&NSString::from_str(path_str));
        let coordinator = NSFileCoordinator::new();
        let accessor = block2::RcBlock::new(|_resolved: NonNull<NSURL>| {});
        let mut out_error: Option<Retained<NSError>> = None;
        coordinator.coordinateReadingItemAtURL_options_error_byAccessor(
            &url,
            // Not `ImmediatelyAvailableMetadataOnly` — that option exists
            // precisely to *avoid* downloading, and would turn this into a
            // no-op that still looks like success.
            NSFileCoordinatorReadingOptions::empty(),
            Some(&mut out_error),
            &accessor,
        );
        out_error
    };

    match err {
        None => Ok(()),
        Some(e) => Err(e.localizedDescription().to_string()),
    }
}
