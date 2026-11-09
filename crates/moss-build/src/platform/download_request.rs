//! Ask the operating system to download a cloud placeholder, and return at once.
//!
//! `NSFileManager.startDownloadingUbiquitousItemAtURL` reaches any File
//! Provider domain from a process that is not the provider, Google Drive's
//! included, and the provider schedules the downloads itself: no thread waits
//! and the fail-fast policy is untouched. Measured on an evicted Drive folder,
//! 793 small files were requested in 1.35 s and resident 29 s later. Arrival is
//! checked the way it always is, by the file no longer being dataless.

use std::path::Path;

use objc2_foundation::{NSFileManager, NSString, NSURL};

/// Called from rayon and reader threads, which never drain an autorelease
/// pool: the URL, the manager and a failure's NSError would otherwise pile up
/// until the thread exits, and the watch sweep repeats requests for days.
pub fn request_download(path: &Path) -> Result<(), String> {
    let Some(path_str) = path.to_str() else {
        return Err("path is not valid UTF-8".to_string());
    };
    objc2::rc::autoreleasepool(|_| {
        let url = NSURL::fileURLWithPath(&NSString::from_str(path_str));
        NSFileManager::defaultManager()
            .startDownloadingUbiquitousItemAtURL_error(&url)
            .map_err(|error| error.localizedDescription().to_string())
    })
}
