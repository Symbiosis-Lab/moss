# Plugin Authentication

## Plugins own their auth lifecycle end-to-end

Channel authentication (signing in to GitHub, Matters, or any future
channel) is owned entirely by the plugin via the moss-api SDK. The host
never mediates a channel sign-in on the plugin's behalf.

### Prior art in the shipped plugins

- **GitHub** — uses a device-flow auth loop inside the plugin process
  (`plugins/github/src/auth.ts`): the plugin requests a device code,
  opens the browser, polls GitHub's token endpoint, and stores the
  credential itself.

- **Matters** — opens a webview for the Matters OAuth flow
  (`plugins/matters/src/main.ts`): the plugin controls the webview
  lifecycle, receives the token via URL callback, and persists it.

In both cases the auth UI is the plugin's own surface, not a host
interrupt or progress-panel notice.

### Contract

Plugins **MUST NOT** emit a host `Awaiting{SignIn}` job to request
channel sign-in. The `Awaiting{InApp{SignIn}}` contract on the host side
is reserved for **system-level** prompts where the action lives in the
host's own Settings (e.g. the domain email-verify 403 check). Using it
for channel auth would surface an "Open Settings" button that has no
corresponding channel credential UI there.

If a plugin needs the user to sign in before a hook can proceed, the
correct pattern is:

1. Detect the unauthenticated state in the hook callback.
2. Open the plugin's own auth UI (device-flow prompt, webview, etc.).
3. Await the token before returning from the hook (or cancel the hook
   and prompt the user to trigger it again after signing in).

### Formal host-mediated auth (deferred)

A formal contract for host-mediated channel authentication (where the
host provides a generic credential store and a sign-in affordance for
plugins) is deferred to a separate plugin-SDK plan. Until that plan
ships, plugins are fully responsible for their own credential flow.
