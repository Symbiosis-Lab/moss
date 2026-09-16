[@symbiosis-lab/moss-api](../README.md) / index

# index

moss Plugin SDK

Shared types and utilities for moss plugins.

## Example

```typescript
import type { DeployContext, HookResult } from "@symbiosis-lab/moss-api";
import { reportProgress } from "@symbiosis-lab/moss-api";

const MyPlugin = {
  async on_deploy(context: DeployContext): Promise<HookResult> {
    await reportProgress("deploying", 0, 100, "Initializing");
    // ... deployment logic
    return { success: true, message: "Deployed successfully" };
  }
};
```

## Messaging

### AdvisoryAction

```ts
type AdvisoryAction = 
  | "None"
  | {
  Command: {
     label: string;
     run: string;
  };
}
  | {
  InApp: {
     args: unknown;
     label: string;
     op: AdvisoryAppOp;
  };
}
  | {
  Link: {
     href: string;
     label: string;
  };
};
```

The recovery affordance for an advisory, expressed as data — mirrors the
Rust `advisory::Action` (externally-tagged: `"None"` for the unit variant,
`{ Variant: {...} }` for data variants). `Action !== "None"` is the gavel's
deciding input for whether a `Blocking` proposal may pop the panel.

***

### AdvisoryAppOp

```ts
type AdvisoryAppOp = "MoveFile" | "OpenBilling" | "SignIn" | "RecheckDns";
```

A closed set of in-app operations an `AdvisoryAction.InApp` can request.
Mirrors the Rust `advisory::AppOp`.

***

### AdvisoryScope

```ts
type AdvisoryScope = "File" | "Config" | "Environment" | "Remote" | "Account";
```

Which axis of the system an advisory is about. Mirrors the Rust
`advisory::Scope` (externally-tagged unit enum → bare string).

***

### AdvisorySeverity

```ts
type AdvisorySeverity = "ShippedDegraded" | "NeedsAction" | "Blocking";
```

How serious the plugin proposes an advisory is. moss CLAMPS this (R13): a
`Blocking` proposal with no actionable affordance is demoted to a quiet
`NeedsAction` hairline dot. Mirrors the Rust `advisory::Severity`.

***

### EscapeSpec

```ts
type EscapeSpec = "cancel" | `resend:${string}` | `recheck:${string}`;
```

Escape kind for `awaiting()` calls. The string variants take a
free-text affordance label after the colon, mirroring the dev harness
dialect (e.g., `"resend:Resend email"`, `"recheck:Recheck DNS"`).
Plain `"cancel"` carries no label.

***

### PluginHook

```ts
type PluginHook = "import" | "publish" | "deploy" | "syndicate" | "process" | "enhance";
```

`PluginHook` mirrors the closed Rust enum in
`src-tauri/src/plugins/types.rs`. The router (T1) cross-products
`PluginHook × TriggerContext` to pick a UI surface for the task.
Plugin authors pick the hook that matches what they're doing; they
do NOT pick the surface (the router owns that).

***

### TriggerContext

```ts
type TriggerContext = "onboarding_flow" | "settings_manual" | "background" | "manual_one";
```

`TriggerContext` mirrors the closed Rust enum. Tells the router *why*
the task was invoked so it can pick a surface that matches the user's
focus context (onboarding cards → ActionPanel; background sync →
Workspace).

***

### reportError()

```ts
function reportError(
   error, 
   context?, 
fatal?): Promise<void>;
```

Report an error to moss

#### Parameters

| Parameter | Type | Default value |
| ------ | ------ | ------ |
| `error` | `string` | `undefined` |
| `context?` | `string` | `undefined` |
| `fatal?` | `boolean` | `false` |

#### Returns

`Promise`\<`void`\>

***

### reportProgress()

```ts
function reportProgress(
   phase, 
   current, 
   total, 
message?): Promise<void>;
```

Report progress to moss

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `phase` | `string` |
| `current` | `number` |
| `total` | `number` |
| `message?` | `string` |

#### Returns

`Promise`\<`void`\>

***

### sendMessage()

```ts
function sendMessage(message): Promise<void>;
```

Send a message to moss

Log and progress messages use events (fire-and-forget) to avoid blocking IPC.
Complete and error messages use commands (request-response) for acknowledgment.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `message` | [`PluginMessage`](#pluginmessage) |

#### Returns

`Promise`\<`void`\>

***

### setMessageContext()

```ts
function setMessageContext(pluginName, hookName): void;
```

Set the message context for subsequent messages
This is typically called automatically by the plugin runtime

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `pluginName` | `string` |
| `hookName` | `string` |

#### Returns

`void`

***

### startTask()

```ts
function startTask(label, options?): Promise<TaskHandle>;
```

Start a plugin task. Returns a `TaskHandle` whose methods drive the
lifecycle (progress → awaiting → succeeded/failed/cancelled).

The hook + trigger pair flows into the Rust-side `route_plugin_task`
router, which picks `(TaskScope, TaskKind, TaskTone)` — i.e., which
UI renderer (Ambient hairline / Inline badge / Narrated titlebar /
Awaiting pulse) surfaces the task. Plugin authors do NOT pick the
surface; they just describe what they're doing and why.

Preferred over `reportProgress()` for new code. The legacy API stays
supported until ADR-015 Phase 3 sweeps all 151 call sites.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `label` | `string` |
| `options` | [`StartTaskOptions`](interfaces/StartTaskOptions.md) |

#### Returns

`Promise`\<[`TaskHandle`](interfaces/TaskHandle.md)\>

#### Example

```ts
const task = await startTask("Importing 42 articles", {
  hook: "import",
  trigger: "onboarding_flow",
});
for (let i = 0; i < articles.length; i++) {
  await task.progress(i / articles.length, `Article ${i + 1}/${articles.length}`);
  await importOne(articles[i]);
}
await task.succeeded(`Imported ${articles.length} articles`);
```

## Hook contexts

| Interface | Description |
| ------ | ------ |
| [ArticleInfo](interfaces/ArticleInfo.md) | Article information for syndication |
| [BaseContext](interfaces/BaseContext.md) | Base context shared by all hooks |
| [ConfigureDomainContext](interfaces/ConfigureDomainContext.md) | Context for configure_domain hook (custom domain setup on deploy platform) |
| [DeployContext](interfaces/DeployContext.md) | Context for on_deploy hook (deployer plugins) |
| [DeploymentInfo](interfaces/DeploymentInfo.md) | Deployment result information |
| [DnsRecord](interfaces/DnsRecord.md) | A single DNS record provided by deploy plugins |
| [DnsTarget](interfaces/DnsTarget.md) | DNS configuration provided by deploy plugins |
| [GenerateContext](interfaces/GenerateContext.md) | Context for on_build hook (generator plugins) |
| [PageNode](interfaces/PageNode.md) | A node in the universal Page Tree. |
| [ProcessContext](interfaces/ProcessContext.md) | Context for before_build hook (process capability) |
| [SourceFiles](interfaces/SourceFiles.md) | Source files categorized by type |
| [SyndicateContext](interfaces/SyndicateContext.md) | Context for after_deploy hook (syndicator plugins) |

## Hooks

| Interface | Description |
| ------ | ------ |
| [HookResult](interfaces/HookResult.md) | Standard result returned from hook execution |
| [HookToast](interfaces/HookToast.md) | Outcome notification described by a hook result. |

## Filesystem

### fileExists()

```ts
function fileExists(relativePath): Promise<boolean>;
```

Check if a file exists in the project directory

Project path is auto-detected from the runtime context.

#### Parameters

| Parameter | Type | Description |
| ------ | ------ | ------ |
| `relativePath` | `string` | Path relative to the project root |

#### Returns

`Promise`\<`boolean`\>

true if file exists, false otherwise

#### Throws

Error if called outside a hook

#### Example

```typescript
if (await fileExists("index.md")) {
  const content = await readFile("index.md");
}
```

***

### listFiles()

```ts
function listFiles(): Promise<string[]>;
```

List all files in the project directory

Returns file paths relative to the project root.
Project path is auto-detected from the runtime context.

#### Returns

`Promise`\<`string`[]\>

Array of relative file paths

#### Throws

Error if directory cannot be listed or called outside a hook

#### Example

```typescript
const files = await listFiles();
// ["index.md", "article/hello.md", "assets/logo.png"]

const mdFiles = files.filter(f => f.endsWith(".md"));
```

***

### listProjectTree()

```ts
function listProjectTree(): Promise<ProjectFileEntry[]>;
```

List all project files with home-file annotations

Each file is annotated with `is_home: true` if it's the detected home file
for its containing folder (index.md, README.md, self-named folder note, etc.).
Detection uses the same logic as the built-in generator.

#### Returns

`Promise`\<[`ProjectFileEntry`](interfaces/ProjectFileEntry.md)[]\>

Array of file entries with is_home annotations

***

### listSiteFilesWithSizes()

```ts
function listSiteFilesWithSizes(): Promise<SiteFileInfo[]>;
```

List all files in the compiled site directory with their sizes

#### Returns

`Promise`\<[`SiteFileInfo`](interfaces/SiteFileInfo.md)[]\>

Array of file info objects with path and size in bytes

***

### readFile()

```ts
function readFile(relativePath): Promise<string>;
```

Read a file from the project directory

Project path is auto-detected from the runtime context.

#### Parameters

| Parameter | Type | Description |
| ------ | ------ | ------ |
| `relativePath` | `string` | Path relative to the project root |

#### Returns

`Promise`\<`string`\>

File contents as a string

#### Throws

Error if file cannot be read or called outside a hook

#### Example

```typescript
// Read an article
const content = await readFile("article/hello-world.md");

// Read package.json
const pkg = JSON.parse(await readFile("package.json"));
```

***

### readSiteFile()

```ts
function readSiteFile(relativePath): Promise<string>;
```

Read a file from the compiled site directory (.moss/site/)

Returns base64-encoded content. Used by deploy plugins to read
site files without direct filesystem access.

#### Parameters

| Parameter | Type | Description |
| ------ | ------ | ------ |
| `relativePath` | `string` | Path relative to the site directory (e.g., "index.html") |

#### Returns

`Promise`\<`string`\>

Base64-encoded file content

#### Throws

Error if file cannot be read

#### Example

```typescript
const base64Content = await readSiteFile("index.html");
const base64Image = await readSiteFile("assets/logo.png");
```

***

### writeFile()

```ts
function writeFile(relativePath, content): Promise<void>;
```

Write content to a file in the project directory

Creates parent directories if they don't exist.
Project path is auto-detected from the runtime context.

#### Parameters

| Parameter | Type | Description |
| ------ | ------ | ------ |
| `relativePath` | `string` | Path relative to the project root |
| `content` | `string` | Content to write to the file |

#### Returns

`Promise`\<`void`\>

#### Throws

Error if file cannot be written or called outside a hook

#### Example

```typescript
// Write a generated article
await writeFile("article/new-post.md", "# Hello World\n\nContent here.");

// Write index page
await writeFile("index.md", markdownContent);
```

## Plugin storage

### pluginFileExists()

```ts
function pluginFileExists(relativePath): Promise<boolean>;
```

Check if a file exists in the plugin's private storage directory

#### Parameters

| Parameter | Type | Description |
| ------ | ------ | ------ |
| `relativePath` | `string` | Path relative to the plugin's storage directory |

#### Returns

`Promise`\<`boolean`\>

true if file exists, false otherwise

#### Throws

Error if called outside a hook

#### Example

```typescript
if (await pluginFileExists("config.json")) {
  const config = JSON.parse(await readPluginFile("config.json"));
} else {
  // Use default config
}
```

***

### readPluginFile()

```ts
function readPluginFile(relativePath): Promise<string>;
```

Read a file from the plugin's private storage directory

Storage path: .moss/plugins/{plugin-name}/{relativePath}

#### Parameters

| Parameter | Type | Description |
| ------ | ------ | ------ |
| `relativePath` | `string` | Path relative to the plugin's storage directory |

#### Returns

`Promise`\<`string`\>

File contents as a string

#### Throws

Error if file cannot be read or called outside a hook

#### Example

```typescript
// Read plugin config
const configJson = await readPluginFile("config.json");
const config = JSON.parse(configJson);

// Read cached data
const cached = await readPluginFile("cache/articles.json");
```

***

### writePluginFile()

```ts
function writePluginFile(relativePath, content): Promise<void>;
```

Write a file to the plugin's private storage directory

Creates parent directories if they don't exist.
Storage path: .moss/plugins/{plugin-name}/{relativePath}

#### Parameters

| Parameter | Type | Description |
| ------ | ------ | ------ |
| `relativePath` | `string` | Path relative to the plugin's storage directory |
| `content` | `string` | Content to write to the file |

#### Returns

`Promise`\<`void`\>

#### Throws

Error if file cannot be written or called outside a hook

#### Example

```typescript
// Save plugin config
await writePluginFile("config.json", JSON.stringify(config, null, 2));

// Cache data
await writePluginFile("cache/articles.json", JSON.stringify(articles));
```

## HTTP

### downloadAsset()

```ts
function downloadAsset(
   url, 
   targetDir, 
options?): Promise<DownloadResult>;
```

Download a URL and save directly to disk

Downloads the file and writes it directly to disk without passing
the binary data through JavaScript. The filename is derived from
the URL, and file extension is inferred from Content-Type if needed.

Project path is auto-detected from the runtime context.

#### Parameters

| Parameter | Type | Description |
| ------ | ------ | ------ |
| `url` | `string` | URL to download |
| `targetDir` | `string` | Target directory within project (e.g., "assets") |
| `options` | [`DownloadOptions`](interfaces/DownloadOptions.md) | Optional download configuration |

#### Returns

`Promise`\<[`DownloadResult`](interfaces/DownloadResult.md)\>

Download result with actual path where file was saved

#### Throws

Error if download or write fails, or called outside a hook

#### Example

```typescript
const result = await downloadAsset(
  "https://example.com/image",
  "assets"
);
if (result.ok) {
  console.log(`Saved to ${result.actualPath}`); // e.g., "assets/image.png"
}
```

***

### fetchUrl()

```ts
function fetchUrl(url, options?): Promise<FetchResult>;
```

Fetch a URL using Rust's HTTP client (bypasses CORS)

#### Parameters

| Parameter | Type | Description |
| ------ | ------ | ------ |
| `url` | `string` | URL to fetch |
| `options` | [`FetchOptions`](interfaces/FetchOptions.md) | Optional fetch configuration |

#### Returns

`Promise`\<[`FetchResult`](interfaces/FetchResult.md)\>

Fetch result with status, body, and helpers

#### Throws

Error if network request fails

#### Example

```typescript
const result = await fetchUrl("https://api.example.com/data");
if (result.ok) {
  const data = JSON.parse(result.text());
}
```

***

### htmlToMarkdown()

```ts
function htmlToMarkdown(html): Promise<string>;
```

Convert an HTML fragment to Markdown via moss's bundled `htmd` converter —
the same converter the rest of the app uses. Plugins call this instead of
shipping their own HTML→Markdown pass, so output (notably hard breaks, which
htmd renders as two trailing spaces rather than a lone backslash) is
consistent app-wide. Returns the input HTML unchanged if conversion fails.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `html` | `string` |

#### Returns

`Promise`\<`string`\>

***

### httpGet()

```ts
function httpGet(url, options?): Promise<FetchResult>;
```

Perform an HTTP GET request

Uses Rust's HTTP client to bypass browser CORS restrictions.
This is useful for API interactions that require custom headers.

#### Parameters

| Parameter | Type | Description |
| ------ | ------ | ------ |
| `url` | `string` | URL to GET |
| `options` | [`GetOptions`](interfaces/GetOptions.md) | Optional configuration including timeout and headers |

#### Returns

`Promise`\<[`FetchResult`](interfaces/FetchResult.md)\>

Fetch result with status, body, and helpers

#### Throws

Error if network request fails

#### Example

```typescript
// Buttondown API newsletter info request
const result = await httpGet(
  "https://api.buttondown.com/v1/newsletters",
  { headers: { Authorization: "Token xxx" } }
);
if (result.ok) {
  const data = JSON.parse(result.text());
}
```

***

### httpPost()

```ts
function httpPost(
   url, 
   body, 
options?): Promise<FetchResult>;
```

Perform an HTTP POST request with JSON body

Uses Rust's HTTP client to bypass browser CORS restrictions.
This is useful for OAuth flows and other API interactions.

#### Parameters

| Parameter | Type | Description |
| ------ | ------ | ------ |
| `url` | `string` | URL to POST to |
| `body` | `Record`\<`string`, `unknown`\> | JSON object to send as the request body |
| `options` | [`PostOptions`](interfaces/PostOptions.md) | Optional configuration including timeout and headers |

#### Returns

`Promise`\<[`FetchResult`](interfaces/FetchResult.md)\>

Fetch result with status, body, and helpers

#### Throws

Error if network request fails

#### Example

```typescript
// GitHub OAuth device code request
const result = await httpPost(
  "https://github.com/login/device/code",
  { client_id: "xxx", scope: "repo workflow" },
  { headers: { Accept: "application/json" } }
);
if (result.ok) {
  const data = JSON.parse(result.text());
}
```

***

### httpPostMultipart()

```ts
function httpPostMultipart(
   url, 
   parts, 
options?): Promise<FetchResult>;
```

Perform an HTTP POST with a `multipart/form-data` body.

Unlike [httpPost](#httppost) (JSON-only), this sends ordered text fields plus
binary file parts — enabling uploads to GraphQL `singleFileUpload`-style
endpoints. File bytes are passed base64-encoded (so they survive the IPC
boundary and can come directly from [readSiteFile](#readsitefile)); moss builds the
multipart body, generates the boundary, and sets the Content-Type.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `url` | `string` |
| `parts` | \{ `files?`: [`MultipartFilePart`](interfaces/MultipartFilePart.md)[]; `textFields?`: [`MultipartTextField`](interfaces/MultipartTextField.md)[]; \} |
| `parts.files?` | [`MultipartFilePart`](interfaces/MultipartFilePart.md)[] |
| `parts.textFields?` | [`MultipartTextField`](interfaces/MultipartTextField.md)[] |
| `options` | [`MultipartPostOptions`](interfaces/MultipartPostOptions.md) |

#### Returns

`Promise`\<[`FetchResult`](interfaces/FetchResult.md)\>

#### Example

```typescript
const res = await httpPostMultipart(endpoint, {
  textFields: [
    { name: "operations", value: JSON.stringify({ query, variables }) },
    { name: "map", value: JSON.stringify({ "0": ["variables.input.file"] }) },
  ],
  files: [{ field: "0", filename: "photo.jpg", contentType: "image/jpeg", contentBase64 }],
}, { headers: { "x-access-token": token } });
```

## Browser

### BrowserCloseReason

```ts
type BrowserCloseReason = 
  | {
  type: "user";
}
  | {
  type: "timeout";
}
  | {
  type: "programmatic";
};
```

Reason why the browser window was closed

***

### closeBrowser()

```ts
function closeBrowser(): Promise<void>;
```

Close the action panel

#### Returns

`Promise`\<`void`\>

***

### openBrowser()

```ts
function openBrowser(url): Promise<BrowserHandle>;
```

Open a URL in the action panel

Returns a BrowserHandle that can be used to detect when the window is closed.

#### Parameters

| Parameter | Type | Description |
| ------ | ------ | ------ |
| `url` | `string` | The URL to open |

#### Returns

`Promise`\<[`BrowserHandle`](interfaces/BrowserHandle.md)\>

BrowserHandle with a `closed` promise

#### Example

```typescript
const browser = await openBrowser("https://example.com/login");

// Wait for user to close window or authentication to complete
const closeReason = await Promise.race([
  browser.closed,
  waitForAuth().then(() => ({ type: "programmatic" as const }))
]);

if (closeReason.type === "user") {
  console.log("User closed the window without completing");
}
```

***

### openBrowserWithHtml()

```ts
function openBrowserWithHtml(html): Promise<void>;
```

Open the action panel with dynamic HTML content

Automatically injects a bridge script that exposes `window.mossApi` with:
- `close()` - closes the browser panel
- `emit(name, payload)` - emits custom events for plugin-specific communication

Uses a custom protocol (moss-plugin://) to serve HTML content
without requiring the `webview-data-url` Cargo feature.

**Manual lifecycle control:**
After calling this function, the browser panel remains open until you explicitly
call `closeBrowser()` or the user closes it. Use `listen()` to handle custom
events emitted from the HTML.

#### Parameters

| Parameter | Type | Description |
| ------ | ------ | ------ |
| `html` | `string` | Raw HTML content to display |

#### Returns

`Promise`\<`void`\>

#### Example

```typescript
import { openBrowserWithHtml, closeBrowser, listen } from "@symbiosis-lab/moss-api";

// Open browser with custom HTML
await openBrowserWithHtml(`
  <!DOCTYPE html>
  <html>
    <head><title>My Form</title></head>
    <body>
      <form id="myForm">
        <input id="nameInput" name="name" />
        <button type="submit">Submit</button>
        <button type="button" onclick="window.mossApi.close()">Cancel</button>
      </form>
      <script>
        document.getElementById('myForm').addEventListener('submit', (e) => {
          e.preventDefault();
          window.mossApi.emit('my-plugin:form-submit', {
            name: document.getElementById('nameInput').value
          });
        });
      </script>
    </body>
  </html>
`);

// Listen for custom event from HTML
const unlisten = await listen('my-plugin:form-submit', (event) => {
  console.log('User submitted:', event.payload);
  closeBrowser(); // Explicitly close when done
});
```

***

### openSystemBrowser()

```ts
function openSystemBrowser(url): Promise<void>;
```

Open a URL in the system's default browser

Useful for OAuth flows where the user may already be logged in
to their browser, providing a better authentication experience.

#### Parameters

| Parameter | Type | Description |
| ------ | ------ | ------ |
| `url` | `string` | The URL to open |

#### Returns

`Promise`\<`void`\>

#### Example

```typescript
// OAuth device flow - user may already be logged in
await openSystemBrowser("https://github.com/login/device");
```

***

### returnToEditor()

```ts
function returnToEditor(): Promise<void>;
```

Ask the app shell to restore the editor in the action panel after a login
flow cancel or failure. Clears the onboarding latch and re-mounts the
editor (empty-folder onboarding cards) in the action panel slot.

Call this after `promptLogin()` returns false on an import (binding /
prompt_login) path so the user isn't left with an empty action panel.

#### Returns

`Promise`\<`void`\>

***

### ~~showBrowserForm()~~

```ts
function showBrowserForm<T>(html, options?): Promise<T | null>;
```

Show an HTML form in the browser panel and wait for the user to submit or cancel.

#### Type Parameters

| Type Parameter |
| ------ |
| `T` |

#### Parameters

| Parameter | Type | Description |
| ------ | ------ | ------ |
| `html` | `string` | Raw HTML content with a form |
| `options?` | \{ `closeDelayMs?`: `number`; `timeoutMs?`: `number`; \} | Optional configuration |
| `options.closeDelayMs?` | `number` | Optional delay before closing browser (default: 0ms / immediate) |
| `options.timeoutMs?` | `number` | Maximum time to wait (default: 300000ms / 5 minutes) |

#### Returns

`Promise`\<`T` \| `null`\>

The submitted form data, or null on cancel/timeout

#### Deprecated

This function couples form lifecycle to moss internals through hidden event listeners.
Use `openBrowserWithHtml()` + manual `closeBrowser()` instead for explicit control.

**Migration guide:**
```typescript
// OLD (deprecated):
const result = await showBrowserForm<LoginData>(html);
if (result) {
  console.log("Submitted:", result);
}

// NEW (recommended):
await openBrowserWithHtml(html);

// Listen for custom event
const unlisten = await listen<LoginData>("my-plugin:submit", (event) => {
  console.log("Submitted:", event.payload);
  closeBrowser();
});

// In your HTML:
// <button onclick="window.mossApi.emit('my-plugin:submit', { username: '...' })">Submit</button>
// <button onclick="window.mossApi.close()">Cancel</button>
```

**Why migrate:**
- Explicit browser lifecycle control (no magic auto-close)
- No hidden event listeners (`moss:browser-form-submit`, `moss:browser-form-cancel`)
- Simpler mental model: open, use, close
- Matches modern plugin patterns (see Matters plugin)

Note: This deprecated function still listens for `moss:browser-form-submit` and
`moss:browser-form-cancel` events for backward compatibility. New code should use
`window.mossApi.emit('your-event', data)` and `window.mossApi.close()` instead.

Returns the submitted data, or `null` if the user cancelled or the timeout expired.
The browser is automatically closed in all cases.

#### Example

```typescript
interface LoginData { username: string; password: string }

const result = await showBrowserForm<LoginData>(`
  <!DOCTYPE html>
  <html>
    <head><title>Login</title></head>
    <body>
      <form id="login">
        <input id="user" placeholder="Username" />
        <input id="pass" type="password" placeholder="Password" />
        <button type="submit">Login</button>
        <button type="button" onclick="window.mossApi.close()">Cancel</button>
      </form>
      <script>
        document.getElementById('login').addEventListener('submit', (e) => {
          e.preventDefault();
          window.mossApi.emit('moss:browser-form-submit', {
            username: document.getElementById('user').value,
            password: document.getElementById('pass').value,
          });
        });
      </script>
    </body>
  </html>
`);

if (result) {
  console.log("User submitted:", result.username);
} else {
  console.log("User cancelled or timed out");
}
```

## Toast

### ~~ToastType~~

```ts
type ToastType = ToastVariant;
```

#### Deprecated

Use ToastVariant instead

***

### ToastVariant

```ts
type ToastVariant = "success" | "error" | "info" | "warning";
```

Toast variant determines the visual style

***

### TOAST\_DISMISS\_EVENT

```ts
const TOAST_DISMISS_EVENT: "show-toast-dismiss" = "show-toast-dismiss";
```

Event for dismissing a toast by ID

***

### TOAST\_EVENT

```ts
const TOAST_EVENT: "show-toast" = "show-toast";
```

Event for showing a new toast

***

### TOAST\_UPDATE\_EVENT

```ts
const TOAST_UPDATE_EVENT: "show-toast-update" = "show-toast-update";
```

Event for updating an existing toast by ID

***

### dismissToast()

```ts
function dismissToast(id): Promise<void>;
```

Dismiss a toast by ID

#### Parameters

| Parameter | Type | Description |
| ------ | ------ | ------ |
| `id` | `string` | The toast ID to dismiss |

#### Returns

`Promise`\<`void`\>

#### Example

```typescript
// Show a toast
await showToast({
  message: "Processing...",
  id: "process-toast",
  persistent: true
});

// Later, dismiss it
await dismissToast("process-toast");
```

***

### showToast()

```ts
function showToast(options): Promise<void>;
```

Show a toast notification in the main moss UI

#### Parameters

| Parameter | Type | Description |
| ------ | ------ | ------ |
| `options` | `string` \| [`ToastOptions`](interfaces/ToastOptions.md) | Toast options or simple message string |

#### Returns

`Promise`\<`void`\>

#### Example

```typescript
// Object form (recommended)
await showToast({
  message: "Deployed!",
  variant: "success",
  actions: [{ label: "View site", url: "https://..." }],
  duration: 8000
});

// Simple form (for quick messages)
await showToast("Processing...");
```

## Events

### emitEvent()

```ts
function emitEvent(event, payload?): Promise<void>;
```

Emit an event to other parts of the application

#### Parameters

| Parameter | Type | Description |
| ------ | ------ | ------ |
| `event` | `string` | Event name (e.g., "repo-created", "dialog-result") |
| `payload?` | `unknown` | Data to send with the event |

#### Returns

`Promise`\<`void`\>

#### Example

```typescript
// From dialog:
await emitEvent("repo-name-validated", { name: "my-repo", available: true });

// From plugin:
await emitEvent("deployment-started", { url: "https://github.com/..." });
```

***

### onEvent()

```ts
function onEvent<T>(event, handler): Promise<() => void>;
```

Listen for events from other parts of the application

#### Type Parameters

| Type Parameter |
| ------ |
| `T` |

#### Parameters

| Parameter | Type | Description |
| ------ | ------ | ------ |
| `event` | `string` | Event name to listen for |
| `handler` | (`payload`) => `void` | Function to call when event is received |

#### Returns

`Promise`\<() => `void`\>

Cleanup function to stop listening

#### Example

```typescript
const unlisten = await onEvent<{ name: string; available: boolean }>(
  "repo-name-validated",
  (data) => {
    console.log(`Repo ${data.name} is ${data.available ? "available" : "taken"}`);
  }
);

// Later, to stop listening:
unlisten();
```

## Binary execution

### executeBinary()

```ts
function executeBinary(options): Promise<ExecuteResult>;
```

Execute an external binary

Working directory is auto-detected from the runtime context
(always the project root).

#### Parameters

| Parameter | Type | Description |
| ------ | ------ | ------ |
| `options` | [`ExecuteOptions`](interfaces/ExecuteOptions.md) | Execution options including binary path and args |

#### Returns

`Promise`\<[`ExecuteResult`](interfaces/ExecuteResult.md)\>

Execution result with stdout, stderr, and exit code

#### Throws

Error if binary cannot be executed or called outside a hook

#### Examples

```typescript
// Run git status
const result = await executeBinary({
  binaryPath: "git",
  args: ["status"],
});

if (result.success) {
  console.log(result.stdout);
} else {
  console.error(result.stderr);
}
```

```typescript
// Run npm install with timeout
const result = await executeBinary({
  binaryPath: "npm",
  args: ["install"],
  timeoutMs: 120000,
  env: { NODE_ENV: "production" },
});
```

## Platform

### ArchType

```ts
type ArchType = "arm64" | "x64";
```

Supported architectures

***

### OSType

```ts
type OSType = "darwin" | "linux" | "windows";
```

Supported operating systems

***

### PlatformKey

```ts
type PlatformKey = "darwin-arm64" | "darwin-x64" | "linux-x64" | "windows-x64";
```

Platform key combining OS and architecture

***

### getPlatformInfo()

```ts
function getPlatformInfo(): Promise<PlatformInfo>;
```

Detect the current platform (OS and architecture)

Uses system commands to detect the platform:
- On macOS/Linux: `uname -s` for OS, `uname -m` for architecture
- On Windows: Falls back to environment variables and defaults

Results are cached after the first call.

#### Returns

`Promise`\<[`PlatformInfo`](interfaces/PlatformInfo.md)\>

Platform information including OS, architecture, and combined key

#### Throws

Error if platform detection fails or platform is unsupported

#### Example

```typescript
const platform = await getPlatformInfo();
console.log(platform.platformKey); // "darwin-arm64"
```

## Cookies

### clearPluginCookies()

```ts
function clearPluginCookies(): Promise<void>;
```

Delete ALL cookies on the current plugin's registered (manifest) domain from
the shared WebKit store. Used for force-fresh login: clears any lingering
server session so the login webview presents a real credential screen.

The plugin's identity is auto-detected from the runtime context.
**Must be called from within a plugin hook.**

#### Returns

`Promise`\<`void`\>

***

### getPluginCookie()

```ts
function getPluginCookie(): Promise<Cookie[] | null>;
```

Get stored cookies for the current plugin.

The plugin's identity is automatically detected from the runtime context.
Cookies are filtered by the domain declared in the plugin's manifest.json.

#### Returns

`Promise`\<[`Cookie`](interfaces/Cookie.md)[] \| `null`\>

Array of cookies for the plugin's registered domain, or `null` if
         called outside of a plugin hook context.

#### Example

```typescript
// Inside a hook function:
const cookies = await getPluginCookie();

// null means no context (e.g., window closed, hook ended)
if (cookies === null) {
  console.log("No plugin context - stopping");
  return;
}

const token = cookies.find(c => c.name === "__access_token");
if (token) {
  // Use token for authenticated requests
}
```

***

### setPluginCookie()

```ts
function setPluginCookie(cookies): Promise<void>;
```

Store cookies for the current plugin.

The plugin's identity is automatically detected from the runtime context.

**Must be called from within a plugin hook** (process, generate, deploy, syndicate).

#### Parameters

| Parameter | Type | Description |
| ------ | ------ | ------ |
| `cookies` | [`Cookie`](interfaces/Cookie.md)[] | Array of cookies to store |

#### Returns

`Promise`\<`void`\>

#### Throws

Error if called outside of a plugin hook execution

#### Example

```typescript
// Inside a hook function:
await setPluginCookie([
  { name: "session", value: "abc123" }
]);
```

## Environment

### getPluginEnvVar()

```ts
function getPluginEnvVar(name): Promise<string | undefined>;
```

Read a host environment variable into the plugin webview.

Returns `undefined` if:
- Tauri is unavailable (running outside the moss webview),
- the variable is not in the server-side allow-list, or
- the variable is not set in the host process.

Plugins should treat the return value as best-effort: a missing value
is the production default, not an error.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `name` | `string` |

#### Returns

`Promise`\<`string` \| `undefined`\>

## Plugin manifest

### PluginCategory

```ts
type PluginCategory = "generator" | "deployer" | "syndicator" | "enhancer" | "processor";
```

## Messages

### PluginMessage

```ts
type PluginMessage = 
  | LogMessage
  | ProgressMessage
  | ErrorMessage
  | CompleteMessage;
```

Messages that plugins can send to moss

## Enhance

### EnhanceContent

```ts
type EnhanceContent = 
  | {
  html: string;
  type: "static";
}
  | {
  pages: Record<string, string>;
  type: "per-page";
};
```

Content declaration for a single slot.

## Social

| Interface | Description |
| ------ | ------ |
| [SocialArticleData](interfaces/SocialArticleData.md) | The social data for a single article — currently its comments. |
| [SocialComment](interfaces/SocialComment.md) | One comment in the .moss/data/social/*.json shared standard. See moss/docs/reference/social-data-standard.md. |
| [SocialDataFile](interfaces/SocialDataFile.md) | A `.moss/data/social/*.json` file: the schema version plus every article's social data, keyed by article path. |

## Keys

### KeyAlgorithm

```ts
type KeyAlgorithm = "ed25519" | "secp256k1-schnorr";
```

A signing algorithm for a key.

- `ed25519` — EdDSA. Signature: raw 64 bytes. Public key: 32 bytes. The right
  choice for IPNS (its `MUST` key type) and most new protocols.
- `secp256k1-schnorr` — BIP-340. Signature: 64 bytes. Public key: x-only 32
  bytes. For Nostr-family protocols.

***

### getKey()

```ts
function getKey(name, algorithm): Promise<KeyInfo>;
```

Get your key named `name`, creating it with `algorithm` the first time.

Idempotent: calling again with the same name returns the same key. The
algorithm is fixed when the key is created — asking for an existing key with a
different algorithm is an error.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `name` | `string` |
| `algorithm` | [`KeyAlgorithm`](#keyalgorithm) |

#### Returns

`Promise`\<[`KeyInfo`](interfaces/KeyInfo.md)\>

***

### listKeys()

```ts
function listKeys(): Promise<KeyInfo[]>;
```

List your keys.

#### Returns

`Promise`\<[`KeyInfo`](interfaces/KeyInfo.md)[]\>

***

### signWithKey()

```ts
function signWithKey(name, payload): Promise<Uint8Array<ArrayBufferLike>>;
```

Sign `payload` with your key named `name`.

The bytes are yours to construct — moss signs exactly what you give it. The
signature is in the key algorithm's standard form (ed25519: raw 64 bytes;
secp256k1-schnorr: BIP-340). Any protocol framing (an IPNS record's
`ipns-signature:` prefix, a Nostr event id) is yours to build before signing.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `name` | `string` |
| `payload` | `Uint8Array` |

#### Returns

`Promise`\<`Uint8Array`\<`ArrayBufferLike`\>\>

## Tauri core (deprecated)

### ~~getTauriCore()~~

```ts
function getTauriCore(): TauriCore;
```

Get the Tauri core API

#### Returns

[`TauriCore`](interfaces/TauriCore.md)

#### Deprecated

Use higher-level APIs instead:
- File operations: `readFile`, `writeFile`, `listFiles`, `fileExists`
- HTTP: `fetchUrl`, `downloadAsset`
- Binary execution: `executeBinary`
- Cookies: `getPluginCookie`, `setPluginCookie`

#### Throws

Error if Tauri is not available
