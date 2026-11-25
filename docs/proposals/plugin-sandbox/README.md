# Plugin sandbox: declared hosts, budgets, no network in a build

**Status:** In progress. The engine's checks, the budgets and the `network` field live in this repository (`moss-build`'s plugin host); the registry check and the declarations live in the plugin registry; the Matters plugin's skip on a build lives in the registry; the check on a plugin panel's HTTP commands and the import on folder open live in the desktop app.

A plugin's code runs in moss's QuickJS engine. Today that code can fetch any address on the internet, use as much memory as the machine has, and spin a loop for as long as it likes. This proposal closes those three doors. A plugin reaches only the hosts its manifest names. Each folder's plugins share a memory cap, and any stretch of plugin code that runs without pausing has a time limit. Plugin code that runs because a build ran gets no network at all. The rules live in one place in moss's host, so the desktop app, `moss build` on the command line and a future server all apply the same ones.

## Who writes plugins, and what they can do today

Today every plugin in the registry is written by the moss team. The registry is open to anyone, and a site's folder can hold any plugin a person copies in. So the design assumes a plugin's author is not the person running it, and may be careless or hostile.

What a plugin's engine code can reach today:

| Reach | Today | Gated by |
|---|---|---|
| The network | Any host, any method, through `fetch` and the `http_*`, `fetch_url` and `download_asset` host calls; redirects followed to any host | nothing |
| The site folder | Writes only in a run the person started, and never under `.moss/` except the plugin's own storage | moss's host |
| Programs | Only the binaries named in `requires` (`execute_binary:<name>`), only in a run the person started | moss's host |
| Secrets | Only the plugin's own | moss's host |
| Memory | Unlimited | nothing |
| CPU | A hook with no sign of progress for 60 seconds is cancelled; a timer callback that loops is out of reach | partly |

On the desktop, a plugin that fetches anywhere can send a person's posts, tokens it was given, or the site's unpublished drafts to a host nobody reviewed. On a server running many people's sites in one process, it could also exhaust the machine for everyone, or reach the server's private network. A server therefore runs only plugins written by the moss team until this sandbox exists, each in its own limited process.

Four exits sit outside the engine and stay outside this design. Each is named so nothing claims more than the engine enforces:

- **Programs.** A plugin granted `execute_binary:curl`, `sh` or `git` can reach the network through that program. The grant is the boundary: a plugin that runs programs is outside the sandbox, and moss says so where it shows the grant.
- **The plugin's browser panel.** A panel page runs in the app's webview and can call the same HTTP commands, and the browser's own `fetch`. The HTTP commands take the same host check (step 5 under "Delivery"); the browser's own `fetch` needs a content security policy on the panel, which belongs to the plugin page work that replaces the panel.
- **The system browser.** A plugin can open any web page in the person's browser, sign in through it, or fill the browser panel, and an address can carry data to any host. Only a hook the person started reaches the browser; a build's run, or a call outside any run, is refused.
- **The address a host resolves to.** A declared host can resolve to a private address. On the desktop that is the person's own network and their choice. On a server it is the OS layer's job (see "Later").

## What other systems do

Deno, Chrome extensions, Figma plugins and Extism all deny network access by default and allow it by host, listed in a manifest. Cloudflare's Workers for Platforms passes a CPU and subrequest budget into every call and sends every tenant `fetch()` through one outbound chokepoint. Shopify Functions set hard ceilings on instructions, memory, input and output. VS Code and Obsidian have no permission model for plugin code, and both say so. Two lessons decide details below. Extism treats a missing `allowed_hosts` as "allow all", so here a missing list means none. Chrome disables an extension whose update adds a permission until the person accepts it; moss asks no second question (see "What the person sees"), but a version that widens its hosts says so where the person can see it.

## The manifest

A plugin lists the hosts it contacts in a top-level `network` field:

```json
{
  "network": {
    "hosts": ["api.example-social.net", "*.media.example-social.net"],
    "setting_hosts": ["server"],
    "site": true
  }
}
```

- `hosts` lists host names. Each entry is an exact host, or `*.` followed by a domain, which matches any name below that domain but not the domain itself. There is no bare `*`. Any port and either `http` or `https` match. `localhost` and IP addresses are allowed only when listed literally.
- `setting_hosts` lists settings whose value is an address the person typed, such as their own Mastodon server. The host of that setting's current value is allowed. Only a declared `string` setting that is neither `hidden` nor a secret may be named, whether the person fills it in the plugin's settings or is asked for it at sign-in. moss and the registry check apply this one rule in the same words. The value is read from one file, the plugin's `config.toml` in its folder, and that file has two writers, both moss writing what the person typed: the plugin's settings page, and moss's own sign-in row when the setting is asked for at sign-in (moss writes the answer before the plugin's sign-in runs). A setting asked for at sign-in is not drawn again on the settings page; it shows only inside the sign-in row's account line (`@name@server`), so the account is never repeated as a field. Nothing else may set a setting named in `setting_hosts`: the plugin storage calls refuse to write `config.toml`, as they already refuse `manifest.json`; a plugin's own saved state (`config.json`, written with `saveConfig`) never grants a host; and a value a sign-in learns from the service (an account id, a data server named in an identity record) is never written into a setting named there. So plugin code never sets a host. The file is in the synced folder, so anyone who can edit the folder can change it, the same people who can change the site itself; that is the folder's trust, not the sandbox's. This is the plugin's own `config.toml`, not the site's `.moss/config.toml`, whose `[services.<plugin>]` tables are never read as hosts. A hidden setting with a default is just a fixed host, and belongs in `hosts`.
- `site: true` allows the site's own published domain, which the person configured, so a deploy plugin can check that the site it published answers.
- A host learned at runtime, such as an account's data server named in an identity record, or a custom domain read from the service's API at sign-in, is not followed. A plugin declares the provider's hosting domain as a pattern (`*.host.example`) and a self-hosted server as a setting the person types. Until a plugin asks for that setting, a request to a self-hosted server is refused with the usual message, which names the host. A plugin's own sign-in page never collects the server: what the person types there is the plugin's claim, not moss's record, so the server is asked on moss's sign-in row. A session for any other server needs no check in the plugin: the host check refuses it, as it refuses an undeclared host.
- `"network": {"hosts": []}` says the plugin makes no requests.
- No `network` field means no network. A manifest that predates the field reaches nothing until its plugin is updated.

The README's "Network access" section stays, as the reasons: one line per host saying what the plugin does there. The registry check fails when a plugin with code has no `network` field, when an entry is not a host name, when a setting host names anything but a declared string setting that is neither hidden nor secret, or when a declared host or setting is not mentioned in that section.

Why a field of its own, not entries in `requires`: older moss ignores an unknown top-level field, so the registry can publish declaring versions before any moss enforces them. A new `requires` entry would be refused by today's registry check and is meaningless to older moss. `requires` stays the list of programs.

## Where enforcement lives

Every request a plugin's engine code makes already goes through one transport, which follows redirects one hop at a time. The host list becomes a required argument of that transport and is checked against the first address and against every redirect target, so a declared host cannot bounce a request to an undeclared one. A service whose web pages redirect to an address the person chose elsewhere is read through an endpoint that answers on the declared host itself, such as its API. The pieces that today reach the network beside the transport move onto it:

- The engine's `fetch` calls the transport directly, without knowing which plugin called. It is bound to its plugin the way the other host calls already are, and routed through the same host dispatch, so `fetch` and the `http_*` calls share one check.
- `download_asset` builds its own client that follows redirects internally. It moves onto the shared transport, and its separate client is deleted.

The allowed set for a call is computed in one function from the plugin's declarations, which moss snapshots when it discovers plugins and never re-reads from the writable manifest on disk, together with the current value of each named setting. The desktop app and `moss build` use the same engine and the same function. A future server uses it too, inside the OS layer described below.

## A build gets no network

A build runs plugin hooks nobody asked for at that moment: on every save in the editor, and on every `moss build`. Those runs already may not write the site folder or run programs. From this change they cannot reach the network either. Fetching is something the person does: a Sync, a Publish or an import they start, after which the folder holds what was fetched and every later build reads it from there. Two things follow:

- The same folder builds the same site on every machine, with no network and no surprise requests on each keystroke.
- A plugin that fetches during a build stops doing so. Today two do. A map plugin downloads map scans on build, and is already being replaced by a command the person runs. The Matters plugin's "Auto-Import Posts" setting, on by default, checks Matters for new posts on every build. It keeps its meaning but moves off the build: its process hook already receives the run's `trigger`, and returns at once when the trigger is `background` (a build); the app imports when the folder opens (a relaunch included) and when the person syncs, which are runs the person started (opening a folder is the one build whose process hooks run detached beside the preview, and that run carries the `settings_manual` trigger; every other build's carries `background`), and a new `moss sync <folder>` command, added with step 4, is that run for someone who builds from the command line: it runs the folder's process hooks once with the `settings_manual` trigger, the one the app's folder-open import and Sync pass, then exits, and the next `moss build` reads what they wrote. A run with that trigger is a person's run, which may write the folder and run programs, so `moss sync` admits plugins through the same folder-trust gate as `moss deploy`: without `--allow-plugins`, a folder whose plugins wait on the folder's answer is refused with their names, and code that came with a folder passed on to someone never runs unasked. So new posts still arrive and no build touches the network. Every computer that opens a synced folder runs that import, so it writes a file only when its bytes change: a second open with nothing new on Matters writes nothing, and two computers never trade copies of the same post into conflict files.

## Budgets

| Budget | Default | What the person sees when it is hit |
|---|---|---|
| Memory, for all plugins of one open folder together | 256 MiB | "*Plugin* ran out of memory (256 MB) and was stopped. The folder's plugins were restarted." The engine is rebuilt, so every plugin of that folder starts fresh, not only the one that was running. |
| Code running without pausing | 10 seconds | "*Plugin* ran for 10 seconds without pausing and was stopped." This also reaches a timer or interval callback that loops, which the 60-second progress check cannot reach today. |
| No progress at all | 60 seconds (unchanged) | The existing timeout message. |
| One request's time limit | The plugin's own `timeoutMs`, at most 5 minutes; 30 seconds when unset | "Request timeout after …", as today. |
| One response read into plugin code | 32 MiB | "*host* sent more than 32 MB; *plugin* can read at most 32 MB in one response." A download written to disk is not counted. |

Why these numbers. Cloudflare gives one Worker isolate 128 MB. A folder's engine holds every plugin of the site at once, and an import holds a batch of posts in memory, so the cap is twice that. Ten seconds of unbroken JavaScript is far beyond anything a plugin needs: QuickJS runs millions of operations in that time, and every request, file read or timer is a pause. The progress timeout stays for code that waits forever. A hook's total wall time is not capped, because a long import is legitimate while it makes progress. Request-rate limits are not set on the desktop; they belong on a server.

The memory cap is per folder, not per plugin, because one QuickJS runtime serves all of a folder's plugins. moss's allocator holds the cap, so running out is known by the refused allocation, never by an error's text, which plugin code can write. Any refusal restarts every plugin of the folder, even one the plugin caught, since the memory is shared; the plugin whose call failed gets the error. One runtime per plugin would give each its own cap and is listed under "Later".

The pause limit is checked while JavaScript runs and reset only where moss hands control back to it: a reply to a request or file call, a timer firing, a hook or event handed in. Code stopped after an `await` never settles its promise, so a stop wakes every waiting hook to look again, and the first to look reports it. With hooks of two plugins waiting at once, the stop can be reported on the wrong one; one runtime per plugin removes that too. A timer or event handler that is stopped has no caller, so moss logs the stop; a stopped event handler's plugin is restarted.

## What the person sees

moss asks one question about a folder's plugins: whether to trust the folder. Trust decides whether the folder's plugin code runs at all, and it is checked when a plugin is admitted. This design never adds a second question. It bounds what admitted code can do, on every call, the same for a plugin moss ships, one from the registry and one the person trusted with the folder.

- **On the plugin's page**, at install and in its settings, moss lists the hosts the plugin contacts, each with its reason from the README, and "your server, from the *Server* setting" for each setting host. A plugin that runs programs also says so: "Runs git and sh on your computer. These can reach any address." A plugin with no hosts says nothing about the network. A plugin whose files do not match a version the registry published, whether copied into the folder or a registry plugin whose manifest was edited, has hosts no registry review has seen; this page shows them as declared by the plugin itself, and the trust question stays the one question. The match is by the files' hash, not the plugin's id.
- **When a request is refused**, the plugin's call fails with an error the plugin can show, and moss logs it under the plugin's name: "*Plugin* tried to reach *host*, which it does not list in its manifest." In a build: "*Plugin* tried to reach *host* during a build; plugins reach the network only when you Sync or Publish." Code still running after the person's run ended, such as a timer, is refused the same way, with "outside a run you started" in place of "during a build". A plugin with no `network` field: "*Plugin* does not list the hosts it contacts, so it cannot reach the network. Update it in Plugins." A plugin copied into the folder with no registry version gets the same refusal with "Its manifest needs a `network` field." in place of the update, because the fix is in its manifest, which its author or the person edits.
- **When an update adds a host**, the update row names the new host. It is information and asks nothing; the update takes the wider list.

Who grants: the manifest declares; installing or updating the plugin, which the person does, accepts it; each setting host is whatever the person typed. The sandbox keeps no approval record of its own: the declaration lives in the plugin's manifest, a setting host in the plugin's settings, and trust in the folder trust record, so there is nothing of its own to keep or sweep.

## Delivery

This proposal is the design, and it is revised in place when the code teaches it something. Each step below is undone by reverting it, keeps no state of its own, and moves or deletes no file in anyone's folder, so none needs a rehearsal on a synced folder.

Where the state lives. The sandbox adds none. A plugin's declaration is its manifest, in the plugin's folder under `.moss/plugins/`, which syncs with the site, so every computer that opens the folder reads the same one. A setting host is the person's value in that plugin's `config.toml`, which syncs the same way. A plugin's secrets stay in moss's secret store on the computer, kept per folder and per plugin, never in the folder, and a plugin still reads only its own. Whether a folder's plugin code runs at all is the folder's trust record.

| Step | Where | An older moss meets it | A newer moss meets an older plugin | Test that fails without it |
|---|---|---|---|---|
| 1. Declare: the `network` field in the manifest type, the registry check, a declaration in every plugin the moss team publishes and ships | this repository, the plugin registry, the desktop app's plugins | Ignores the field: the manifest reader accepts unknown fields, and so does today's registry check | Nothing is refused yet | The registry check fails a plugin with code and no `network` field |
| 2. Host check: the transport takes the allowed hosts; `fetch` bound to its plugin; `download_asset` on the shared transport; plugin storage refuses `config.toml` | this repository | Runs a declaring plugin unlimited, as today | A plugin with no `network` field is refused with the update message | An undeclared host, a redirect to one, and a write to `config.toml` are each refused |
| 3. Budgets: the memory cap, the pause limit, the response cap | this repository | No limits, as today | Same limits for every plugin | A plugin that allocates past the cap, loops in a timer, or reads a larger response gets the message above |
| 4. No network in a build, and `moss sync` | the Matters plugin first, then this repository | Builds fetch as today | An older Matters' build run is refused and logged, and its import happens at the next Sync | A fetch from a run with the `background` trigger is refused; a second import with nothing new writes no file |
| 5. Panel HTTP commands take the host check | the desktop app | Not applicable | Same as step 2 | A panel request to an undeclared host is refused |

Two computers on different moss versions sharing one folder read the same plugin folder. The older one runs a declaring plugin as it always did; the newer one limits it to the hosts it declares, which cover what it does, so both build the same site. An update installed from either computer carries the field, which the older one ignores.

Order. Step 1 is published in the registry before any moss enforces anything. Step 4 goes in three parts: the app's import on folder open and `moss sync` first, so a skipped build loses no import; then the Matters change, published; then the engine rule. Steps 2 to 4 land on the development branch, each with its test seen failing without its call site.

| Checkpoint | Done when | Who says yes |
|---|---|---|
| Design | This proposal, reviewed for structure against the other designs that touch plugin state | the maintainers' review |
| Declarations | The registry check requires `network`; every plugin the moss team publishes or ships declares its hosts; each still signs in, imports and publishes a copy end to end | verified by the builders |
| Host check | An undeclared host, a redirect to one, and a write to `config.toml` refused, each test failing without its call site; the crate's tests green | verified by the builders |
| Budgets | The memory cap, the pause limit and the response cap each tested the same way | verified by the builders |
| No network in a build | A build's fetch refused; Matters imports on folder open and `moss sync` without writing unchanged files | verified by the builders |
| Release | The sites moss maintains built with the released and the candidate moss and compared page by page, with no page worse | the maintainers, before the release that carries step 2 or 4 |

The release is the one step people meet, because it changes what plugins already in their folders may do. Everything before it is verified on the development branch and a test server without asking.

| Component | Owns | Needs from this work | Order |
|---|---|---|---|
| Folder trust | Whether a folder's plugin code runs; the one question | Nothing: the sandbox starts after admission | independent |
| Where state lives | Which files sync with the folder and which stay on the computer | The placement above; the sandbox adds no state | agreed |
| Channel sign-in | The social plugins' manifests and moss's sign-in row | Declarations on its versions; the sign-in row writes typed answers to `config.toml`, and nothing learned at sign-in into a host setting | before step 2 |
| Site identity | What a sign-in writes into a site's services | The rule above for any setting named in `setting_hosts` | before step 2 |
| The plugin registry | The registry check and published versions | The `network` check and declaring versions | step 1 |
| The desktop app | Plugin panels and the import on folder open | The folder-open import, then step 5 | the import before the Matters change; step 5 after step 2 |
| The Matters plugin | Its process hook | Return at once on a build | after the folder-open import and `moss sync`, before step 4's engine rule |

What is not in this round: one runtime per plugin, request-rate budgets and the OS layer on a server (see "Later"). Until the OS layer exists, a server runs only plugins the moss team writes.

## Later

- **An OS layer on a server.** A server will run a plugin's service part in its own process inside an OS sandbox: Landlock and seccomp on Linux, or a gVisor or Firecracker boundary when people's plugins are run for them. That layer also refuses private and loopback addresses that the language layer cannot see behind DNS.
- **One runtime per plugin**, so each plugin has its own memory cap and one plugin's runaway code cannot stop another's.
- **Request-rate and subrequest budgets** per call on a server, and per-tenant quotas.
- **WebAssembly plugins**, whose imports are capabilities by construction and whose fuel or epoch limits make CPU budgets exact.
- **A content security policy on plugin pages**, when the plugin page replaces the browser panel.
