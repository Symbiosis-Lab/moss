# Moving an unlisted folder without publishing its address

- **Status:** In progress.

## The problem

Some sites keep pages that are meant to be reached only by a link someone was given: a design proposal sent to one client, a draft shown to one reader. The page carries `noindex`, nothing links to it, and its address holds a random part that nobody can guess. moss already keeps such a page out of the sitemap, the feeds, search and `llms.txt`.

Moving such a folder breaks that promise in four places. We found them on 2026-11-02 by moving two hand-made review folders into one parent folder on a copy of a real site, adding a `[redirects]` line for each old address, and running `moss build` and `moss deploy --dry-run` with moss `0.15.18-dev` at `48c0c21d37`.

1. **The redirect table publishes every secret address.** A site with a real domain gets `_moss/redirects.json`, a public file that lists every redirect, old address and new. After the move it held all three random parts, and a reader of that one file could open every moved page. The host the site uses does not read the file (only the forwarding pages work there), so on that host the file forwards nothing and only discloses. The same file is live today at a fixed address on every published site that has a domain.
2. **A folder takes one line per file.** `[redirects]` matches whole addresses only. Two small folders took 19 hand-written lines: one for the folder, one for every HTML file under it, one for the link-preview picture. Every picture, video and stylesheet left out goes offline.
3. **The publish check lets those files go silently.** `moss deploy --dry-run` reported 36 addresses going offline, each "gone because you deleted its source", and still said "Publish check: passes". The files were moved with the same bytes, not deleted. A moved page is recognised by its uid and the check asks for a redirect; a moved file is not.
4. **A redirected file serves other bytes than its target.** A redirect from an old `.png` to its new address writes a copy of the source file (302,830 bytes, full colour) while the new address serves moss's processed picture (125,778 bytes, palette). The guide calls it "a byte copy of its target".

Two smaller gaps: the forwarding page carries no `noindex` and no link-preview tags, so an old link shared in a chat app re-draws with no card; and `moss describe --json` lists only the `[site]` settings, so the `[redirects]` table is found in the guide but not in the contract an agent reads first.

## The design

**No table of redirects is published.** No host moss publishes to reads `_moss/redirects.json`: moss's own hosting, the GitHub Pages publish and the other publish plugins all serve the forwarding pages and copies, and nothing else. So the build stops writing the file, and every redirect answers through its own forwarding page or copy, as it already did. That keeps an unlisted page's address out of every public file without a second test for which pages are unlisted. The next publish removes the file from a live site, because both moss's hosting and the GitHub Pages publish replace the whole tree. A redirect that had no static form (a non-HTML file sent to a page or an external URL, an old address with capitals or spaces in a folder name) lived only in the table and so worked nowhere; the build now says it does not redirect.

**A folder redirect moves everything under it.** When both sides of a `[redirects]` line end in `/` and the new side is a folder the site serves, the line covers every address under the old folder: each page and `.html` file gets a forwarding page, and each other file gets the served file of the same path under the new folder. One line moves a folder:

```toml
[redirects]
"/review/old-name/" = "/review/client/2026-10-29/"
```

**The publish check knows a moved file.** A file that stops being served at one address while the same bytes start being served at another reads as moved, not deleted, and the check refuses the publish with the `[redirects]` line that keeps it, as it does for a page. Several moved files under one folder are reported as one folder line.

**A redirect copy is the served file.** A file redirect copies the target as this build serves it, so both addresses give the same bytes.

**`describe` lists the table.** `moss describe --json` gains an entry for `[redirects]` (its form, the two static forms, the folder rule).

## What lives where

All of it is in this repository: the redirect table and its forms (`crates/moss-build/src/build/feeds/redirect_table.rs`, `redirects.rs`), the publish check (`crates/moss-build/src/deploy/removal_gate.rs` and the change set it reads), `describe`, and the authoring guide's "Moved and removed pages" section.

## Decided

- A folder redirect covers only the addresses the new folder serves. An address under the old folder with no counterpart is not sent to the new folder's front page: that would hide a missing file, and the publish check already names it.
- No host reads `_moss/redirects.json`, so the file goes entirely. A host that answers redirects itself would get the table through the publish step, never as a public file; none exists yet.

## Not built yet

- The forwarding page carries no link-preview tags, so an old link shared in a chat app draws no card. It does carry `noindex`.
