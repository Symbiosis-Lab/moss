//#region src/utils/tauri.ts
/**
* Get the Tauri core API
*
* @deprecated Use higher-level APIs instead:
* - File operations: `readFile`, `writeFile`, `listFiles`, `fileExists`
* - HTTP: `fetchUrl`, `downloadAsset`
* - Binary execution: `executeBinary`
* - Cookies: `getPluginCookie`, `setPluginCookie`
*
* @throws Error if Tauri is not available
* @category Tauri core (deprecated)
*/
function getTauriCore() {
	const w = window;
	if (!w.__TAURI__?.core) throw new Error("Tauri core not available");
	return w.__TAURI__.core;
}
/**
* Check if Tauri is available
* @category Tauri core (deprecated)
*/
function isTauriAvailable() {
	return !!window.__TAURI__?.core;
}

//#endregion
//#region src/utils/env.ts
/**
* Plugin-side env-var access.
*
* Plugins run inside a webview (no Node `process.env`), so reading host
* environment variables requires a Rust→TS bridge. The
* `get_plugin_env_var` Tauri command in the app's plugin runtime module
* enforces a server-side allow-list — plugins cannot read arbitrary
* environment variables, only the ones moss has whitelisted for test /
* harness use.
*
* Currently allow-listed (see runtime.rs `ALLOWED` constant):
* - `MOSS_MATTERS_TEST_PROFILE` — bypasses Matters auth and switches to
*   public-fetch mode for the named profile (T8a e2e harness).
* - `MOSS_MATTERS_DOMAIN` — overrides the Matters domain (e.g. "matters.icu")
*   so moss-claude.sh can target the test env without pre-seeding config.json.
*
* New entries require an explicit Rust-side edit + code review.
*/
/**
* Read a host environment variable into the plugin webview.
*
* Returns `undefined` if:
* - Tauri is unavailable (running outside the moss webview),
* - the variable is not in the server-side allow-list, or
* - the variable is not set in the host process.
*
* Plugins should treat the return value as best-effort: a missing value
* is the production default, not an error.
* @category Environment
*/
async function getPluginEnvVar(name) {
	if (!isTauriAvailable()) return void 0;
	try {
		return await getTauriCore().invoke("get_plugin_env_var", { name }) ?? void 0;
	} catch (err) {
		console.warn(`[moss-api] getPluginEnvVar(${name}) failed:`, err);
		return;
	}
}

//#endregion
//#region src/utils/events.ts
/**
* Get Tauri event API
* @internal
*/
function getTauriEvent$1() {
	const w = window;
	if (!w.__TAURI__?.event) throw new Error("Tauri event API not available");
	return w.__TAURI__.event;
}
/**
* Check if Tauri event API is available
* @category Events
*/
function isEventApiAvailable() {
	return !!window.__TAURI__?.event;
}
/**
* Emit an event to other parts of the application
*
* @param event - Event name (e.g., "repo-created", "dialog-result")
* @param payload - Data to send with the event
*
* @example
* ```typescript
* // From dialog:
* await emitEvent("repo-name-validated", { name: "my-repo", available: true });
*
* // From plugin:
* await emitEvent("deployment-started", { url: "https://github.com/..." });
* ```
* @category Events
*/
async function emitEvent(event, payload) {
	await getTauriEvent$1().emit(event, payload);
}
/**
* Listen for events from other parts of the application
*
* @param event - Event name to listen for
* @param handler - Function to call when event is received
* @returns Cleanup function to stop listening
*
* @example
* ```typescript
* const unlisten = await onEvent<{ name: string; available: boolean }>(
*   "repo-name-validated",
*   (data) => {
*     console.log(`Repo ${data.name} is ${data.available ? "available" : "taken"}`);
*   }
* );
*
* // Later, to stop listening:
* unlisten();
* ```
* @category Events
*/
async function onEvent(event, handler) {
	return await getTauriEvent$1().listen(event, (e) => {
		handler(e.payload);
	});
}

//#endregion
//#region src/utils/messaging.ts
let currentPluginName = "";
let currentHookName = "";
/**
* Set the message context for subsequent messages
* This is typically called automatically by the plugin runtime
* @category Messaging
*/
function setMessageContext(pluginName, hookName) {
	currentPluginName = pluginName;
	currentHookName = hookName;
}
/**
* Send a message to moss
*
* Log and progress messages use events (fire-and-forget) to avoid blocking IPC.
* Complete and error messages use commands (request-response) for acknowledgment.
* @category Messaging
*/
async function sendMessage(message) {
	if (message.type === "log" || message.type === "progress") {
		if (!isEventApiAvailable()) return;
		try {
			await emitEvent("plugin-message", {
				pluginName: currentPluginName,
				hookName: currentHookName,
				message
			});
		} catch {}
		return;
	}
	if (!isTauriAvailable()) return;
	try {
		await getTauriCore().invoke("plugin_message", {
			pluginName: currentPluginName,
			hookName: currentHookName,
			message
		});
	} catch (error) {
		console.error("❌ [SDK] Failed to send message:", message.type, "–", error);
	}
}
/**
* Report progress to moss
* @category Messaging
*/
async function reportProgress(phase, current, total, message) {
	await sendMessage({
		type: "progress",
		phase,
		current,
		total,
		message
	});
}
/**
* Report an error to moss
* @category Messaging
*/
async function reportError(error, context, fatal = false) {
	await sendMessage({
		type: "error",
		error,
		context,
		fatal
	});
}
/**
* Internal helper for invoking the Rust command. Resolves to the task
* id Rust echoes back; throws if the invoke fails (caller decides how
* to surface it — typically Awaiting / progress events fail-silent so a
* dropped narrator update doesn't crash a plugin).
*
* Task id is `string` end-to-end (u64 in Rust → string in specta) to
* preserve precision above 2^53.
*/
/**
* Sentinel task id returned when running outside a Tauri context
* (unit tests, browser preview of plugin). Subsequent `TaskHandle`
* method calls short-circuit when they see this id so they don't
* issue invokes with a fake taskId that the Rust router would
* reject as "unknown task id".
*
* Exported solely so the corresponding test can assert against it
* by symbol; production code should never compare against this
* literal — use `TaskHandle.id === OUT_OF_TAURI_TASK_ID` is the
* only legitimate check, and the `TaskHandle` methods already
* encapsulate it.
* @category Messaging
*/
const OUT_OF_TAURI_TASK_ID = "-1";
async function invokeLifecycle(pluginName, hook, trigger, taskId, lifecycle) {
	if (!isTauriAvailable()) return OUT_OF_TAURI_TASK_ID;
	const raw = await getTauriCore().invoke("report_plugin_task_lifecycle_command", {
		pluginName,
		hook,
		trigger,
		taskId,
		lifecycle
	});
	return typeof raw === "number" ? String(raw) : raw;
}
/**
* Start a plugin task. Returns a `TaskHandle` whose methods drive the
* lifecycle (progress → awaiting → succeeded/failed/cancelled).
*
* The hook + trigger pair flows into the Rust-side `route_plugin_task`
* router, which picks `(TaskScope, TaskKind, TaskTone)` — i.e., which
* UI renderer (Ambient hairline / Inline badge / Narrated titlebar /
* Awaiting pulse) surfaces the task. Plugin authors do NOT pick the
* surface; they just describe what they're doing and why.
*
* Preferred over `reportProgress()` for new code. The legacy API stays
* supported until a later phase sweeps the remaining call sites.
*
* @example
* const task = await startTask("Importing 42 articles", {
*   hook: "import",
*   trigger: "onboarding_flow",
* });
* for (let i = 0; i < articles.length; i++) {
*   await task.progress(i / articles.length, `Article ${i + 1}/${articles.length}`);
*   await importOne(articles[i]);
* }
* await task.succeeded(`Imported ${articles.length} articles`);
* @category Messaging
*/
async function startTask(label, options = {}) {
	const hook = options.hook ?? "import";
	const trigger = options.trigger ?? "background";
	const hasProgress = options.hasProgress ?? true;
	const cancellable = options.cancellable ?? false;
	const pluginName = currentPluginName;
	const started = {
		type: "started",
		label,
		has_progress: hasProgress,
		cancellable
	};
	if (options.job !== void 0) started.job = options.job;
	const id = await invokeLifecycle(pluginName, hook, trigger, void 0, started);
	if (id === OUT_OF_TAURI_TASK_ID) return {
		id,
		async progress() {},
		async awaiting() {},
		async advise() {},
		async succeeded() {},
		async failed() {},
		async cancelled() {}
	};
	const pendingAdvisories = [];
	let spent = false;
	return {
		id,
		async progress(fraction, message) {
			await invokeLifecycle(pluginName, hook, trigger, id, {
				type: "progress",
				fraction,
				message
			});
		},
		async awaiting(directive, venue, escape = "cancel") {
			await invokeLifecycle(pluginName, hook, trigger, id, {
				type: "awaiting",
				directive: venue ? `${directive} in ${venue}` : directive,
				escape
			});
		},
		async advise(advisory) {
			if (spent) return;
			pendingAdvisories.push(advisory);
		},
		async succeeded(receipt, amount) {
			spent = true;
			const lifecycle = {
				type: "succeeded",
				receipt
			};
			if (pendingAdvisories.length > 0) lifecycle.advisories = pendingAdvisories.slice();
			if (amount !== void 0) lifecycle.amount = amount;
			await invokeLifecycle(pluginName, hook, trigger, id, lifecycle);
		},
		async failed(error, recoverable = false) {
			spent = true;
			const lifecycle = {
				type: "failed",
				error,
				recoverable
			};
			if (pendingAdvisories.length > 0) lifecycle.advisories = pendingAdvisories.slice();
			await invokeLifecycle(pluginName, hook, trigger, id, lifecycle);
		},
		async cancelled() {
			spent = true;
			await invokeLifecycle(pluginName, hook, trigger, id, { type: "cancelled" });
		}
	};
}

//#endregion
//#region src/utils/browser.ts
/**
* Browser utilities for plugins
* Abstracts Tauri browser commands to decouple plugins from internal APIs
*/
/**
* Get Tauri event API for listening to and emitting events
* @internal
*/
function getTauriEvent() {
	const w = window;
	if (!w.__TAURI__?.event?.listen) throw new Error("Tauri event API not available");
	return w.__TAURI__.event;
}
/**
* Bridge script that exposes `window.mossApi` in browser panel HTML.
* This decouples plugin HTML from Tauri internals.
*
* Provides explicit control over browser lifecycle:
* - `close()` - Closes the browser panel
* - `emit(name, payload)` - Emits custom events for plugin-specific communication
*
* @internal
*/
const BROWSER_BRIDGE_SCRIPT = `<script>
(function() {
  const { event, core } = window.__TAURI__;
  window.mossApi = {
    close: () => core.invoke('close_action_panel'),
    emit: (name, payload) => event.emit(name, payload),
  };
})();
<\/script>`;
/**
* Inject the bridge script into HTML content.
* If HTML contains </head>, inject before it. Otherwise, prepend.
* @internal
*/
function injectBridgeScript(html) {
	const headCloseIdx = html.indexOf("</head>");
	if (headCloseIdx !== -1) return html.slice(0, headCloseIdx) + BROWSER_BRIDGE_SCRIPT + html.slice(headCloseIdx);
	return BROWSER_BRIDGE_SCRIPT + html;
}
/**
* Open a URL in the action panel
*
* Returns a BrowserHandle that can be used to detect when the window is closed.
*
* @param url - The URL to open
* @returns BrowserHandle with a `closed` promise
*
* @example
* ```typescript
* const browser = await openBrowser("https://example.com/login");
*
* // Wait for user to close window or authentication to complete
* const closeReason = await Promise.race([
*   browser.closed,
*   waitForAuth().then(() => ({ type: "programmatic" as const }))
* ]);
*
* if (closeReason.type === "user") {
*   console.log("User closed the window without completing");
* }
* ```
* @category Browser
*/
async function openBrowser(url) {
	await getTauriCore().invoke("open_action_panel", {
		url,
		belowTitlebar: true
	});
	const closed = new Promise((resolve) => {
		const { listen } = getTauriEvent();
		listen("browser-closed", (event) => {
			const payload = event.payload;
			resolve(payload.reason);
		}).then((unlisten) => {
			closed.then(() => unlisten());
		});
	});
	return { closed };
}
/**
* Close the action panel
* @category Browser
*/
async function closeBrowser() {
	await getTauriCore().invoke("close_action_panel", {});
}
/**
* Ask the app shell to restore the editor in the action panel after a login
* flow cancel or failure. Clears the onboarding latch and re-mounts the
* editor (empty-folder onboarding cards) in the action panel slot.
*
* Call this after `promptLogin()` returns false on an import (binding /
* prompt_login) path so the user isn't left with an empty action panel.
* @category Browser
*/
async function returnToEditor() {
	await getTauriCore().invoke("return_to_editor", {});
}
/**
* Open a URL in the system's default browser
*
* Useful for OAuth flows where the user may already be logged in
* to their browser, providing a better authentication experience.
*
* @param url - The URL to open
* @example
* ```typescript
* // OAuth device flow - user may already be logged in
* await openSystemBrowser("https://github.com/login/device");
* ```
* @category Browser
*/
async function openSystemBrowser(url) {
	await getTauriCore().invoke("open_system_browser", { url });
}
/**
* Open the action panel with dynamic HTML content
*
* Automatically injects a bridge script that exposes `window.mossApi` with:
* - `close()` - closes the browser panel
* - `emit(name, payload)` - emits custom events for plugin-specific communication
*
* Uses a custom protocol (moss-plugin://) to serve HTML content
* without requiring the `webview-data-url` Cargo feature.
*
* **Manual lifecycle control:**
* After calling this function, the browser panel remains open until you explicitly
* call `closeBrowser()` or the user closes it. Use `listen()` to handle custom
* events emitted from the HTML.
*
* @param html - Raw HTML content to display
* @example
* ```typescript
* import { openBrowserWithHtml, closeBrowser, listen } from "@symbiosis-lab/moss-api";
*
* // Open browser with custom HTML
* await openBrowserWithHtml(`
*   <!DOCTYPE html>
*   <html>
*     <head><title>My Form</title></head>
*     <body>
*       <form id="myForm">
*         <input id="nameInput" name="name" />
*         <button type="submit">Submit</button>
*         <button type="button" onclick="window.mossApi.close()">Cancel</button>
*       </form>
*       <script>
*         document.getElementById('myForm').addEventListener('submit', (e) => {
*           e.preventDefault();
*           window.mossApi.emit('my-plugin:form-submit', {
*             name: document.getElementById('nameInput').value
*           });
*         });
*       <\/script>
*     </body>
*   </html>
* `);
*
* // Listen for custom event from HTML
* const unlisten = await listen('my-plugin:form-submit', (event) => {
*   console.log('User submitted:', event.payload);
*   closeBrowser(); // Explicitly close when done
* });
* ```
* @category Browser
*/
async function openBrowserWithHtml(html) {
	const injectedHtml = injectBridgeScript(html);
	await getTauriCore().invoke("set_action_panel_html", { html: injectedHtml });
}
/**
* Show an HTML form in the browser panel and wait for the user to submit or cancel.
*
* @deprecated This function couples form lifecycle to moss internals through hidden event listeners.
* Use `openBrowserWithHtml()` + manual `closeBrowser()` instead for explicit control.
*
* **Migration guide:**
* ```typescript
* // OLD (deprecated):
* const result = await showBrowserForm<LoginData>(html);
* if (result) {
*   console.log("Submitted:", result);
* }
*
* // NEW (recommended):
* await openBrowserWithHtml(html);
*
* // Listen for custom event
* const unlisten = await listen<LoginData>("my-plugin:submit", (event) => {
*   console.log("Submitted:", event.payload);
*   closeBrowser();
* });
*
* // In your HTML:
* // <button onclick="window.mossApi.emit('my-plugin:submit', { username: '...' })">Submit</button>
* // <button onclick="window.mossApi.close()">Cancel</button>
* ```
*
* **Why migrate:**
* - Explicit browser lifecycle control (no magic auto-close)
* - No hidden event listeners (`moss:browser-form-submit`, `moss:browser-form-cancel`)
* - Simpler mental model: open, use, close
* - Matches modern plugin patterns (see Matters plugin)
*
* Note: This deprecated function still listens for `moss:browser-form-submit` and
* `moss:browser-form-cancel` events for backward compatibility. New code should use
* `window.mossApi.emit('your-event', data)` and `window.mossApi.close()` instead.
*
* Returns the submitted data, or `null` if the user cancelled or the timeout expired.
* The browser is automatically closed in all cases.
*
* @param html - Raw HTML content with a form
* @param options - Optional configuration
* @param options.timeoutMs - Maximum time to wait (default: 300000ms / 5 minutes)
* @param options.closeDelayMs - Optional delay before closing browser (default: 0ms / immediate)
* @returns The submitted form data, or null on cancel/timeout
*
* @example
* ```typescript
* interface LoginData { username: string; password: string }
*
* const result = await showBrowserForm<LoginData>(`
*   <!DOCTYPE html>
*   <html>
*     <head><title>Login</title></head>
*     <body>
*       <form id="login">
*         <input id="user" placeholder="Username" />
*         <input id="pass" type="password" placeholder="Password" />
*         <button type="submit">Login</button>
*         <button type="button" onclick="window.mossApi.close()">Cancel</button>
*       </form>
*       <script>
*         document.getElementById('login').addEventListener('submit', (e) => {
*           e.preventDefault();
*           window.mossApi.emit('moss:browser-form-submit', {
*             username: document.getElementById('user').value,
*             password: document.getElementById('pass').value,
*           });
*         });
*       <\/script>
*     </body>
*   </html>
* `);
*
* if (result) {
*   console.log("User submitted:", result.username);
* } else {
*   console.log("User cancelled or timed out");
* }
* ```
* @category Browser
*/
async function showBrowserForm(html, options) {
	const timeoutMs = options?.timeoutMs ?? 3e5;
	const closeDelayMs = options?.closeDelayMs ?? 0;
	await openBrowserWithHtml(html);
	const tauriEvent = getTauriEvent();
	let settled = false;
	let resolveResult;
	const resultPromise = new Promise((r) => {
		resolveResult = r;
	});
	const timer = setTimeout(() => settle(null), timeoutMs);
	const unlistenSubmit = await tauriEvent.listen("moss:browser-form-submit", (e) => settle(e.payload));
	const unlistenCancel = await tauriEvent.listen("moss:browser-form-cancel", () => settle(null));
	function settle(value) {
		if (settled) return;
		settled = true;
		clearTimeout(timer);
		unlistenSubmit();
		unlistenCancel();
		if (closeDelayMs > 0) setTimeout(() => {
			closeBrowser();
			resolveResult(value);
		}, closeDelayMs);
		else {
			closeBrowser();
			resolveResult(value);
		}
	}
	return resultPromise;
}

//#endregion
//#region src/utils/context.ts
/**
* Get the internal plugin context
*
* This is used internally by moss-api utilities to resolve paths
* and plugin identity. Plugins should not call this directly.
*
* @returns The current plugin execution context
* @throws Error if called outside of a plugin hook execution
*
* @internal
*/
function getInternalContext() {
	const context = window.__MOSS_INTERNAL_CONTEXT__;
	if (!context) throw new Error("This function must be called from within a plugin hook. Ensure you're calling this from process(), generate(), deploy(), or syndicate().");
	return context;
}
/**
* Check if we're currently inside a plugin hook execution
*
* @returns true if inside a hook, false otherwise
*
* @internal
*/
function hasContext() {
	return window.__MOSS_INTERNAL_CONTEXT__ !== void 0;
}

//#endregion
//#region src/utils/filesystem.ts
/**
* File system operations for moss plugins
*
* These functions provide access to project files (user content).
* Project path is auto-detected from the runtime context.
*
* For plugin's private storage, use the plugin-storage API instead.
*/
/**
* Read a file from the project directory
*
* Project path is auto-detected from the runtime context.
*
* @param relativePath - Path relative to the project root
* @returns File contents as a string
* @throws Error if file cannot be read or called outside a hook
*
* @example
* ```typescript
* // Read an article
* const content = await readFile("article/hello-world.md");
*
* // Read package.json
* const pkg = JSON.parse(await readFile("package.json"));
* ```
* @category Filesystem
*/
async function readFile(relativePath) {
	const ctx = getInternalContext();
	return getTauriCore().invoke("read_project_file", {
		projectPath: ctx.project_path,
		pluginName: ctx.plugin_name,
		relativePath
	});
}
/**
* Write content to a file in the project directory
*
* Creates parent directories if they don't exist.
* Project path is auto-detected from the runtime context.
*
* @param relativePath - Path relative to the project root
* @param content - Content to write to the file
* @throws Error if file cannot be written or called outside a hook
*
* @example
* ```typescript
* // Write a generated article
* await writeFile("article/new-post.md", "# Hello World\n\nContent here.");
*
* // Write index page
* await writeFile("index.md", markdownContent);
* ```
* @category Filesystem
*/
async function writeFile(relativePath, content) {
	const ctx = getInternalContext();
	await getTauriCore().invoke("write_project_file", {
		projectPath: ctx.project_path,
		pluginName: ctx.plugin_name,
		relativePath,
		data: content
	});
}
/**
* List all files in the project directory
*
* Returns file paths relative to the project root.
* Project path is auto-detected from the runtime context.
*
* @returns Array of relative file paths
* @throws Error if directory cannot be listed or called outside a hook
*
* @example
* ```typescript
* const files = await listFiles();
* // ["index.md", "article/hello.md", "assets/logo.png"]
*
* const mdFiles = files.filter(f => f.endsWith(".md"));
* ```
* @category Filesystem
*/
async function listFiles() {
	const ctx = getInternalContext();
	return getTauriCore().invoke("list_project_files", { projectPath: ctx.project_path });
}
/**
* List all project files with home-file annotations
*
* Each file is annotated with `is_home: true` if it's the detected home file
* for its containing folder (index.md, README.md, self-named folder note, etc.).
* Detection uses the same logic as the built-in generator.
*
* @returns Array of file entries with is_home annotations
* @category Filesystem
*/
async function listProjectTree() {
	return getTauriCore().invoke("list_project_tree", {});
}
/**
* Check if a file exists in the project directory
*
* Project path is auto-detected from the runtime context.
*
* @param relativePath - Path relative to the project root
* @returns true if file exists, false otherwise
* @throws Error if called outside a hook
*
* @example
* ```typescript
* if (await fileExists("index.md")) {
*   const content = await readFile("index.md");
* }
* ```
* @category Filesystem
*/
async function fileExists(relativePath) {
	getInternalContext();
	try {
		await readFile(relativePath);
		return true;
	} catch {
		return false;
	}
}
/**
* Read a file from the compiled site directory (.moss/site/)
*
* Returns base64-encoded content. Used by deploy plugins to read
* site files without direct filesystem access.
*
* @param relativePath - Path relative to the site directory (e.g., "index.html")
* @returns Base64-encoded file content
* @throws Error if file cannot be read
*
* @example
* ```typescript
* const base64Content = await readSiteFile("index.html");
* const base64Image = await readSiteFile("assets/logo.png");
* ```
* @category Filesystem
*/
async function readSiteFile(relativePath) {
	return getTauriCore().invoke("read_site_file", { relativePath });
}
/**
* List all files in the compiled site directory with their sizes
*
* @returns Array of file info objects with path and size in bytes
* @category Filesystem
*/
async function listSiteFilesWithSizes() {
	return getTauriCore().invoke("list_site_files_with_sizes", {});
}

//#endregion
//#region src/utils/keystore.ts
/**
* Keys and secrets — the two things moss holds for you.
*
* A **key** you never see: you ask for it by name and moss signs with it. A
* **secret** you do see, because it is a token somebody else issued the user
* and you have to put it in a header. Both are scoped to your plugin
* automatically, both live outside the user's repo, and moss is the custodian
* of both. That is why they share a file.
*
* moss is a keystore. You ask for a key by name, sign with it, and list your
* keys — you never receive private bytes. moss holds them; you use them. This is
* the same arrangement as a hardware wallet or a browser's non-extractable
* `CryptoKey`, and for the same reason: your key stays usable and stays yours,
* but a compromised build of your plugin cannot walk away with it.
*
* Keys are **yours** — scoped to your plugin automatically. You do not pass an
* id, and you cannot name another plugin's key; two plugins that both call
* `getKey("ipns")` get two different keys. There is nothing to declare in your
* manifest: creating and using your own key spends nothing of anyone else's, so
* it needs no permission.
*
* Why moss holds the bytes rather than handing them to you: a key is the durable
* identity behind a name you publish (an IPNS name *is* its public key and
* cannot be rotated). Left in your plugin's folder it would be committed to the
* user's repo and pushed. moss keeps it out of git and lets the user back it up;
* you keep full use of it.
*
* **You never draw a credential input.** moss asks the user for the token, in
* moss's own modal, driven by the `setup.credentials` block in your manifest. A
* plugin that draws its own password field is teaching users to type credentials
* into whatever asks, which is the habit that makes phishing work — so the
* registry refuses it. What you may do is *store what an authenticated flow
* already returned to you*: that is `setSecret`. It refuses any key you declared
* in `setup.credentials` or as a `config_schema` field of type `secret`, since
* those are the ones the user typed and moss holds for them.
*
* The loop is git's fill → approve/reject, and the reject half is the one
* plugins forget: a revoked token fails every publish identically until someone
* says so. `rejectSecret` says so AND asks again, resolving with the
* replacement, so your error path is catch → reject → retry rather than a
* failed publish and an explanation.
*
* ```ts
* let jwt = await moss.getSecret("pinata_jwt");
* let res = await moss.fetch(url, { headers: { Authorization: `Bearer ${jwt}` } });
* if (res.status === 401) {
*   jwt = await moss.rejectSecret("pinata_jwt", { detail: "Pinata rejected this token." });
*   if (!jwt) throw new Error("Publishing needs a Pinata token.");
*   res = await moss.fetch(url, { headers: { Authorization: `Bearer ${jwt}` } });
* }
* ```
*
* @category Keys
*/
function toBase64(bytes) {
	let binary = "";
	for (const b of bytes) binary += String.fromCharCode(b);
	return btoa(binary);
}
function fromBase64(b64) {
	const binary = atob(b64);
	const out = new Uint8Array(binary.length);
	for (let i = 0; i < binary.length; i++) out[i] = binary.charCodeAt(i);
	return out;
}
function decode(w) {
	return {
		name: w.name,
		algorithm: w.algorithm,
		publicKey: fromBase64(w.publicKeyBase64)
	};
}
/**
* Get your key named `name`, creating it with `algorithm` the first time.
*
* Idempotent: calling again with the same name returns the same key. The
* algorithm is fixed when the key is created — asking for an existing key with a
* different algorithm is an error.
*
* @category Keys
*/
async function getKey(name, algorithm) {
	return decode(await getTauriCore().invoke("key_get_or_create", {
		name,
		algorithm
	}));
}
/**
* List your keys.
* @category Keys
*/
async function listKeys() {
	return (await getTauriCore().invoke("key_list", {})).map(decode);
}
/**
* Sign `payload` with your key named `name`.
*
* The bytes are yours to construct — moss signs exactly what you give it. The
* signature is in the key algorithm's standard form (ed25519: raw 64 bytes;
* secp256k1-schnorr: BIP-340). Any protocol framing (an IPNS record's
* `ipns-signature:` prefix, a Nostr event id) is yours to build before signing.
*
* @category Keys
*/
async function signWithKey(name, payload) {
	return fromBase64((await getTauriCore().invoke("key_sign", {
		name,
		payloadBase64: toBase64(payload)
	})).signatureBase64);
}
/**
* Your secret stored under `key`, or `null` if there is none.
*
* `null` is the normal first-run answer, not an error. If your manifest
* declares the key under `setup.credentials`, moss has already asked for it
* before your hook ran, so `null` here means the user cancelled.
*
* @category Secrets
*/
async function getSecret(key) {
	return await getTauriCore().invoke("get_plugin_secret", { key });
}
/**
* Tell moss the secret stored under `key` does not work — and get a replacement.
*
* Rejecting forgets the stored value and re-draws moss's own credential modal,
* carrying your `detail` sentence as the reason — one sentence saying why you
* are asking again, rendered above moss's own field. You supply the words and
* no pixels. It resolves with the new value, or `null` if the user cancelled.
* So the error path is a retry, not a failed publish with an explanation:
*
* ```ts
* const fresh = await moss.rejectSecret("pinata_jwt", {
*   detail: "Pinata says this token is no longer valid.",
* });
* if (fresh === null) return; // the user declined; stop, don't loop
* ```
*
* Call this when the service rejects the credential itself — a 401, a revoked
* token — not when a request merely failed. Rejecting on a network error throws
* away a perfectly good token AND interrupts the user for nothing.
*
* The wait is the user's, so it is unbounded: moss's inactivity watchdog counts
* an open credential modal as progress, not as a hung hook. With no window to
* draw in — a headless `moss build` — the value is forgotten and `null` comes
* back immediately, which is the honest answer when nobody can be asked.
*
* @category Secrets
*/
async function rejectSecret(key, options) {
	return await getTauriCore().invoke("reject_plugin_secret", {
		key,
		detail: options?.detail ?? null
	});
}
/**
* Store what an authenticated flow returned to you.
*
* **You may never draw the input yourself.** This is for a token you already
* hold because a login moss supervised produced it — an OAuth redirect, a
* session exchange. If what you want is to *ask* the user for a credential,
* declare it in your manifest's `setup.credentials` and moss will draw the
* field; a plugin-drawn password box is a registry blocker, not a style choice.
*
* Refused for any key your manifest declared as one moss asks the user for — a
* `setup.credentials` entry, or a `config_schema` field of type `secret`. Those
* slots hold what a person typed into moss's modal, and a plugin quietly
* replacing one would leave the user believing their own token is still there.
* If you need such a credential replaced, call {@link rejectSecret}: moss
* forgets it and asks again, and you get the new value back. Every other key in
* your scope is yours to write. The key is scoped to your plugin automatically
* — you cannot write another plugin's secret, the same way you cannot read one.
*
* An empty `value` erases the key — that is how you sign a user out, and moss
* then reports nothing stored for the slot. Earlier releases stored the empty
* string literally, so a signed-out account went on showing as connected.
*
* Why here and not in your own plugin folder: `.moss/plugins/` is inside the
* user's repo and is not gitignored, so a token you keep yourself is a token
* that gets committed and pushed. This is the same custody argument moss already
* makes for signing keys.
*
* @category Secrets
*/
async function setSecret(key, value) {
	await getTauriCore().invoke("set_plugin_secret", {
		key,
		value
	});
}

//#endregion
//#region src/utils/plugin-storage.ts
/**
* Plugin storage API for moss plugins
*
* Provides access to a plugin's private storage directory at:
* .moss/plugins/{plugin-name}/
*
* Plugin identity is auto-detected from the runtime context -
* plugins never need to know their own name or path.
*
* Config is just a file: readPluginFile("config.json")
*/
/**
* Read a file from the plugin's private storage directory
*
* Storage path: .moss/plugins/{plugin-name}/{relativePath}
*
* @param relativePath - Path relative to the plugin's storage directory
* @returns File contents as a string
* @throws Error if file cannot be read or called outside a hook
*
* @example
* ```typescript
* // Read plugin config
* const configJson = await readPluginFile("config.json");
* const config = JSON.parse(configJson);
*
* // Read cached data
* const cached = await readPluginFile("cache/articles.json");
* ```
* @category Plugin storage
*/
async function readPluginFile(relativePath) {
	const ctx = getInternalContext();
	return getTauriCore().invoke("read_plugin_file", {
		pluginName: ctx.plugin_name,
		projectPath: ctx.project_path,
		relativePath
	});
}
/**
* Write a file to the plugin's private storage directory
*
* Creates parent directories if they don't exist.
* Storage path: .moss/plugins/{plugin-name}/{relativePath}
*
* @param relativePath - Path relative to the plugin's storage directory
* @param content - Content to write to the file
* @throws Error if file cannot be written or called outside a hook
*
* @example
* ```typescript
* // Save plugin config
* await writePluginFile("config.json", JSON.stringify(config, null, 2));
*
* // Cache data
* await writePluginFile("cache/articles.json", JSON.stringify(articles));
* ```
* @category Plugin storage
*/
async function writePluginFile(relativePath, content) {
	const ctx = getInternalContext();
	await getTauriCore().invoke("write_plugin_file", {
		pluginName: ctx.plugin_name,
		projectPath: ctx.project_path,
		relativePath,
		content
	});
}
/**
* Check if a file exists in the plugin's private storage directory
*
* @param relativePath - Path relative to the plugin's storage directory
* @returns true if file exists, false otherwise
* @throws Error if called outside a hook
*
* @example
* ```typescript
* if (await pluginFileExists("config.json")) {
*   const config = JSON.parse(await readPluginFile("config.json"));
* } else {
*   // Use default config
* }
* ```
* @category Plugin storage
*/
async function pluginFileExists(relativePath) {
	const ctx = getInternalContext();
	return getTauriCore().invoke("plugin_file_exists", {
		pluginName: ctx.plugin_name,
		projectPath: ctx.project_path,
		relativePath
	});
}

//#endregion
//#region src/utils/http.ts
/**
* HTTP operations for moss plugins
*
* These functions provide HTTP capabilities that bypass browser CORS
* restrictions by using Rust's HTTP client under the hood.
*
* Project path for downloads is auto-detected from the runtime context.
*/
/**
* Fetch a URL using Rust's HTTP client (bypasses CORS)
*
* @param url - URL to fetch
* @param options - Optional fetch configuration
* @returns Fetch result with status, body, and helpers
* @throws Error if network request fails
*
* @example
* ```typescript
* const result = await fetchUrl("https://api.example.com/data");
* if (result.ok) {
*   const data = JSON.parse(result.text());
* }
* ```
* @category HTTP
*/
async function fetchUrl(url, options = {}) {
	const { timeoutMs = 3e4 } = options;
	const result = await getTauriCore().invoke("fetch_url", {
		url,
		timeoutMs
	});
	const binaryString = atob(result.body_base64);
	const bytes = Uint8Array.from(binaryString, (char) => char.charCodeAt(0));
	return {
		status: result.status,
		ok: result.ok,
		contentType: result.content_type,
		body: bytes,
		text() {
			return new TextDecoder().decode(bytes);
		}
	};
}
/**
* Perform an HTTP POST request with JSON body
*
* Uses Rust's HTTP client to bypass browser CORS restrictions.
* This is useful for OAuth flows and other API interactions.
*
* @param url - URL to POST to
* @param body - JSON object to send as the request body
* @param options - Optional configuration including timeout and headers
* @returns Fetch result with status, body, and helpers
* @throws Error if network request fails
*
* @example
* ```typescript
* // GitHub OAuth device code request
* const result = await httpPost(
*   "https://github.com/login/device/code",
*   { client_id: "xxx", scope: "repo workflow" },
*   { headers: { Accept: "application/json" } }
* );
* if (result.ok) {
*   const data = JSON.parse(result.text());
* }
* ```
* @category HTTP
*/
async function httpPost(url, body, options = {}) {
	const { timeoutMs = 3e4, headers = {} } = options;
	const result = await getTauriCore().invoke("http_post", {
		url,
		body: JSON.stringify(body),
		headers,
		timeoutMs
	});
	const binaryString = atob(result.body_base64);
	const bytes = Uint8Array.from(binaryString, (char) => char.charCodeAt(0));
	return {
		status: result.status,
		ok: result.ok,
		contentType: result.content_type,
		body: bytes,
		text() {
			return new TextDecoder().decode(bytes);
		}
	};
}
/**
* Convert an HTML fragment to Markdown via moss's bundled `htmd` converter —
* the same converter the rest of the app uses. Plugins call this instead of
* shipping their own HTML→Markdown pass, so output (notably hard breaks, which
* htmd renders as two trailing spaces rather than a lone backslash) is
* consistent app-wide. Returns the input HTML unchanged if conversion fails.
* @category HTTP
*/
async function htmlToMarkdown(html) {
	return getTauriCore().invoke("html_to_markdown", { html });
}
/**
* Perform an HTTP GET request
*
* Uses Rust's HTTP client to bypass browser CORS restrictions.
* This is useful for API interactions that require custom headers.
*
* @param url - URL to GET
* @param options - Optional configuration including timeout and headers
* @returns Fetch result with status, body, and helpers
* @throws Error if network request fails
*
* @example
* ```typescript
* // Buttondown API newsletter info request
* const result = await httpGet(
*   "https://api.buttondown.com/v1/newsletters",
*   { headers: { Authorization: "Token xxx" } }
* );
* if (result.ok) {
*   const data = JSON.parse(result.text());
* }
* ```
* @category HTTP
*/
async function httpGet(url, options = {}) {
	const { timeoutMs = 3e4, headers = {} } = options;
	const result = await getTauriCore().invoke("http_get", {
		url,
		headers,
		timeoutMs
	});
	const binaryString = atob(result.body_base64);
	const bytes = Uint8Array.from(binaryString, (char) => char.charCodeAt(0));
	return {
		status: result.status,
		ok: result.ok,
		contentType: result.content_type,
		body: bytes,
		text() {
			return new TextDecoder().decode(bytes);
		}
	};
}
/**
* Perform an HTTP POST with a `multipart/form-data` body.
*
* Unlike {@link httpPost} (JSON-only), this sends ordered text fields plus
* binary file parts — enabling uploads to GraphQL `singleFileUpload`-style
* endpoints. File bytes are passed base64-encoded (so they survive the IPC
* boundary and can come directly from {@link readSiteFile}); moss builds the
* multipart body, generates the boundary, and sets the Content-Type.
*
* @example
* ```typescript
* const res = await httpPostMultipart(endpoint, {
*   textFields: [
*     { name: "operations", value: JSON.stringify({ query, variables }) },
*     { name: "map", value: JSON.stringify({ "0": ["variables.input.file"] }) },
*   ],
*   files: [{ field: "0", filename: "photo.jpg", contentType: "image/jpeg", contentBase64 }],
* }, { headers: { "x-access-token": token } });
* ```
* @category HTTP
*/
async function httpPostMultipart(url, parts, options = {}) {
	const { timeoutMs = 3e4, headers = {} } = options;
	const result = await getTauriCore().invoke("http_post_multipart", {
		url,
		textFields: parts.textFields ?? [],
		files: parts.files ?? [],
		headers,
		timeoutMs
	});
	const binaryString = atob(result.body_base64);
	const bytes = Uint8Array.from(binaryString, (char) => char.charCodeAt(0));
	return {
		status: result.status,
		ok: result.ok,
		contentType: result.content_type,
		body: bytes,
		text() {
			return new TextDecoder().decode(bytes);
		}
	};
}
/**
* Download a URL and save directly to disk
*
* Downloads the file and writes it directly to disk without passing
* the binary data through JavaScript. The filename is derived from
* the URL, and file extension is inferred from Content-Type if needed.
*
* Project path is auto-detected from the runtime context.
*
* @param url - URL to download
* @param targetDir - Target directory within project (e.g., "assets")
* @param options - Optional download configuration
* @returns Download result with actual path where file was saved
* @throws Error if download or write fails, or called outside a hook
*
* @example
* ```typescript
* const result = await downloadAsset(
*   "https://example.com/image",
*   "assets"
* );
* if (result.ok) {
*   console.log(`Saved to ${result.actualPath}`); // e.g., "assets/image.png"
* }
* ```
* @category HTTP
*/
async function downloadAsset(url, targetDir, options = {}) {
	const ctx = getInternalContext();
	const { timeoutMs = 3e4 } = options;
	const result = await getTauriCore().invoke("download_asset", {
		url,
		projectPath: ctx.project_path,
		targetDir,
		timeoutMs
	});
	return {
		status: result.status,
		ok: result.ok,
		contentType: result.content_type,
		bytesWritten: result.bytes_written,
		actualPath: result.actual_path
	};
}

//#endregion
//#region src/utils/binary.ts
/**
* Binary execution for moss plugins
*
* Allows plugins to execute external binaries (git, npm, etc.)
* in a controlled environment.
*
* Working directory is auto-detected from the runtime context
* (always the project root).
*/
/**
* Execute an external binary
*
* Working directory is auto-detected from the runtime context
* (always the project root).
*
* @param options - Execution options including binary path and args
* @returns Execution result with stdout, stderr, and exit code
* @throws Error if binary cannot be executed or called outside a hook
*
* @example
* ```typescript
* // Run git status
* const result = await executeBinary({
*   binaryPath: "git",
*   args: ["status"],
* });
*
* if (result.success) {
*   console.log(result.stdout);
* } else {
*   console.error(result.stderr);
* }
* ```
*
* @example
* ```typescript
* // Run npm install with timeout
* const result = await executeBinary({
*   binaryPath: "npm",
*   args: ["install"],
*   timeoutMs: 120000,
*   env: { NODE_ENV: "production" },
* });
* ```
*
* @category Binary execution
*/
async function executeBinary(options) {
	const ctx = getInternalContext();
	const { binaryPath, args, timeoutMs = 6e4, env, stdin, workingDir, onStderr } = options;
	const resolvedWorkingDir = workingDir ? `${ctx.project_path}/${workingDir}` : ctx.project_path;
	const streamId = onStderr ? crypto.randomUUID() : void 0;
	let unlisten;
	if (onStderr && streamId) unlisten = await onEvent("binary-output", (payload) => {
		if (payload.streamId === streamId) onStderr(payload.line);
	});
	try {
		const result = await getTauriCore().invoke("execute_binary", {
			binaryPath,
			args,
			workingDir: resolvedWorkingDir,
			timeoutMs,
			env,
			stdinData: stdin,
			streamId
		});
		return {
			success: result.success,
			exitCode: result.exit_code,
			stdout: result.stdout,
			stderr: result.stderr
		};
	} finally {
		if (unlisten) unlisten();
	}
}

//#endregion
//#region src/utils/platform.ts
/**
* Platform detection utilities for moss plugins
*
* Detects the current operating system and architecture to enable
* platform-specific binary downloads and operations.
*/
let cachedPlatform = null;
/**
* Detect the current platform (OS and architecture)
*
* Uses system commands to detect the platform:
* - On macOS/Linux: `uname -s` for OS, `uname -m` for architecture
* - On Windows: Falls back to environment variables and defaults
*
* Results are cached after the first call.
*
* @returns Platform information including OS, architecture, and combined key
* @throws Error if platform detection fails or platform is unsupported
*
* @example
* ```typescript
* const platform = await getPlatformInfo();
* console.log(platform.platformKey); // "darwin-arm64"
* ```
* @category Platform
*/
async function getPlatformInfo() {
	if (cachedPlatform) return cachedPlatform;
	const os = await detectOS();
	const arch = await detectArch(os);
	const platformKey = `${os}-${arch}`;
	const supportedPlatforms = [
		"darwin-arm64",
		"darwin-x64",
		"linux-x64",
		"windows-x64"
	];
	if (!supportedPlatforms.includes(platformKey)) throw new Error(`Unsupported platform: ${platformKey}. Supported platforms: ${supportedPlatforms.join(", ")}`);
	cachedPlatform = {
		os,
		arch,
		platformKey
	};
	return cachedPlatform;
}
/**
* Detect the operating system
*/
async function detectOS() {
	try {
		const result = await executeBinary({
			binaryPath: "uname",
			args: ["-s"],
			timeoutMs: 5e3
		});
		if (result.success) {
			const osName = result.stdout.trim().toLowerCase();
			if (osName === "darwin") return "darwin";
			if (osName === "linux") return "linux";
		}
	} catch {}
	try {
		const result = await executeBinary({
			binaryPath: "cmd",
			args: ["/c", "ver"],
			timeoutMs: 5e3
		});
		if (result.success && result.stdout.toLowerCase().includes("windows")) return "windows";
	} catch {}
	throw new Error("Unable to detect operating system. Supported systems: macOS (Darwin), Linux, Windows");
}
/**
* Detect the CPU architecture
*/
async function detectArch(os) {
	if (os === "windows") try {
		const result = await executeBinary({
			binaryPath: "cmd",
			args: [
				"/c",
				"echo",
				"%PROCESSOR_ARCHITECTURE%"
			],
			timeoutMs: 5e3
		});
		if (result.success) {
			if (result.stdout.trim().toLowerCase() === "arm64") return "arm64";
			return "x64";
		}
	} catch {
		return "x64";
	}
	try {
		const result = await executeBinary({
			binaryPath: "uname",
			args: ["-m"],
			timeoutMs: 5e3
		});
		if (result.success) {
			const machine = result.stdout.trim().toLowerCase();
			if (machine === "arm64" || machine === "aarch64") return "arm64";
			if (machine === "x86_64" || machine === "amd64") return "x64";
			if (machine.includes("arm")) return "arm64";
			return "x64";
		}
	} catch {}
	return "x64";
}

//#endregion
//#region src/utils/cookies.ts
/**
* Cookie management for moss plugins
*
* Allows plugins to store and retrieve authentication cookies
* for external services (e.g., Matters.town, GitHub).
*
* Cookies are automatically scoped to the plugin's registered domain
* (defined in manifest.json) - plugins cannot access other plugins' cookies.
*/
/**
* Get stored cookies for the current plugin.
*
* The plugin's identity is automatically detected from the runtime context.
* Cookies are filtered by the domain declared in the plugin's manifest.json.
*
* @returns Array of cookies for the plugin's registered domain, or `null` if
*          called outside of a plugin hook context.
*
* @example
* ```typescript
* // Inside a hook function:
* const cookies = await getPluginCookie();
*
* // null means no context (e.g., window closed, hook ended)
* if (cookies === null) {
*   console.log("No plugin context - stopping");
*   return;
* }
*
* const token = cookies.find(c => c.name === "__access_token");
* if (token) {
*   // Use token for authenticated requests
* }
* ```
* @category Cookies
*/
async function getPluginCookie() {
	if (!hasContext()) return null;
	const ctx = getInternalContext();
	return getTauriCore().invoke("get_plugin_cookie", {
		pluginName: ctx.plugin_name,
		projectPath: ctx.project_path
	});
}
/**
* Store cookies for the current plugin.
*
* The plugin's identity is automatically detected from the runtime context.
*
* **Must be called from within a plugin hook** (process, generate, deploy, syndicate).
*
* @param cookies - Array of cookies to store
* @throws Error if called outside of a plugin hook execution
*
* @example
* ```typescript
* // Inside a hook function:
* await setPluginCookie([
*   { name: "session", value: "abc123" }
* ]);
* ```
* @category Cookies
*/
async function setPluginCookie(cookies) {
	const ctx = getInternalContext();
	await getTauriCore().invoke("set_plugin_cookie", {
		pluginName: ctx.plugin_name,
		projectPath: ctx.project_path,
		cookies
	});
}
/**
* Delete ALL cookies on the current plugin's registered (manifest) domain from
* the shared WebKit store. Used for force-fresh login: clears any lingering
* server session so the login webview presents a real credential screen.
*
* The plugin's identity is auto-detected from the runtime context.
* **Must be called from within a plugin hook.**
*
* @category Cookies
*/
async function clearPluginCookies() {
	const ctx = getInternalContext();
	await getTauriCore().invoke("clear_plugin_cookies", {
		pluginName: ctx.plugin_name,
		projectPath: ctx.project_path
	});
}

//#endregion
//#region src/utils/toast.ts
/**
* Toast notification utilities for plugins
*
* Allows plugins to display toast notifications in the main moss UI
* through Tauri's event system.
*
* ## Design Principles
*
* 1. **Plugin Full Control**: Plugins specify exactly what appears in toast
* 2. **Minimal Assumptions**: moss just renders what plugin says
* 3. **Direct Path**: Plugin → showToast() → Frontend renders
* 4. **Separation of Concerns**: Toast (UX) is separate from HookResult (flow control)
*/
/**
* Event for showing a new toast
* @category Toast
*/
const TOAST_EVENT = "show-toast";
/**
* Event for updating an existing toast by ID
* @category Toast
*/
const TOAST_UPDATE_EVENT = "show-toast-update";
/**
* Event for dismissing a toast by ID
* @category Toast
*/
const TOAST_DISMISS_EVENT = "show-toast-dismiss";
/**
* Show a toast notification in the main moss UI
*
* @param options - Toast options or simple message string
*
* @example
* ```typescript
* // Object form (recommended)
* await showToast({
*   message: "Deployed!",
*   variant: "success",
*   actions: [{ label: "View site", url: "https://..." }],
*   duration: 8000
* });
*
* // Simple form (for quick messages)
* await showToast("Processing...");
* ```
* @category Toast
*/
async function showToast(options) {
	await emitEvent(TOAST_EVENT, typeof options === "string" ? { message: options } : options);
}
/**
* Dismiss a toast by ID
*
* @param id - The toast ID to dismiss
*
* @example
* ```typescript
* // Show a toast
* await showToast({
*   message: "Processing...",
*   id: "process-toast",
*   persistent: true
* });
*
* // Later, dismiss it
* await dismissToast("process-toast");
* ```
* @category Toast
*/
async function dismissToast(id) {
	await emitEvent(TOAST_DISMISS_EVENT, { id });
}

//#endregion
export { TOAST_DISMISS_EVENT, TOAST_EVENT, TOAST_UPDATE_EVENT, clearPluginCookies, closeBrowser, dismissToast, downloadAsset, emitEvent, executeBinary, fetchUrl, fileExists, getKey, getPlatformInfo, getPluginCookie, getPluginEnvVar, getSecret, getTauriCore, htmlToMarkdown, httpGet, httpPost, httpPostMultipart, listFiles, listKeys, listProjectTree, listSiteFilesWithSizes, onEvent, openBrowser, openBrowserWithHtml, openSystemBrowser, pluginFileExists, readFile, readPluginFile, readSiteFile, rejectSecret, reportError, reportProgress, returnToEditor, sendMessage, setMessageContext, setPluginCookie, setSecret, showBrowserForm, showToast, signWithKey, startTask, writeFile, writePluginFile };