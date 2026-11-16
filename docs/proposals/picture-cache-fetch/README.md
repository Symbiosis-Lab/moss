# Fetch a cached picture before encoding it again, in a cloud site folder

**Status:** Proposed. Every piece lives in this repository (`moss-build`).

A site folder kept by a cloud service carries moss's encoded pictures with it: the shared store `.moss/cache/{objects,transforms}` syncs like any other file, and its keys work on every machine (the source's SHA-256, the transform, and parameters read from the synced config; blobs are named by their hash and hashed again before use). But a file the service keeps only in the cloud counts as a miss today, so a second machine encodes every picture the first one already made, and uploads it again. This proposal makes moss download a cached picture when it is cheaper than encoding it, without ever making the person wait for the network.

## Why

Measured on a copy of a real site of 101 pictures in Google Drive, with every record and blob of the shared store evicted to the cloud and no per-machine build tree, which is what a second machine sees on first open. moss 0.15.25 re-encoded **101 of 101 pictures: 43.5 CPU s (39.6 user, 3.9 sys), 31.1 s wall, peak memory 1.51 GB, and 1,068 files written into the synced store** (679 blobs, 389 records), each of them an upload. The same folder with its store local re-encodes 31 pictures in 11.0 CPU s.

On the real folder the copy came from, on one machine, 7,998 of 26,954 blobs and 19,275 of 22,134 records were cloud-only. The store also held 2,112 folders that the service had renamed aside (`ab 2`, `cd (1)`), at least 148 MB; 1,868 of them are shard folders under `objects/`, not copies of the whole store.

## What happens now

- **A cloud-only record is a miss.** `TransformCache` reads it as `RecordRead::Offline` and the picture is encoded. A download is requested only in a managed folder.
- **A cloud-only blob is a miss.** `ObjectStore::ready_blob` skips a placeholder, so `convert_single_image` finds no usable output and encodes.
- **The encode is uploaded again.** `store_file_in_base`, `store_bytes_in_base` and `BlobPublication::run` treat a cloud-only destination as absent and write the blob over it. A blob's path is its hash and the bytes are verified against it, so this never changes shared content, but it is an upload of a file the cloud already holds. Where the new bytes differ from the other machine's, they get a new name, and what changes is the record entry that names them.
- **Records are shared files that are rewritten.** One record per source holds every transform of it (base, rungs, metadata, validation, fit verdicts) and is merge-written whenever an entry changes, by the image and video pipelines, `put_summary`, `write_cached_meta`, and the managed folder's record publication. Two machines writing one record before either has synced is what a service turns into a conflict copy.
- **Shard folders are created on demand and removed when empty.** `create_dir_all` runs per write, and the store's sweep removes a shard it has emptied, so a shard name can be created again later. Two machines creating `objects/ab/` within the service's propagation delay make `ab 2`. The shipped heal (`synced-store-duplicates`) folds copies of `.moss/cache` itself, not of a shard inside it.

## Goals

1. The person never waits on the network. Preview and edits use what is on the machine now, and the original picture stands in until an encoded one exists (shipped: the preview serves the original for a picture added during a session).
2. A cloud-only encoded picture is downloaded in the background, with bounded concurrency. While the person is looking, moss encodes a stand-in only if the download has not finished in about the time the encodes would take.
3. A derivative already published keeps its bytes. When a record names a blob, that blob is the picture's derivative, and a publish waits for it; a re-encode replaces it only when the download fails or the bytes fail their hash. Today's re-encode on a second machine breaks this, not only costs CPU: where its bytes differ, it changes what the published site serves.
4. No two machines rewrite one cache file, so the service has no conflict copy to make.
5. Every fetched file is checked against its hash before use; a bad or partial download falls back to encoding.

## Prior art

- **Bazel** splits the action cache from content storage, verifies downloads (`--remote_verify_downloads`), and falls back to local execution when the remote fails (`--remote_local_fallback`). Its dynamic execution starts the local action only after `--dynamic_local_execution_delay`. "Wait about as long as the encodes, then encode" is that delay with the fallback model, not the race: the encode starts only when the download is late.
- **Nix** publishes one immutable `<hash>.narinfo` per store path, writes the archive before it, and keeps no shared index; presence is "the narinfo exists". A cache that rebuilds different bytes under one key breaks its clients, which is the record problem here.
- **Apple**: `startDownloadingUbiquitousItem(at:)` starts a download with no callback and no priority; `SF_DATALESS` says a file is a placeholder. moss already uses both (`platform::request_download`, `icloud::is_evicted`).
- **Lightroom and Photos** sync small derived copies, each with a checksum, over their own services rather than a general sync folder; Adobe tells people to keep previews out of synced folders. Precedent for syncing derivatives with a checksum each.
- **moss's own measurement** (`keep-site-downloaded`): asking for every placeholder in the store at build start made a first build slower (91.3 s against 61.3 s), because picture downloads took the bandwidth the records needed. Order matters: records first, and only the blobs this build will place.

## Design

### Never upload what the cloud already holds

A cloud-only destination counts as present (`ObjectStore::holds` is already that predicate) in `store_file_in_base`, `store_bytes_in_base` and `BlobPublication::run`. An encode whose blob the shared store already holds keeps its bytes in the per-machine store (`.moss/build.nosync/cache/local-objects`), which `ready_blob` reads first. An unmanaged folder takes the same local-then-publish path a managed one already takes in `store_file`. A blob the shared store lacks is still published there.

### Shard folders created once

The sweep stops removing emptied shard folders under `objects/` and `transforms/`, so a shard name, once created, is never created again. The heal at build start goes one level down: a renamed sibling of a shard (recognised by the same `is_sync_copy_of`) is folded by the same `merge`. Blobs are named by their content, so folding is a set union; a record the canonical shard lacks is moved in, one it has is kept. The emptied sibling is then removed, as the heal already removes an emptied copy of the store.

### Fetch, waiting at most as long as the encodes

One rule asks for records: a read that finds a record cloud-only (`RecordRead::Offline`) requests its download, wherever it happens and in any cloud folder. It replaces today's managed-folder-only request. The blocking build already reads every picture's record when it collects the images to convert (`collect_images_for_conversion`), so the render's whole duration becomes head start for those downloads. Blobs are asked for only once their record is local and names them, and only for pictures this build places, never for the store as a whole.

The wait is the existing `cloud_readiness::await_ready`: its deadline, poll, re-request and cancellation already exist, and its re-request is what makes sure each file is asked for even where the bounded background queue (`BACKGROUND_WAITING`) has dropped a request. In `convert_single_image`'s cache lookup, a record or blob that is cloud-only is no longer a miss: the worker calls `await_ready` on it with the time left on the run's deadline and the run's `cancel_flag`, and then reads as today, a hit if the file arrived and verifies, an encode otherwise. The call is made before the item takes its share of the megapixel budget, so a waiting item does not hold encode capacity.

The deadline is one for the whole run, not one per picture: the estimated wall time of encoding every deferred picture (the sum of per-picture estimates from megapixels at a fixed prior rate, about 0.43 CPU s a picture with all rungs on the measured folder, divided by the pool's threads). A per-picture budget of under a second could never be met: one poll alone waits 750 ms, and a download from the cloud takes seconds. With one deadline, the worst case is about twice the encode time (the wait, then the encodes), and the best case is no encode at all. The rate is a fixed prior; keeping a per-machine rate would be state held only for a heuristic. A background build, with nobody looking, has no deadline: it waits for the downloads until cancelled, because nobody needs a stand-in.

An encode made because the deadline passed is a preview stand-in. When the record names a blob, the stand-in goes to the per-machine tree (`local-objects` and a per-machine entry) and never into the shared record, so the timeout path writes no shared record at all. A publish waits for the named blob and uses the stand-in's encode only if the download fails or the blob fails its hash check. A picture with no record anywhere is encoded and published as today.

The preview does not wait: the router serves the original for a promised picture until its output exists. For the seal, a waiting picture stays in flight with no new code, provided two things hold: the wait runs inside the worker's item, so the item keeps its pending mark until it ends (a render that joined the run then waits for it before sealing), and the wait honours `cancel_flag`, so a superseded run does not hold builds that joined it for the length of the deadline.

### Records written once, if records still conflict

With the stand-in kept out of shared records, two machines write different bytes under one record only when both encode a picture that neither has a record for, before either has synced. Whether that still makes conflict copies is measured after the first three slices: the conflict copies under `transforms/` on the Drive copy, in the two-machine scenario. This piece touches all of `TransformCache`, so it is built only if they do.

A record must carry its output in its name, because two machines are not known to encode the same bytes. Proposed layout: one flat file per source, transform, parameters and output in the existing `ab/cd/` shard, `<source>.<transform>.<params hash>.<output oid>.json`, with the transform's `/` escaped. Two machines that encode differently write two names, never one name twice, so the service has nothing to rename. A reader takes any sibling whose blob is present and verifies.

Everything that rewrites a record today changes with it: fit verdicts (`kept_for`) and blob validation move to the per-machine tree, since they are this machine's findings; `convert_single_image`'s removal of a record whose blob has the wrong size becomes "ignore this sibling"; `put_summary` and `write_cached_meta` write a new sibling; the managed folder's record publication, which exists to merge into a record still in the cloud, is no longer needed. The sweep reads the new names (it now walks three levels of files) and condemns a legacy merged record once every entry in it has a new sibling, so the old reader goes away. This covers all of `TransformCache` (pictures, video, the math render cache, slots, maps, listing styles and the preview router read records), so it gets its own review before it is built.

## Slices

Each lands with a test that fails without it, and is measured on the same Drive copy against 0.15.25: CPU seconds, peak cores, peak memory, files written into the synced store, uploads, and wall time.

1. **Never upload what the cloud already holds.** Test: storing a blob whose shared copy is cloud-only writes nothing into the shared store and leaves the bytes in the per-machine store.
2. **Shard folders created once**: the sweep keeps empty shards, the heal folds renamed shard siblings. Tests: a sweep that empties a shard leaves the folder; a store with `objects/ab 2` holding blobs and a record ends with them in `objects/ab` and no sibling.
3. **Fetch.** Written against `TransformCache` lookups, so the record layout can change under it. Tests use an injected materializer, as `cloud_prefetch` does, and each fails today: a cloud-only blob that arrives before the deadline is a hit with no encode; a picture whose download never arrives does not delay the encodes of pictures whose cache is local; a render that joined the run does not seal before a waiting picture ends; a stand-in encode leaves the shared record unchanged and a publish uses the named blob. Its evidence includes the hit fraction (fetched against encoded) on the 101-picture second-machine open.
4. **Records written once**, only if the measurement above still finds conflicting records, and after its own review.

## Measurement

Every run starts from a reset copy: the subset rebuilt, uploaded, then every record and blob evicted, with no per-machine build tree. 0.15.25 is rerun once on that reset to confirm 101 of 101 and to record peak cores, which the first baseline lacks. Scenarios: a second machine's first open; the same machine after eviction with its build tree kept, opened unchanged and after a one-page edit; and iCloud with "Optimise Mac storage", or a note that it was not run.

## Open questions

- If downloads rarely finish inside the deadline on a real second machine, is the stand-in worth its CPU, or should the preview keep showing originals for longer? The hit fraction from slice 3 answers it.
