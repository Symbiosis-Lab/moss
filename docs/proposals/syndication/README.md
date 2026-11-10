# Syndication: one contract for posting copies to other networks

**Status:** In progress.

A site is the original, and the networks hold copies (the IndieWeb calls this POSSE: publish on your own site, syndicate elsewhere). An author writes a post once, in a folder of markdown, and moss posts a copy to each network the folder names after the post goes live, then writes each copy's address back into the post. Every network plugs in through one contract, so a new network is one plugin and no new machinery, and moss, not each plugin, owns the parts that must be right every time: which posts go where, never posting twice, and recording where each copy lives.

## The author's side

A folder note says once where its posts go, with the `cascade:` every moss folder already has:

```markdown
---
title: Notes
url: notes
cascade:
  syndicate: [bluesky, mastodon]
---
```

- `syndicate:` names channels, the ids under `[channels.<id>]` in `.moss/config.toml`. One post opts out with `syndicate: false` or names its own list; leaving a channel out of a post's own list is how a post skips that channel, and adding it back undoes the skip. `email` in the list names the site's newsletter, which the app sends; it is not a channel and moss posts nothing for it.
- After a copy is posted, moss adds its address to the post's `syndicated:` list, the field moss reads to avoid posting duplicate copies. Only the `syndicated:` line changes; the rest of the file stays as written. A file whose frontmatter moss cannot edit in place (the simplified `key: value` dialect, a duplicated key) keeps its bytes, and moss reports that the address was recorded only in its own record.
- A post goes out when it first goes live. Naming a channel for a folder does not post the folder's earlier posts: a post that was already live (on any publish target) before the channel was named for it goes out only if its own file names the channel.
- Credentials never enter the folder. They live in moss's secret store, outside the site, entered in the app's own credential prompt or, headless, with `moss plugin secret set <plugin> <key>`, which reads the value from standard input.

This changes one thing for an app user: today a copy goes out only for the page being previewed, when Publish is clicked; with this design every post a folder names a channel for goes out when it first goes live. The app's per-post toggle becomes a write of `syndicate:` into that post.

## When it runs

Two steps, so that a slow or failing network can never hold up or fail a publish.

1. **Plan, when a publish lands.** Every publish route passes one point once its bytes are live (`deploy::landed`). There moss decides, from local files only, which copies are now due, and writes one record for each. No network call happens here, so planning adds nothing a publish's own progress watchdog would have to wait for.
2. **Send, after the publish has returned.** `moss deploy` sends the due copies once the publish reports success, and prints one line per copy as each finishes: its address, or why it is not posted. Before posting a copy it fetches the copy's page, a few times over a few seconds, and holds the copy for the next run if the page does not answer, so no copy links to a page an edge is not serving yet (an onion address is taken on trust). The app sends after its own proof that the site is live, and shows each copy as it goes: sending, then posted, updated, unchanged, refused, unconfirmed, handed off or held. A later publish, or a hosted runner, finds any copies still due and sends them; a copy is never lost because the run that planned it stopped.

`moss deploy --dry-run` shows what the plan would be, per channel: for each copy its form, the text of each part with its length against the network's limit, and the link card. `--dry-run --json` prints the same under `copies`. It posts and records nothing. A site with no address of its own (one published only through a plugin that reports its address at publish) has none to show yet, so the dry run's links to its pages are paths; the copies themselves link to the address the publish reports.

A run sends at most ten copies per channel and holds the rest for the next publish. The case this guards is the first publish of a freshly imported site, where every page is new at once: a burst of old posts on someone's timeline cannot be taken back, so moss says what it held and how to send them.

## What a channel declares

A channel is a plugin with `contributes.channel` in its manifest, as today; posting is what a channel is. A channel that posts through this contract describes, in a `syndication` block, what its network can carry:

```json
"contributes": {
  "channel": {
    "setup": { "settings": [
      { "key": "handle", "type": "string", "required": true },
      { "key": "app_password", "type": "secret" }
    ] },
    "syndication": {
      "forms": ["short", "thread", "teaser"],
      "max_chars": 300,
      "counting": "graphemes",
      "images": { "max": 4, "alt": true },
      "idempotent": true
    }
  }
}
```

- `forms`: the shapes of copy the network takes. `short` is a post's whole text; `thread` is a post written in parts, posted as replies to the first; `long` is a whole article, where the network has an article format; `teaser` is the article's description or opening, cut at a sentence end that fits, with a link to the original; `handoff` means the network has no posting API (below).
- `max_chars`, `counting` (`graphemes`, `chars`, `utf16` as a JavaScript string's length counts, or `weighted`, where a CJK character counts two), `link_chars` (what every address counts as, where the network counts it so: 23 on X and Mastodon, assumed for `weighted`) and `images` bound a copy. moss checks them, so a plugin is never handed a copy its network would refuse.
- `host_setting`: for a network on the author's own server (a Mastodon instance), the setting that holds the server's address. Its host joins the manifest's `domains` as a host the channel's copies live on, so a copy listed by hand there is adopted.
- `idempotent`: the plugin passes the job's `idempotency_key` to its network (a record key, an `Idempotency-Key` header), so a retry cannot make a second copy. It changes what moss does after a crash; see below.

The plugin exports up to four functions, one per operation, each handling exactly one copy. Whether a channel can edit or remove its copies is read from which of them it exports, the way moss already detects every other hook.

| Export | Called when |
|---|---|
| `publish_copy(job)` | a post is due on this network and has no copy there |
| `update_copy(job)` | the post's source changed in the publish that just landed, and the copy's content would differ |
| `delete_copy(job)` | the author asks for a deleted post's copies to be removed; copies otherwise stand, since a folder moved by mistake must not erase a year of posts elsewhere |
| `find_copy(job)` | a send stopped before the network answered: is the copy there? |

A job carries the post's absolute address, its language, date and tags, the form moss chose, and the copy already shaped: the text of each part with its links, each picture's file and alt text, for a teaser its title, description and cover, and for a whole article its title, HTML, description and cover. An update or delete also carries the copy's `id` and `url`. Its `idempotency_key` is derived from the post's note id and the channel alone, so every retry, on any computer, carries the same key, and an edit made between a crash and its retry cannot turn the retry into a second post.

The plugin answers with the `HookResult` every hook returns, with the outcome in its `copy` field, as `check_setup` puts its verdict in `setup`:

- `success: true` with `copy: { id, url }` (for a thread, the first part's, with the rest in `parts`): posted.
- `success: false`: the network refused before anything was posted. `copy.refused` says what kind of refusal it was, so a person knows what to do: `auth` (the credential), `content` (the text or a picture), `rate` (too many posts for now), `network` or `unknown`, with the network's own message. moss tries again on the next publish.
- `success: true` with `copy: { handoff: { text, open_url, id } }`: see below.
- Throwing, a timeout, or no answer: moss cannot know whether a copy exists.

The plugin turns the job into its network's request (Bluesky's facets, Mastodon's form fields) and nothing else: it does not decide whether to post, does not read or write the post's file, and keeps no record of what it posted.

A network with no posting API declares the `handoff` form. Its `publish_copy` returns the shaped copy and the page where the author pastes it; moss prints it after the publish (the app shows it with a Copy button), records the copy as *handed off*, and adopts the copy when the author adds its address to `syndicated:`. Substack is this case: it has no API for posts or Notes, so its channel hands off a teaser and link.

A network where the author finishes an article in its own editor hands off a draft instead: `publish_copy` puts the article into a draft there, opens it for the author where the host can show a page (the app can; a terminal cannot), and returns the published copy if the author publishes it then. Otherwise it returns a hand-off with an empty `text`, the draft's page as `open_url` and the draft's `id`. Each later send asks the channel's `find_copy`, with that `id` as `previous.id`, whether the draft has been published: it answers with the published copy, which moss records as posted and writes back; with the same hand-off while the draft still waits, which moss neither sends again nor reports again; or with no copy, when the author has deleted the draft, which moss sends again. Copies go out one at a time, so while the author writes in the network's editor, the copies after it wait. Matters' articles are this case.

## Never twice

moss keeps one record per copy, in `.moss/deploy/copies/`, beside the records of what was published. One small file per copy, not one shared file, because the folder is often synced by a cloud client, and a sync conflict on one shared file would drop entries; a dropped *posted* entry is a repost. The records travel with the folder, so a second computer publishing it sees what the first one posted.

A record is keyed by the post's note id (the `uid:` moss keeps through renames) and the channel. It holds the copy's id and address, the hash of the content it was posted from, and its state. The key is the uid the post was built under, which the publish that puts it online writes into its file (creating a frontmatter block if it has none); a post whose stamp did not land keeps that uid through the publish record, so its records stay keyed the same.

| State | moss does |
|---|---|
| *due* | sends it (at most ten per channel per run; the rest stay *due*) |
| *sending* (a run stopped mid-call) | if the channel is `idempotent`, sends again with the same key; otherwise looks the copy up first, as for *unconfirmed* |
| *posted* | nothing, unless the post's source changed in a landed publish and `update_copy` is exported |
| *refused* | sends it again on the next run |
| *handed off* | for a draft on the network, looks it up with `find_copy` and records it as posted once published; otherwise nothing, until the author adds the copy's address to `syndicated:` |
| *unconfirmed* | looks the copy up with `find_copy`: found, it is recorded as posted; absent, it is sent; when the channel cannot look it up, nothing, until the author adds the copy's address to `syndicated:` (moss adopts it) or runs `moss deploy --retry-copies=<channel>` |

The *sending* state is written to disk before the network is called and the result after, so a crash between them leaves a mark rather than an empty slot. A network that is not idempotent cannot make a retry safe, so for it moss chooses at most once: a copy may be missing, and the author is told, but it is not posted twice.

Two more guards:

- A `syndicated:` address on a host that a channel owns counts as that channel's copy, so a lost record, or a copy posted before this design, is adopted rather than reposted.
- Two computers publishing the same folder at nearly the same time are not serialized: a sync client is not a lock. An idempotent network still cannot get two copies; for any other, at most once holds for each computer until the records have synced.

The hash only spares work: an unchanged copy is not sent again. It never causes an update by itself, because it is taken over the shaped copy, and a change in how moss shapes copies, or a renamed post's new address, must not edit every copy on a network. An update is sent only for a post whose source changed in the publish that just landed.

## What moss shapes

- A post with no title (its file is named by its date) is a `short` copy when its text fits, a `thread` when it is written in parts (a line holding only `---` between blank lines) and the channel takes threads, and otherwise a `teaser`.
- A post with a title is a `long` copy where the channel declares `long`, and a `teaser` everywhere else.
- Text is the post's markdown as plain text: emphasis dropped, links kept as their address, pictures moved out of the text into the copy's pictures with their alt text. The first link is the copy's link card where the network makes one.

Every copy links back to its page. Short posts, each thread part and handoffs include the page's address in their text, and long articles include it as an HTML link. These links are included before checking the network's length limit; a copy that does not fit becomes a teaser where supported, or is refused. The page adds no links to its copies, visible or hidden, and no comment reply links to them. The `syndicated:` field and `.moss/deploy/copies/` records still track copies for deduplication, editing and deletion.

## What it retires

The earlier path posted through one plugin hook, `syndicate`, that received every article and decided everything itself, including writing `syndicated:` back by re-serializing the author's frontmatter. Its last user, the Matters plugin, now posts through the copy hooks: short posts as Matters moments, articles as drafts the author publishes in Matters' editor. The `syndicate` hook and its context are removed once the app no longer calls them, together with the app's post-publish syndication call and its before-and-after count.

## What lives where

- In this repository: the `syndicate:` key, the planner, the records, the write-back, the `syndication` manifest block and the four exports, the plan at landing and the send after it, the dry-run report, `moss plugin secret set`, and the calls a host makes: send one post's copies now, send the held rest now, preview a post's copies, and remove a deleted post's copies.
- In the plugin registry: one plugin per network, each tested against a local server of that network where one exists (a Mastodon-compatible server, a local AT Protocol data server) and against recorded responses where none does.
- In the app: the credential prompt and the Publish summary of copies, both reading the same records.
- Not in this design: posting on a site's behalf from a hosted service with no computer running. That needs a credential the service can use without seeing it (a broker holding owner-approved, revocable grants), and is a separate design. The contract does not change for it: a due record is the same job wherever it is sent from.

## Build order

1. The contract: records, planner, write-back, the exports, plan at landing and send after, the dry run, `moss plugin secret set`. Tested with a fixture channel.
2. Bluesky, then Mastodon, through the author's existing accounts; then short posts on Matters; then LinkedIn and X.
3. Pictures, which need plugins to send a raw binary request body (today a plugin can send JSON or a multipart form, and Bluesky and LinkedIn take neither for an upload).
4. Replies coming home into the store moss's comments already read.
