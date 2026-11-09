# Keep what a build reads downloaded, in a cloud site folder

**Status:** Proposed. The macOS pieces live in this repository (`moss-build`, `platform/`); the Windows piece needs a Windows machine to build and measure.

In a site folder that a cloud service keeps (iCloud Drive, Google Drive, Dropbox, OneDrive, Box), moss should have every file a build reads on the computer before the build needs it, by asking the operating system, and should leave everything else where the person's service put it.

## Why

A cloud service may keep any file only in the cloud and fetch it on first read. For a site that is a slow first build after an update, or after the service freed space: on one folder measured here, about 17,700 of moss's cache records were placeholders, and moss, reading them one at a time, would have spent about 87 minutes on one build. Asking the system to download them (`startDownloadingUbiquitousItem`) where the build meets them took that folder's emptied test copy from 6 min 30 s to about a minute, and that request already ships for the cache records a build finds missing. This proposal extends it to everything in scope and adds Windows.

## Scope: what a build reads

A build does not read the whole folder. A source picture or video whose transforms are cached is vouched for by its stat and never opened, so gigabytes of source media can stay online without costing a build anything. In scope:

- moss's cache records (`.moss/cache/transforms/`);
- the stored outputs the current generation references (`.moss/cache/objects/`);
- the page inputs the scan reads (markdown, data files, the theme).

Everything in scope is downloaded by the build anyway; asking early only moves the wait off the build. Nothing outside it is requested, pinned or walked.

## What was measured on macOS, from moss's own process (not the provider's)

On an emptied copy of a real site in Google Drive's own folder, 7,547 placeholders:

| Route | Result |
|---|---|
| `NSFileManager.startDownloadingUbiquitousItemAtURL`, per file | works: 793 records requested in 1.35 s and resident 29 s later; 3,490 files (420 MB) resident in 125 s. The provider schedules the transfers; no moss thread waits. |
| the same call on a directory | requests nothing below it: 864 records still placeholders a minute later. Requests are per file. |
| File Provider pin, the `com.apple.fileprovider.pinned#PX` extended attribute | set without error, ignored by Google Drive: nothing downloaded in two minutes, on a folder or on single files. Untested on Dropbox and OneDrive. |
| `NSFileProviderManager` (`getDomains`, `evictItem`) | refused: only the provider's own process may use it |
| Finder's "Available offline" / "Keep Downloaded" | not yet measured (needs a person to click) |

So on macOS there is a native way to download and, for Google Drive, none to keep downloaded: a file the service frees again is a placeholder again, and the next build asks again, which costs nothing while everything is local.

## Design

Work per operating-system layer, not per service.

| Layer | Services | Download what a build reads | Keep it downloaded | Check |
|---|---|---|---|---|
| macOS File Provider | Google Drive, Dropbox, OneDrive, Box | `startDownloadingUbiquitousItemAtURL` per file (measured on Drive) | none for a non-provider process; the pin attribute is ignored by Drive and untested on the others | `SF_DATALESS` in `st_flags` |
| macOS iCloud Drive (`bird`) | iCloud | the same call, its documented home | none public | `SF_DATALESS`, and a `.name.icloud` stub before macOS 14 |
| Windows Cloud Files | OneDrive, Dropbox, iCloud for Windows, Box | `CfHydratePlaceholder` | `CfSetPinState(CF_PIN_STATE_PINNED)` on moss's own `.moss/cache` only | `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS` and the pin state |
| Windows, Google Drive | Google Drive (its own virtual drive, not Cloud Files) | a read | none from outside the app | a read that succeeds |
| Linux | most clients keep full copies | nothing to do | nothing to do | none |

**Where the request is made: where the build meets a placeholder.** The scan stats every input and asks for each placeholder it meets, natively and per file; that is what took the first build of the emptied test copy from 389.7 s to 61.3 s. Asking earlier was measured and does not help: a walk of moss's store at build start that asked for every placeholder (6,677 files, most of them pictures) made the first build 91.3 s, because their downloads took the bandwidth the records needed, and one that asked for the records only (1,633) made it 71.9 s, no better than without it. The remaining scan time is not spent waiting for records.

**Pinning.** On Windows, pinning is what a person sees as "Always keep on this device", so moss pins only its own data (`.moss/cache`), never the person's pages and media.

**Check, never trust.** Whether a file arrived is read from the file system's own state, never from the service: `SF_DATALESS` on macOS, the recall attribute on Windows.

**Fall back.** Where the request fails or does not reach a service, moss keeps doing what it does now: bounded reads of what the build needs.

**Tell the person only when it matters.** If a build needed page inputs that were placeholders (counted per build), the folder's status says so once, in the service's own words, with the folder as the subject:

| Service | Its words |
|---|---|
| Google Drive | Available offline |
| Dropbox | Make available offline |
| OneDrive | Always keep on this device |
| iCloud Drive | Keep Downloaded |

For example: "Some of this folder's pages are only online in Google Drive. Choose Available offline for the folder to make builds faster." The service is named from the folder's location (`provider_from_account_dir`); an unknown service gets the sentence without a name.

**Never evict or unpin.** Undoing either is the person's choice.

## Prior art

- **Refuse.** Zotero's documentation says not to keep its data directory in a cloud folder; Lightroom Classic will not open a catalog from a network volume. Both hold a database that cannot be fetched piece by piece.
- **Advise.** Obsidian and Logseq keep notes as plain files in any folder and, when a service frees them, tell people to keep the folder downloaded; neither asks the system on its own.
- **Use the platform.** Apple's own document apps find and fetch iCloud files with `NSMetadataQuery` and `startDownloadingUbiquitousItem`. On Windows, OneDrive's "Always keep on this device" is the Cloud Files pin state, which any program may set on a file under the sync root.
- moss sits with the last group: the folder is the site and stays a plain folder, so moss asks the system rather than refusing the folder or only advising.

## Cost and coverage

| Piece | Work | Coverage |
|---|---|---|
| macOS: the request wherever a build meets a placeholder in scope, per-build count of placeholder page inputs, status line | about a day | Google Drive measured; Dropbox, OneDrive, Box and iCloud expected through the same call until each is measured |
| Windows: hydrate the same set, pin `.moss/cache` | about three days plus a Windows test machine | OneDrive, Dropbox, iCloud for Windows; Google Drive on Windows only through the fallback |

What it does not cover: keeping Google Drive's files downloaded on macOS, which only the person can choose, and a service that offers no request (none found yet).
