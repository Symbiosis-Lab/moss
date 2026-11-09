# Duplicate stores in a synced folder: heal them, and stop making them

**Status:** Proposed. Every piece lives in this repository (`moss-build`).

A site folder kept by a sync service can hold `.moss/cache`, `.moss/cache 2`, `.moss/cache 3` and `.moss/cache 4` side by side. Only the first is read. The others hold tens of thousands of files that every machine sharing the folder downloads, uploads and counts against its storage, and nothing ever removes them. This proposal says why they appear, how moss should fold them back into one store without asking anyone, and how it should stop making them.

## How the duplicates were made

The build tree was split in two: the content-addressed store moved to `.moss/cache/{objects,transforms}`, which syncs with the folder, and everything else to the per-machine `.moss/build.nosync`. An older tree is moved into that shape at the first build start after the update (`build::lifecycle::tree_migration`). Its store step is `move_or_merge`: if `.moss/cache/objects` does not exist, create `.moss/cache` and rename the old store into it; otherwise merge into the one that is there.

On one machine that is correct. On a folder shared by several machines it is a race. Each machine updates on its own schedule, and when the second one runs the move, the first one's new `.moss/cache` has often not arrived through the sync service yet. So `to.exists()` is false on both machines, both create a directory named `cache` in the same parent, and the service resolves the two by renaming one: `cache 2`. Every further machine, or a reinstall that runs the move again before the folder has caught up, adds `cache 3`, `cache 4`. In the case that prompted this proposal, the three duplicates appeared within hours of the two releases that first carried the split, one per machine that opened the site.

The same module already knows this failure for the old root: a `build 2` beside `build` is harvested for blobs that hash to their names and then removed (`renamed_aside_siblings`, `merge`, with `sightings` leaving a root that the provider restores after its removal until the next launch). It does not look for siblings of the store itself.

## Prior art

How sync services name a collision, from their documentation or as observed:

| Service | A second item with the same name becomes |
|---|---|
| Google Drive for desktop on macOS | `name 2` (observed on a shared folder) |
| iCloud Drive | `name 2` |
| Dropbox | `name (Someone's conflicted copy YYYY-MM-DD)` |
| OneDrive | `name-MACHINENAME` |
| Syncthing | `name.sync-conflict-YYYYMMDD-HHMMSS-DEVICE.ext` |

How tools that keep their own data inside a folder survive that:

- **Refuse synced folders.** Zotero's documentation tells users not to keep the data directory in a cloud-storage folder, and Lightroom Classic will not open a catalog from a network volume. Both hold a database whose pages cannot be merged file by file.
- **Content-addressed stores.** git objects, restic packs and git-annex objects are named by their hash, so two copies of a name are the same bytes, and a duplicate directory is harmless once its contents are folded in. What breaks a git repository in a synced folder is the mutable files (refs, the index), not the objects. restic rebuilds its index from the packs it finds, which is exactly a harvest.
- **Mergeable records.** git-annex keeps its location log in a branch merged with a union driver, so two machines' writes never conflict; each line carries a timestamp and the newest line for a key wins.
- **Apps that own their sync.** Bear and Ulysses sync through CloudKit, not through files, so they never meet a file-level conflict. Obsidian asks users to run one sync method at a time and, in its own sync service, merges conflicting notes instead of copying them aside.

moss sits with the second and third groups: it chose to keep its converted media in the folder so a second machine does not encode it again, and the store was built to be content-addressed for that reason.

## First principles

1. **A blob's name is its content.** Two blobs with the same name are the same bytes, so folding one store into another is a set union and cannot conflict. A blob is trusted when it is read (`ready_blob` hashes it), so a move does not need to read it.
2. **A record is regenerable and needs a rule.** A transform record says which blob a transform of a source produced. When two stores hold a record for the same source, the one in the canonical store wins; an entry it lacks that the duplicate has is added. Losing a record costs one encode, never correctness.
3. **A directory in a synced folder is created once and never re-created.** A delete followed by a create, or two machines creating the same name, is what a sync service turns into a duplicate. Creating a store directory lazily, at whatever moment a build first needs it, is the hazard.
4. **Healing is silent and repeatable.** Nobody should be asked to tidy a cache. Every step can be interrupted and run again with the same result.

## Design

### Heal, at every build start

`migrate_build_tree` already runs at each build start and is idempotent. It gains one step: the siblings of `.moss/cache` that have the renamed-aside shape (`cache 2`, `cache 31`, found by the existing `renamed_aside_siblings`) are folded in and removed.

For each sibling, guarded by `sightings` as the old root is:

- **Objects.** Each blob the canonical store lacks is renamed into it. A rename is a metadata change, so a blob the service still holds only in the cloud moves without being downloaded, and it is verified when it is first read, as every blob already is. A blob the canonical store already has is dropped. Nothing is hashed during the heal, which matters because a fail-fast read of a placeholder would otherwise skip most of a duplicate on the very folders this is for.
- **Transform records.** A record the canonical store lacks is renamed in. When both have one and both are readable, the duplicate's entries the canonical record lacks are merged in through `TransformCache::merge`. When either is still in the cloud, the canonical one is kept and the duplicate's is dropped (principle 2).
- **Then** the emptied sibling is removed. If removal fails, it is logged and retried at the next build start. If the service restores it, `sightings` leaves it until the next launch, as for the old root.
- **Logged** once per sibling: how many blobs and records moved, how many were already there, and whether the directory was removed.

The existing `merge` hashes each blob before trusting it (`verify = true` for objects). For store siblings that check moves to the first read, for the reason in the first bullet. The old-root harvest can keep hashing, since those roots are local.

### Prevent, at the one place the store is created

- The store directories are created when a site's `.moss` is first laid out, by the same writer that creates `.moss/config.toml`, not by the first build that needs them.
- `move_or_merge`, and any later code that finds `.moss/cache` missing in a folder that is not new (it has a `.moss/config.toml` older than this process), first asks the provider for the parent's listing and waits for it, as `86ea3f4650` already does for a cache folder the cloud has not downloaded. Only an answer of "absent" from the provider lets it create the directory.
- The old-tree move stays, since older trees still exist, but it creates nothing: if `.moss/cache` does not exist after the wait, it renames the old store in as the store, the one create that cannot be avoided, and logs it.

### Tests

- A folder with `cache` and `cache 2`: after a build start, every blob and record from `cache 2` is in `cache`, `cache 2` is gone, and a second build start does nothing.
- A record in both, with different entries: the merged record holds both, and the canonical entry wins on the same key.
- A sibling blob that is a cloud placeholder (the existing `fault::refuse_dataless` and `icloud::pretend` seams): it is renamed in without a read.
- A sibling the provider restores after removal: left alone on the second sighting.
- Two simulated machines running the old-tree move against one folder, where the second sees no `.moss/cache` until the provider answers: one store, no sibling.

## Open questions

- Should the heal also run when the app opens a folder, before any build, so a site that is only opened and not built is cleaned too? Build start is enough for every folder that is ever previewed.
- How long may the prevention's wait for the provider's listing be before a first build gives up and builds without the shared store? A per-machine build is always correct, so the answer can be short.
