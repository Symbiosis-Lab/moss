# Reviewing and hiding comments

Nothing reviews a new comment before it goes live. Whatever is in the site's comment data at publish time appears on the page, spam included. Before publishing a site that has comments turned on, list them and hide what does not belong.

## Where comments come from

Comments are synced into `.moss/data/social/*.json`, one file per source (the moss comment service, a syndication target, and so on). Each comment has an `id`, a `source`, sanitized HTML `content`, a creation time and an author. The build reads those files and bakes the visible comments into each page; a page with comments switched off (`comments: false`) shows none whatever the data holds. Listing needs no build.

## List

```
moss comments list [<folder>] [--json]
```

Every comment, newest first: id, source, the page it is on, author, creation time, the first 60 characters of its text with markup stripped, and whether it is hidden. The page is shown as its address and title once a build has run (the last build's article map supplies them); before that it is the page uid, and a line says so. `--json` prints an array of rows (`id`, `source`, `pageUid`, `pageUrl`, `pageTitle`, `author`, `createdAt`, `text`, `hidden`).

Look for the usual spam shapes: links to unrelated sites, product or SEO text unconnected to the page, the same text posted on several pages, a flood from one author.

## Hide and unhide

```
moss comments hide [<folder>] <id>... [--source <name>] [--json]
moss comments unhide [<folder>] <id>... [--source <name>] [--json]
```

- Several ids can be given at once. Every id is checked before anything is written, so a call with one unknown id changes nothing.
- If the same id exists under two sources, the command stops and asks for `--source <name>`.
- Hiding a comment that is already hidden writes nothing and says so. Hiding a parent hides its replies; a reply that is hidden only through its parent is restored by unhiding the parent.
- `--json` prints `{"action", "changed": [...], "unchanged": [...]}` for hide and unhide; any failure prints `{"error": "..."}` and exits 1.

Hiding is reversible and takes effect at the next publish: nothing is erased from the comment data and nothing is sent to a server. A hide is a signed event appended to `.moss/data/social/moderation.jsonl`; `unhide` is a later event that supersedes it. After hiding, build or deploy as usual and check the page.

## The signing key

A hide is signed with the site's own key, kept in `.moss/identity`. The build only honours events signed by that key. The command never creates a key: a new one would not match the key earlier hides were signed with, and every earlier hide would stop applying. On a folder without the key (a fresh copy, or a folder that lost `.moss/identity`) `hide` fails with a plain message and writes nothing; restore the folder's `.moss/identity` rather than working around it.
