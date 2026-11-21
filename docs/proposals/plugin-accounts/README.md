# Plugin accounts: one Sign in button for every channel

**Status:** Proposed

A channel that posts somewhere needs an account there. Today each plugin solves that its own way: some export `login` and run a sign-in, some ask the person to paste a handle, a server and a token into settings fields, and a plugin writes `boundUserName` into its own config to say the folder is signed in. The credential itself sits under one key per plugin per computer, so two folders signed in to two accounts overwrite each other's token. This proposal makes signing in one contract that every channel follows, so the settings page shows one Sign in button and never a token field.

## Who does what

moss owns where the credential lives, which folder is bound to which account, the life of a sign-in and everything the person sees. The plugin owns talking to the provider: the sign-in itself, checking which account a credential belongs to, and revoking it at the provider when that is possible.

## Three kinds of channel

A manifest already says which kind a channel is, so nothing new is declared:

- **Sign-in channel**: `login` is `true` (the plugin exports `login`) or a sign-in address (moss opens it). Settings shows Sign in, then the account's name and Sign out.
- **Token-only channel**: no `login`, and a `secret` setting the person would have to supply. The app draws no token field. A token already stored keeps working and the row shows the account; otherwise the row says sign-in is not available for this channel yet. A token can be provided headless with `moss plugin secret set <plugin> <key>`; it goes to the pending place a sign-in uses, and the first account `copy_account` names becomes the binding. When the provider rejects a stored token (`rejectSecret`), the row shows signed out; nothing prompts for a new one.
- **No-account channel**: neither. Settings shows no account controls.

This proposal covers channels. A deploy target or other plugin whose only credential is an API token (an IPFS pinning key) keeps its credential prompt until its provider offers a sign-in.

A value the sign-in cannot find out by itself, such as which Mastodon server the account is on, is a `string` setting with `at_sign_in: true`: it is asked inside the sign-in, before `login` runs, and never drawn as a settings field.

## The plugin's side

`login(ctx)` runs the sign-in and returns one of three answers:

```ts
type LoginResult =
  | { status: "completed"; account: { id: string; name?: string } }
  | { status: "cancelled" }
  | { status: "failed"; message: string };
```

`id` is the provider's stable identifier for the account (a DID, a numeric user id, `id@server`), never a display name: copies and secrets are bound to it. `name` is only shown. A sign-in in the person's browser uses `startBrowserSignIn()`, whose `open` rejects with a `SignInCancelled` error when the person closes the sign-in or another one starts, so a plugin can tell cancelled from failed. A sign-in in the action panel uses the panel's browser.

What the sign-in earned is stored with `setSecret` under a key the manifest declares as a hidden `secret`. During `login` those writes go to a pending place moss gives this one sign-in; on `completed` moss moves them to the account the answer names, and on `cancelled`, `failed` or a newer sign-in it throws them away. Outside `login`, `getSecret` and `setSecret` read and write the secrets of the account this folder is bound to. A plugin never puts an account name into a key.

`copy_account(ctx)` keeps its meaning: which account the stored credential belongs to now, answered from what is stored, or `signed_out` when there is none or the provider refuses it. A channel that signs in through an address returns no `LoginResult`; its binding comes from the first account `copy_account` names after the page opens.

An optional `logout(ctx)` revokes the credential at the provider (Mastodon's `/oauth/revoke`, for one) and forgets the settings the plugin's sign-in filled in. moss calls it before forgetting the credential, and signs out locally whatever it answers.

## moss's side

**Status.** A channel's row shows one of four states:

- signed in: `copy_account` named an account;
- signed out: it answered `signed_out`, or nothing is stored. When the folder still records an account, the row offers to sign in again as that account;
- unavailable: the secret store could not be read, which moss knows before asking the plugin. Never shown as signed out;
- not saying: the channel exports no `copy_account`.

A `copy_account` that fails stays an error, as today. These are the account states moss already keeps for copies; nothing new is added to them.

**Binding.** moss records which account each folder is signed in to in `.moss/state.toml`, under `[accounts.<plugin>]` with the account's `id` and `name`, written through the same in-place edit as the deploy state and without raising the file's schema version, so an older moss on another computer can still record a deploy. Plugins cannot write that file. The file travels with the folder (its sync service, its git history), so the binding is per folder, not per computer: signing in as another account on one computer rebinds the folder everywhere, and a computer without that account's secret shows signed out and offers to sign in again. The `id` and `name` are public identifiers, fine in a folder that is a public repository; secrets never leave the computer. Copies keep the account they were sent from, and a copy sent from another account is left alone, as today.

**One sign-in at a time.** moss listens for one browser sign-in at a time, across all channels. Pressing Sign in ends any sign-in already running; a sign-in moss opens by itself, when a publish needs an account, joins one already running instead of starting another.

**Sign out** closes the channel's sign-in panel, calls `logout` if the plugin exports it, removes the secrets the manifest declares, clears this folder's binding, and clears the channel's sign-in browser session. `logout` runs first because it needs the credential to revoke it, and it is where a plugin forgets the settings its sign-in learned (a publication's address, a server), since moss cannot tell those from settings the person typed. The cleared binding is kept as a `signed-out` mark, so a conflicted copy of `state.toml` written before the sign out does not bring it back; a folder with nothing to clear is not written. Another folder on this computer bound to the same account is left as it is, and its row shows signed out because the secrets and cookies are gone; on other computers the folder shows signed out once its synced binding is gone. A step that fails is reported as one that can be retried; the others still happen. Off macOS, clearing the browser session cannot reach the server's httpOnly cookies, so the next sign-in there may find the platform still signed in.

**Browser sessions.** Each channel that signs in through the action panel gets its own cookie store, so signing out of one never signs the person out of another. This needs macOS 14 or later; on older macOS, sign-out clears the channel's own domains from the shared store, as today. A sign-in in the system browser leaves no cookies in moss.

## Where secrets live

In moss's secret store: one file per secret, readable only by the person, under the app's data directory and outside every folder, now under the plugin and the account. moss does not use the system keychain: it would add three platform backends, a fourth store for the command line and headless machines, and an unlock prompt in the middle of a publish. The store is hardened instead: every write goes to a new file that is synced and renamed over the old one, so a crash never leaves an empty token; a file found with wider permissions is replaced by an owner-only one; the folder is excluded from device backups; and on Windows it sits in the person's own roaming application data.

## Moving what exists

- **Bindings.** A folder whose plugin config holds `boundUserId` is bound to that account. Matters keeps only `userName`, a handle that can change, so its folders are bound to the id its `copy_account` names on the next run, not to the handle. moss keeps reading `boundUserName`, `boundUserId` and `login_dismissed` from plugin config until every registry channel ships on this contract, and writes only `state.toml`.
- **Secrets.** A secret under the old per-plugin key moves to the bound account's place only after the new place reads it back; the old file goes last. A move that stops halfway starts again, and never deletes the only copy. Matters' own `access_token:<name>` keys and its pending slot move the same way, and its code for them is deleted.
- **What reads the old binding today** moves to `state.toml`: the check that opens a sign-in by itself when a publish needs one, the sync that follows a sign-in (which keys off `completed` instead), the "Signed in as" row, Matters' account-mismatch guard, and the panel sign-in helper LinkedIn, X and Substack share, whose `copy_account` answers from that config.

## Compatibility

A `login` that returns nothing is read as before, through the old binding, until the plugin is updated. A plugin that returns a `LoginResult` raises its `min_moss_version` to the release that ships this contract, which moss and the registry already enforce, so an older moss does not install it. A channel's old token fields keep their stored values; only the field disappears.

## Order of work

1. This contract, its types in `@symbiosis-lab/moss-api` (with `SignInCancelled`), the compatibility reading, the binding in `state.toml`, and the plugins chapter of `moss guide`.
2. The hardened store scoped by account, the move of old credentials, and `moss plugin secret set`.
3. The settings rows for the three kinds of channel and the four states.
4. Bluesky and Mastodon on the contract: Bluesky returns its DID and handle; Mastodon asks `verify_credentials` for its id and adds `logout`.
5. The other channels.
6. Separate panel sessions and sign-out cleanup.
