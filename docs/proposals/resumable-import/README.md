# Resumable imports and the Activity list

- **Status:** Proposed. Designed and prototyped in October 2026. Not scheduled, and nothing described here is built yet.
- **Discussion:** [issue #16](https://github.com/Symbiosis-Lab/moss/issues/16)
- **Where the work would land:** the resume mechanism in this repository (`crates/moss-build` and `crates/moss-cli`), the screens in the desktop app.

## The problem

`moss import <url> <folder> --recursive` copies a whole website into a folder of markdown. On a real site that takes minutes to hours, and a lot can interrupt it: the terminal or the app closes, the computer sleeps, the network drops, the site asks moss to slow down. Today any of these loses the run, and running it again writes every page a second time.

This proposal makes an import something you can start and walk away from. It keeps going while moss runs, picks up where it left off after any interruption, and waits on its own when it has to. It also gives the desktop app one place, reachable from the menu bar, that shows what is running, what is waiting, what needs you and what finished while you were away.

Two things are designed here. The first is the resume mechanism. It lives in the importer, so it is the same for the app and for `moss import` in a terminal. The second is the job list in the desktop app, called Activity.

## What you would see

The screens are a prototype, not the app. The launcher screens are built on the app's real markup and styles plus one new stylesheet. The menu bar, the notifications and the alerts are drawn, because the system draws the real ones. Every site name and address is invented. The screens show macOS, and Windows is not drawn yet.

### In the menu bar

The menu gains one section at the top and is otherwise the menu that ships today. There is one line per unfinished job, and one per job that finished since you last looked: the site, a dash, what is happening.

<img src="screens/Menu-importing.png" width="400" alt="Menu bar menu with one line: Riverside Orchestra, importing, 212 of about 530 pages"> <img src="screens/Menu-waiting.png" width="400" alt="Menu with a waiting import that continues at 2:32 PM and a site that is publishing">

<img src="screens/Menu-attention.png" width="400" alt="Menu where one import needs attention, marked with an amber dot"> <img src="screens/Menu-finished.png" width="400" alt="Menu showing two imports that finished while the user was away, one with 4 problems">

<img src="screens/Menu-idle.png" width="400" alt="Menu with nothing running: only the Activity item is added"> <img src="screens/Menu-zh.png" width="400" alt="The menu in Traditional Chinese">

- Each line is a real menu item. Choosing it opens that job in Activity, or opens the site if the job finished clean.
- There are at most three lines, then "2 more…".
- A finished line leaves the menu once you have opened the site or Activity.
- An item named Activity is always there, so the list can be reached when nothing is running.
- The icon gains one variant: a dot when a job needs you or finished with problems. It does not animate while work runs.

### In the launcher

The launcher is the app's home window. Activity is its second panel, beside App Settings, with the same frame. A button left of the gear opens it. The button shows a hollow green ring while a job is unfinished and an amber dot when one needs you. While jobs exist, one line at the foot of the home screen says what moss is doing and opens Activity. The system may hide a menu bar item, so the menu bar cannot be the only way in.

<img src="screens/Main.png" width="400" alt="Launcher home with nothing running"> <img src="screens/Home-importing.png" width="400" alt="Launcher home with a line at the foot: Importing Riverside Orchestra, 212 of about 530 pages">

A row is the site's name, one line of status, at most one button, and a "more" menu. The button is the one thing you are most likely to want: Pause while running, Try Now while waiting on the site, Resume when paused, the fix when attention is needed, Review when done with problems, Open when done.

<img src="screens/Activity-running.png" width="400" alt="Activity with one running import and a Pause button"> <img src="screens/Activity-waiting.png" width="400" alt="Activity with an import waiting on the site, one waiting for the network, and one paused">

<img src="screens/Activity-attention.png" width="400" alt="Activity where one import needs attention, beside a publish and an install in progress"> <img src="screens/Activity-finished.png" width="400" alt="Activity with finished jobs: one with problems and a Review button, one clean, one stopped">

Jobs that need you come first, then unfinished jobs in the order they started, then finished jobs, newest first, under a heading with Clear. Publishing, connecting a domain and installing a plugin stack show the same row with their own status text and no pause.

<img src="screens/Activity-everything.png" width="400" alt="Activity with every kind of row at once, in the order described"> <img src="screens/Activity-empty.png" width="400" alt="Activity with nothing running: Imports and publishing show up here. They keep going after you close the window.">

A thin line under a running row is the only progress drawing. It fills against the best estimate and never moves backwards, drifts when there is no estimate, and stands still in grey while the job waits or is paused. The words carry the honesty: "212 of about 530 pages" while the total is an estimate, "12 pages so far" when there is none. Amber marks a row that needs you or finished with problems, and the row says so in words too. Nothing is red at rest, and a clean finish carries no mark.

The "more" menu is a native popup, so the small window cannot clip it: Stop and Keep What's Imported, Discard Import…, Show in Finder, Copy Site Address.

<img src="screens/Activity-row-menu.png" width="400" alt="The more menu open on a row"> <img src="screens/Activity-dark.png" width="400" alt="Activity in dark mode">

<img src="screens/Activity-zh.png" width="400" alt="Activity in Traditional Chinese">

### The report

Review opens the job in place. The report gives the receipt, then what could not be imported with a reason per page and one Try Again, then what was left behind because it only works on the original site, such as a donation form or a booking calendar. The same facts are a file in the job's folder.

<img src="screens/Report.png" width="400" alt="Report for an import: 526 pages and 1,871 images imported in 12 minutes, 4 pages that could not be imported with reasons, 2 left behind">

### Notifications, closing and quitting

moss notifies once per change, only when a job finishes or needs you, and only when no moss window is visible. A notification about an import that finished clean opens the site. Any other opens Activity. Permission is asked the first time you start an import. Without permission moss stays silent, and the menu bar and Activity carry everything.

<img src="screens/Note-finished.png" width="400" alt="Notification: Import finished. Riverside Orchestra: 530 pages imported."> <img src="screens/Note-problems.png" width="400" alt="Notification: Import finished with 4 problems">

<img src="screens/Note-attention.png" width="400" alt="Notification: Import needs attention. The site is refusing moss's requests."> <img src="screens/Note-quit.png" width="400" alt="Notification after quitting mid-import: Import paused. Riverside Orchestra continues the next time you open moss.">

Closing the last window asks nothing. The job continues and the menu bar shows it. Quitting while only imports are running asks nothing either, because they continue at the next launch. Quitting while work that cannot resume is running, such as a publish in flight, shows one alert that names the consequence, with Cancel as the default.

<img src="screens/Alert-quit.png" width="280" alt="Alert: Quit while Field Notes is publishing?"> <img src="screens/Alert-discard.png" width="280" alt="Alert: Discard this import? Pages you have edited since are kept."> <img src="screens/Alert-continue.png" width="280" alt="Alert: Continue the unfinished import? Continue Import, Start Over, Cancel">

### In the terminal

`moss import <url> <folder>` continues an unfinished job for that source and says so, with `--restart` to discard it and begin again. Ctrl-C pauses. A job started in the terminal appears in the app's Activity, and the reverse, because both read the same record in the folder.

## How resume works

**One page, one place.** A source address maps to exactly one path in the folder, and a page that already carries the same `origin` in its frontmatter is that page. Renaming on a name collision stays for two different sources that want one name. It stops being the answer to "I have seen this before".

**A file that exists is complete.** Every write the import makes goes to a temporary name and is renamed into place: notes, assets, and the finishing pass's rewrites. A crash leaves either the old state or the whole file, never half of one. The atomic writer in `crates/moss-build/src/infra/atomic_write.rs` already does this for text and is extended to bytes.

**A journal records what is safe.** Each job appends to one file in the site folder, `.moss/data/import/<job>/journal.jsonl`, one JSON object per line. A line is written only after the thing it describes is on disk. The kinds are `start` (source, options, machine, moss version), `found` (an address joined the queue), `page` (address, path, content hash), `asset`, `skip`, `fail` (address, reason, tries), `wait` (which host, or none when the network is down; until when, if known; why) and `state` (paused, stopped, needs attention, done). The format carries a version, and a reader ignores a torn last line. Appends are batched, at most one write every two seconds and always before a state change, because the folder may sit in a sync provider where every write is an upload.

**Resuming is replaying.** A resumed job reads its journal and rebuilds what the crawl held in memory. The queue is everything found and not finished. The visited set and the duplicate-content hashes come from the `page` lines, and the asset map from the `asset` lines. Anything that was in flight has no line, so it is fetched again, and the two rules above make that harmless. There is no separate recovery path: starting a job and resuming one run the same code.

**Waiting is a state, not a failure.** When a host answers 429 or 503, the job records a wait for that host with an absolute time. It honours `Retry-After` in full up to an hour and backs off on its own otherwise. Other hosts keep going. Because the time is a clock time on disk, sleep and quit neither lose the wait nor extend it. No network is the same state with a different reason, and it ends when the network returns. A page that fails on its own is retried, then set aside for a second pass at the end, then listed in the report with its reason. It still gets the stub page it gets today, so the gap is visible in the site. A stub is not a finished page: the journal records it as a `fail`, and Try Again or a later import replaces a stub that nobody has edited.

**The job asks for you only when waiting cannot fix it.** That is when the host keeps refusing after the waits have added up to six hours, the disk is full, the folder is gone or cannot be written, or the journal was written by a newer moss.

**Pause, Stop, Discard.** Pause holds the job until you resume it, across launches. Stop ends it and keeps everything imported so far. It is not a failure and asks nothing. Discard removes what this job wrote, after a confirmation, and leaves alone any file whose content no longer matches the hash in the journal, so pages you have edited survive.

**Importing again.** A new import of the same source into the same folder reads the earlier journals first. Pages they wrote that still exist are skipped. Nothing is merged and nothing is overwritten. If the earlier job is unfinished, moss offers to continue it instead.

**The folder may live in a sync provider.** `.moss/data` syncs with the folder, and this design adds a file there. Three rules follow. Unreadable is not absent: a journal the provider has evicted, or that cannot be read, means the job shows as waiting until it can be read, never "no job" and never a fresh start over it. One machine writes: a conflict copy of a journal is shown as needing attention, never merged. And the two-second batch interval is a guess until it is measured inside a Google Drive folder and an iCloud folder.

**Two machines, one folder.** The journal syncs, so a second computer will see an unfinished job. Only the machine named in the job's `start` line resumes it on its own. Elsewhere the job shows as started on another machine and offers Continue Here, which appends a takeover line and makes this machine the owner. On one machine, the app and the terminal are kept from running the same job by a lock in the part of `.moss` that does not sync. The lock names the process and goes stale after two minutes without a heartbeat. Nothing that changes every few seconds is written to the synced side.

## Job states

The screens read the journal, never their own memory of what they last drew. Other tools have a documented failure where a window said Paused while the engine kept running.

| State | Meaning | Ends when | Resumes on its own |
|---|---|---|---|
| Running | moss is working | the work finishes or something below happens | — |
| Waiting | moss stopped itself and knows why: a slow-down from the site, no network | the time passes or the network returns | yes |
| Paused | you stopped it for now | you resume | no |
| Needs attention | only you can unblock it | you act | no |
| Done | finished, with a count of problems that may be zero | — | — |
| Stopped | you ended it and kept what was imported | — | no; importing again continues from it |

## Wording

| Where | English | 繁體中文 | 简体中文 |
|---|---|---|---|
| Panel and menu item | Activity | 活動 | 活动 |
| Running | Importing · 212 of about 530 pages | 匯入中 · 已完成 212 頁，共約 530 頁 | 导入中 · 已完成 212 页，共约 530 页 |
| Running, no estimate | Importing · 12 pages so far | 匯入中 · 目前 12 頁 | 导入中 · 目前 12 页 |
| Waiting on the site | Waiting · the site asked moss to slow down. Continues at 2:32 PM. | 等待中 · 對方網站要求放慢速度，下午 2:32 繼續。 | 等待中 · 对方网站要求放慢速度，下午 2:32 继续。 |
| Waiting on the network | Waiting for a network connection · 31 of about 90 pages so far | 等待網路連線 · 目前 31 頁，共約 90 頁 | 等待网络连接 · 目前 31 页，共约 90 页 |
| Paused | Paused · 12 of about 240 pages imported | 已暫停 · 已匯入 12 頁，共約 240 頁 | 已暂停 · 已导入 12 页，共约 240 页 |
| Needs attention | The site is refusing moss's requests. 31 pages are imported: try again, or keep what's here. | 對方網站拒絕了青苔的請求。已匯入 31 頁，你可以重試，或保留目前的內容。 | 对方网站拒绝了青苔的请求。已导入 31 页，你可以重试，或保留目前的内容。 |
| Done | Imported 530 pages in 12 min | 已匯入 530 頁，用時 12 分鐘 | 已导入 530 页，用时 12 分钟 |
| Done with problems | Imported 526 pages · 4 couldn't be imported | 已匯入 526 頁 · 4 頁未能匯入 | 已导入 526 页 · 4 页未能导入 |
| Stopped | Stopped · 212 pages imported | 已停止 · 已匯入 212 頁 | 已停止 · 已导入 212 页 |
| Another machine | Started on another Mac · 12 of about 240 pages | 在另一台 Mac 上開始 · 12 頁，共約 240 頁 | 在另一台 Mac 上开始 · 12 页，共约 240 页 |
| Buttons | Pause, Resume, Try Now, Try Again, Review, Open, Continue Here, Clear | 暫停、繼續、立即重試、重試、檢視、開啟、在這裡繼續、清除 | 暂停、继续、立即重试、重试、查看、打开、在这里继续、清除 |

A wait shows a clock time once it is a minute or more away, and "in a moment" below that. No product in the survey below shows a clock time for an automatic wait, so this is our own choice: it is stable text that needs no ticking countdown, and it answers when to look again.

## What moss does today

Read from the importer in this repository in October 2026.

- The crawl keeps everything in memory. The frontier, the page cap, the duplicate-content hashes, the asset map, the tally and the per-host pacer live in `crates/moss-build/src/vault/import/scrape/crawl_state.rs`, and none of it is saved. The list of written pages and the home page are locals of the crawl loop in `scrape/run.rs`.
- Pages and assets are written with a direct write, so a crash can leave a half-written file that looks finished. The finishing pass rewrites pages in place the same way.
- A second run over the same folder gives every page a twin, including pages you have edited since. The writer checks only that a file exists and renames on collision. `origin:` is written to frontmatter and never read back.
- A page gets three quick tries, and `Retry-After` is honoured only up to 60 seconds. After that a real page becomes a stub marked as an error. There is no way to cancel a run.
- In the desktop app, the import belongs to the window that started it and nothing about it is saved, so it is lost on quit or a crash. The app imports one page at a time today. A whole-site import runs only from `moss import --recursive`.
- The app already tracks other work that outlives a window (publishing, domain checks, uploads, plugin stack installs), but shows it nowhere once the window is closed. Quitting does not mention it, and the app sends no system notifications.

## Prior art

Product wording below was read from the products' own documentation where it could be reached, and is paraphrased.

- **Resume is a record per item, not a byte offset.** aria2's control file, Scrapy's job directory and Browsertrix's saved state all keep the queue and the seen set. Importers that resume by file existence alone skip half-written files forever, which is why writes are atomic and the journal line comes after the file. Scrapy refuses a job directory from another version and Browsertrix was bitten by format drift, which is why the journal carries a version.
- **Startup is recovery.** This is the crash-only software principle, and Android WorkManager's states work the same way: a wait is a reason attached to a job, and a retry returns to the queue.
- **A saved, absolute resume time per host** goes beyond every tool surveyed. It is cheap, and it is what makes sleep and quit safe.
- **A menu bar item shows a menu, not a popover, and is never the only home**, because the system may hide it (Apple's Human Interface Guidelines).
- **Pause sits beside a way to end, and an ending that loses work confirms** (same guidelines). Browsertrix has Stop, which keeps results, and Cancel, which discards them. Obsidian's importer has Pause, Resume and Stop.
- **A wait is named with its reason and can be overridden.** iCloud Photos names its pause reasons and offers Sync Now. Browsertrix has "Running (Rate Limited)".
- **State is told by shape and words as well as colour**, as OneDrive's status icons do.
- **One notification per change, never repeated** (Apple's guidelines, Microsoft's toast guidance).
- **A results screen gives counts, a reason per item and a report that can be kept**, as Obsidian's importer does.
- **Running an importer again must not duplicate.** Linear skips what it already imported. Several other importers duplicate or only warn.
- **Not checked against a primary source:** the literal menu bar menus of Docker, Tailscale, Backblaze and Transmit, the download lists of Safari, Chrome and Firefox, Finder's copy window, and which apps notify on completion. No source gave a rule for list order, for how long finished rows stay, or for wording a growing total. Those three are our own choices.

## Pieces

Each piece can land alone. The first two pay off in the terminal at once.

| | Piece | Where | How it is proved |
|---|---|---|---|
| 1 | Atomic writes, and one page one place by `origin` | this repository | a test that kills a write midway, and a second run over a real folder that creates no twin |
| 2 | The journal, replay, waits, a cancel signal, and resume in the CLI | this repository | interrupting a real import at several points and comparing the folder with an uninterrupted run; the cost of journal writes measured inside a synced folder |
| 3 | The app's job list, the import continuing after its window closes, and resume at launch | desktop app | |
| 4 | The Activity panel with its report, and the menu bar section | desktop app | |
| 5 | Notifications and the two quit cases | desktop app | |
| 6 | Starting a whole-site import from the app, with the offer to continue an unfinished one | desktop app | |

In this repository, pieces 1 and 2 touch the import writer and the finishing pass (`scrape/writer.rs`, `scrape/run.rs`), a new journal module beside `scrape/crawl_state.rs`, the scrape entry point (a cancel signal and a richer progress value: state, wait reason and time, counts with an estimate), the path rules in `crates/moss-build/src/moss_paths.rs` (a test that the `.moss/data/` rule covers `.moss/data/import`), the `import` command in `crates/moss-cli`, and the importing guide, which should say what a second run does.

## Settled so far

1. The record of a job lives in the site folder, under `.moss/data`. A site is its folder, so its unfinished import travels with it.
2. Jobs run while moss runs, including when only the menu bar item is left, and continue at the next launch. moss does not add itself to login items in the first version.
3. Activity shows every kind of background job from day one. Import is the first kind that can resume. The others appear while they run.
4. moss sends a system notification when a job finishes or needs you, and only when no moss window is visible.
5. Importing again skips what an earlier import wrote. It never merges and never overwrites. A re-import that merges changes from the source is a later, separate question.

## Open questions

These are all about the screens. Pieces 1 and 2 do not depend on them. Opinions are welcome in the issue.

1. Is a clock time right for automatic waits, or should short waits count down?
2. Finished rows stay for seven days, and rows with problems stay until reviewed. Is that right, or should finished rows clear themselves sooner?
3. Should menu bar lines be real items that open the job, as proposed, or dimmed status text with a single Activity command, as Time Machine does?
4. The report opens inside the launcher. The alternative is the site's own window, which has more room but must be opened first.
5. Is 活動 the right word in Chinese, or should the panel be named for jobs (工作)?
