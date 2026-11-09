# Comment moderation that survives a new site key

Status: In progress

A site's comment moderation is a log of hide and unhide events in `.moss/data/social/moderation.jsonl`, each signed with the site's key in `.moss/identity`. At build time an event counts only if it was signed by the folder's current key (`ModEvent::verify`, `reduce::resolve_detailed`). So when the folder's key changes, every comment the owner ever hid shows again on the next publish. A folder gets a new key when its secret key is missing for real (a copy or a git clone without `.moss/identity/secret-key`, a site handed to a new owner without the old key) and `IdentityService::ensure_signing_key` mints one; a key that a sync client has only evicted is waited for and does not count as missing.

This proposal keeps earlier moderation in force across every key change. It lives entirely in this repository.

## How others carry trust across a key change

Update frameworks and identity logs keep one chain of key changes rather than re-signing old content. In The Update Framework a new root is signed by the old and the new keys and a client walks every step from the root it trusts; in did:plc, rotation keys sign operations that each name the previous one. Mastodon's account move and Nostr's key-migration drafts do not carry old content's trust across. None of them recovers a lost key cryptographically: that is done out of band, or with a recovery key set aside in advance.

Those verifiers are third parties pinning a root. moss's verifier is the site owner's own build, reading the same folder that holds the key and the log, and whoever can write that folder can already replace the key and append events. A signed chain would prove nothing a plain list in the folder does not, so moss keeps a list.

## The design

The folder keeps the keys it retired in `.moss/identity/previous-keys.jsonl`, one line each, oldest first:

```json
{"pubkey":"<older>","until":1790000000}
```

`until` is when the key stopped being the folder's key (unix seconds), for people reading the file; trust does not depend on it. The list is its own append-only file, not a field of `public.json`, because every writer of `public.json`, older moss versions included, rebuilds that file from a fixed set of fields and would silently drop a field it does not know, most likely when the owner signs in again right after a new key. A reader skips a torn last line.

The reducer trusts the current key and every retired key: one set, built once per build. Events are still signed only with the current key (`append_owner_event` checks that the key it signs with matches `public.json`'s current key); retired keys are trusted for reading only. Ordering is unchanged: `seq` is numbered across the whole log, not per key (`next_seq` takes the highest of the log and the `mod-seq` counter), so a hide by a retired key and a later unhide by the current one still resolve by `(seq, ts)`.

Two things add to the list:

- `Identity::save`, whenever `public.json` already names a different key, appends that key and flushes it before it writes the new one. That covers every path that replaces a folder's key (regeneration in `ensure_signing_key`, an import) in one place; saving or importing the same key retires nothing. A crash in between lists a key that is still current, which trusts nothing new; there is no moment where the folder has the new key and has forgotten the old one.
- `moss comments adopt-key [<folder>] <old pubkey>`, for a folder whose `.moss/identity` was lost along with the key, appends to the same file. It refuses a key that signed no event in this site's log, a key whose events name another site, and a key already trusted. `moss comments list` names any key whose moderation the folder does not trust and points to `adopt-key`, and the build logs a warning for it.

The moderation log's schema is untouched, so older moss versions read the folder as before.

## Not doing

A signed chain of key-change records (The Update Framework's shape, with the old key signing its successor) becomes worth it only if moderation is ever verified off the folder, by readers' browsers or another host; the list already names the keys such a chain would need. Re-signing the events still in effect with the new key would rewrite a signed append-only log at every change.

A new key is also a new device to the hosting service: it is not linked to the owner's account, and the service's record of which keys published the site and any log-sharing consent are keyed by the old one. This list does not carry those. The `moss comments list` notice says a new key also needs signing in again (`moss deploy --email`).

## Size and tests

A few dozen lines in `identity/keypair.rs`, `identity/service.rs`, `comment/reduce.rs` and `comment/moderation.rs`, the CLI verb and the notice; no dependency, nothing in the published site. Tests: a hide signed by key A still hides after the folder regenerates to key B; an unhide by B after a hide by A shows the comment; a chain A, B, C trusts all three; regeneration records the old key before the new one is written; a save of `public.json` leaves the list intact and a torn last line is skipped; `adopt-key` refuses a key with no events, a key whose events name another site, and a trusted key; a key that cannot be recorded as retired is not replaced; `untrusted_signers` names a key the folder does not trust.
