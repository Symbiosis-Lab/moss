//#region src/testing/mock-moss-fence.ts
/**
* Mock-side mirror of the `.moss/` plugin sandbox fence.
*
* Mirrors the real guard: `PluginPath::sandboxed` in
* open/crates/moss-build/src/vault/fs.rs, and its one documented exception,
* `PluginPath::shared_social_data`,
* which is bound to the calling plugin's own `<plugin_id>.json` file — not
* the whole shared directory. Before this mirror existed, mock-tauri.ts's
* `write_project_file` / `read_project_file` handlers never enforced the
* `.moss/` fence at all — so a plugin's own test suite kept passing for two
* months after the real command started refusing the Matters plugin's
* `.moss/data/social/matters.json` write, because "green" here proved
* nothing about the real guard. Kept in its own module rather than inline in
* mock-tauri.ts so that file's size doesn't grow for a self-contained policy
* mirror.
*
* The example cases this rule must get right — a plugin's own file, a
* sibling's, first-party `review.json`, the legacy directory, traversal —
* are not hand-duplicated here: both this module's own test and the Rust
* `PluginPath::shared_social_data` test read the same fixture table,
* `open/fixtures/social-data-fence-cases.json`, so the two policies cannot
* silently drift the way the mock and the real guard once did.
*/
/** Whether the first meaningful segment of a project-relative path is `.moss`. */
function isMossInternalPath(relativePath) {
	const first = relativePath.replace(/\\/g, "/").split("/").find((seg) => seg !== "" && seg !== ".");
	return first !== void 0 && first.toLowerCase() === ".moss";
}
/**
* File stems under `.moss/data/social/` that name a first-party writer, not
* a plugin — mirrors `RESERVED_SOCIAL_DATA_IDS` in vault/fs.rs. A plugin
* manifest simply naming itself "review" would otherwise pass the
* cross-plugin check trivially (the id and the file it claims genuinely
* agree), so this is checked before the filename comparison.
*/
const RESERVED_SOCIAL_DATA_IDS = ["review"];
/**
* The one documented exception to the `.moss/` fence: the calling plugin's
* own file in the shared social-data standard's canonical directory
* (`.moss/data/social/<pluginId>.json`) or its legacy home
* (`.moss/social/<pluginId>.json`, plus that directory's `.migrated-bak`
* archive copy) — never another plugin's file, and never first-party data
* such as `review.json`.
*/
function isOwnSharedSocialDataPath(pluginId, relativePath) {
	if (!pluginId) return false;
	if (RESERVED_SOCIAL_DATA_IDS.some((reserved) => pluginId.toLowerCase() === reserved)) return false;
	const segments = relativePath.replace(/\\/g, "/").split("/").filter((seg) => seg !== "" && seg !== ".");
	const ownCanonicalFile = `${pluginId}.json`;
	const ownLegacyArchiveFile = `${pluginId}.json.migrated-bak`;
	if (segments.length === 4 && segments[1] === "data" && segments[2] === "social") return segments[0].toLowerCase() === ".moss" && segments[3] === ownCanonicalFile;
	if (segments.length === 3 && segments[1] === "social") return segments[0].toLowerCase() === ".moss" && (segments[2] === ownCanonicalFile || segments[2] === ownLegacyArchiveFile);
	return false;
}
const MOSS_FENCE_MESSAGE = "Access to .moss/ is not allowed. Use plugin storage (readPluginFile / writePluginFile) or readSiteFile instead.";
/** Throws the same message the real Rust guard returns, unless the path is
* the calling plugin's own shared-social-data file. */
function enforceMossFence(pluginId, relativePath) {
	if (isMossInternalPath(relativePath) && !isOwnSharedSocialDataPath(pluginId ?? null, relativePath)) throw new Error(MOSS_FENCE_MESSAGE);
}

//#endregion
//#region src/testing/mock-tauri.ts
/**
* Tauri IPC mocking utilities for testing moss plugins
*
* Provides in-memory implementations of Tauri IPC commands that plugins use
* through moss-api. This enables integration testing without a running Tauri app.
*
* @example
* ```typescript
* import { setupMockTauri } from "@symbiosis-lab/moss-api/testing";
*
* describe("my plugin", () => {
*   let ctx: MockTauriContext;
*
*   beforeEach(() => {
*     ctx = setupMockTauri();
*   });
*
*   afterEach(() => {
*     ctx.cleanup();
*   });
*
*   it("reads files", async () => {
*     ctx.filesystem.setFile("/test/project/test.md", "# Hello");
*     const content = await readFile("test.md");
*     expect(content).toBe("# Hello");
*   });
* });
* ```
*/
function createMockFilesystem() {
	const files = /* @__PURE__ */ new Map();
	return {
		files,
		getFile(path) {
			return files.get(path);
		},
		setFile(path, content) {
			const now = /* @__PURE__ */ new Date();
			const existing = files.get(path);
			files.set(path, {
				content,
				createdAt: existing?.createdAt ?? now,
				modifiedAt: now
			});
		},
		deleteFile(path) {
			return files.delete(path);
		},
		listFiles(pattern) {
			const allPaths = Array.from(files.keys());
			if (!pattern) return allPaths;
			const regex = /* @__PURE__ */ new RegExp("^" + pattern.replace(/\*/g, ".*").replace(/\?/g, ".") + "$");
			return allPaths.filter((p) => regex.test(p));
		},
		clear() {
			files.clear();
		}
	};
}
function createDownloadTracker() {
	let activeDownloads = 0;
	let maxConcurrent = 0;
	const completedDownloads = [];
	const failedDownloads = [];
	return {
		get activeDownloads() {
			return activeDownloads;
		},
		get maxConcurrent() {
			return maxConcurrent;
		},
		get completedDownloads() {
			return completedDownloads;
		},
		get failedDownloads() {
			return failedDownloads;
		},
		startDownload(url) {
			activeDownloads++;
			if (activeDownloads > maxConcurrent) maxConcurrent = activeDownloads;
		},
		endDownload(url, success, error) {
			activeDownloads--;
			if (success) completedDownloads.push(url);
			else failedDownloads.push({
				url,
				error: error || "Unknown error"
			});
		},
		reset() {
			activeDownloads = 0;
			maxConcurrent = 0;
			completedDownloads.length = 0;
			failedDownloads.length = 0;
		}
	};
}
function createMockUrlConfig() {
	const responses = /* @__PURE__ */ new Map();
	const callCounts = /* @__PURE__ */ new Map();
	const defaultResponse = {
		status: 200,
		ok: true,
		contentType: "image/png",
		bodyBase64: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==",
		bytesWritten: 68,
		actualPath: "assets/image.png"
	};
	return {
		responses,
		defaultResponse,
		setResponse(url, response) {
			responses.set(url, response);
			callCounts.set(url, 0);
		},
		getResponse(url) {
			const config = responses.get(url);
			if (!config) return defaultResponse;
			if (Array.isArray(config)) {
				const count = callCounts.get(url) || 0;
				callCounts.set(url, count + 1);
				return config[Math.min(count, config.length - 1)];
			}
			return config;
		},
		reset() {
			responses.clear();
			callCounts.clear();
		}
	};
}
function createMockBinaryConfig() {
	const results = /* @__PURE__ */ new Map();
	const defaultResult = {
		success: true,
		exitCode: 0,
		stdout: "",
		stderr: ""
	};
	return {
		results,
		defaultResult,
		setResult(key, result) {
			results.set(key, result);
		},
		getResult(binaryPath, args) {
			const exactKey = `${binaryPath} ${args.join(" ")}`.trim();
			if (results.has(exactKey)) return results.get(exactKey);
			if (results.has(binaryPath)) return results.get(binaryPath);
			return defaultResult;
		},
		reset() {
			results.clear();
		}
	};
}
function createMockCookieStorage() {
	const cookies = /* @__PURE__ */ new Map();
	return {
		cookies,
		getCookies(pluginName, projectPath) {
			const key = `${pluginName}:${projectPath}`;
			return cookies.get(key) || [];
		},
		setCookies(pluginName, projectPath, newCookies) {
			const key = `${pluginName}:${projectPath}`;
			cookies.set(key, newCookies);
		},
		clearCookies(pluginName, projectPath) {
			cookies.delete(`${pluginName}:${projectPath}`);
		},
		clear() {
			cookies.clear();
		}
	};
}
function createMockSecretStorage() {
	const secrets = /* @__PURE__ */ new Map();
	const rejected = /* @__PURE__ */ new Set();
	let nextPromptAnswer = null;
	const at = (plugin, key) => `${plugin}:${key}`;
	return {
		seed: (plugin, key, value) => void (value === "" ? secrets.delete(at(plugin, key)) : secrets.set(at(plugin, key), value)),
		get: (plugin, key) => secrets.get(at(plugin, key)) ?? null,
		wasRejected: (plugin, key) => rejected.has(at(plugin, key)),
		answerNextPrompt: (value) => void (nextPromptAnswer = value),
		reject: (plugin, key) => {
			rejected.add(at(plugin, key));
			const answer = nextPromptAnswer;
			nextPromptAnswer = null;
			if (answer === null) secrets.delete(at(plugin, key));
			else secrets.set(at(plugin, key), answer);
			return answer;
		},
		clear: () => {
			secrets.clear();
			rejected.clear();
		}
	};
}
function createMockBrowserTracker() {
	const openedUrls = [];
	const systemBrowserUrls = [];
	const htmlContent = [];
	let closeCount = 0;
	let isOpen = false;
	return {
		get openedUrls() {
			return openedUrls;
		},
		get systemBrowserUrls() {
			return systemBrowserUrls;
		},
		get htmlContent() {
			return htmlContent;
		},
		get closeCount() {
			return closeCount;
		},
		set closeCount(value) {
			closeCount = value;
		},
		get isOpen() {
			return isOpen;
		},
		set isOpen(value) {
			isOpen = value;
		},
		reset() {
			openedUrls.length = 0;
			systemBrowserUrls.length = 0;
			htmlContent.length = 0;
			closeCount = 0;
			isOpen = false;
		}
	};
}
function createMockDialogTracker() {
	const shownDialogs = [];
	const submittedResults = /* @__PURE__ */ new Map();
	let nextResult = null;
	return {
		get shownDialogs() {
			return shownDialogs;
		},
		get nextResult() {
			return nextResult;
		},
		set nextResult(value) {
			nextResult = value;
		},
		get submittedResults() {
			return submittedResults;
		},
		setNextResult(result) {
			nextResult = result;
		},
		submitResult(dialogId, value) {
			submittedResults.set(dialogId, {
				type: "submitted",
				value
			});
		},
		cancelDialog(dialogId) {
			submittedResults.set(dialogId, { type: "cancelled" });
		},
		reset() {
			shownDialogs.length = 0;
			submittedResults.clear();
			nextResult = null;
		}
	};
}
/**
* Extract filename from URL (mimics Rust backend behavior)
*/
function extractFilenameFromUrl(url) {
	try {
		const segments = new URL(url).pathname.split("/").filter((s) => s.length > 0);
		for (const segment of segments) if (/^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/i.test(segment)) return `${segment}.png`;
		const lastSegment = segments[segments.length - 1] || "image";
		return lastSegment.includes(".") ? lastSegment : `${lastSegment}.png`;
	} catch {
		return "image.png";
	}
}
/**
* Set up mock Tauri IPC for testing
*
* This sets up `window.__TAURI__.core.invoke` to intercept all IPC calls
* and route them to in-memory implementations. It also sets up
* `__MOSS_INTERNAL_CONTEXT__` for the context-aware APIs.
*
* @param options - Optional configuration for project path and plugin name
* @returns Context with mock utilities and cleanup function
*
* @example
* ```typescript
* const ctx = setupMockTauri({ projectPath: "/my/project", pluginName: "my-plugin" });
*
* // Set up test data
* ctx.filesystem.setFile("/my/project/article.md", "# Test");
* ctx.urlConfig.setResponse("https://example.com/image.png", {
*   status: 200,
*   ok: true,
*   contentType: "image/png",
*   bytesWritten: 1024,
* });
*
* // Run your plugin code...
*
* // Verify results
* expect(ctx.downloadTracker.completedDownloads).toHaveLength(1);
*
* // Cleanup
* ctx.cleanup();
* ```
*/
function setupMockTauri(options) {
	const projectPath = options?.projectPath ?? "/test/project";
	const pluginName = options?.pluginName ?? "test-plugin";
	const filesystem = createMockFilesystem();
	const downloadTracker = createDownloadTracker();
	const urlConfig = createMockUrlConfig();
	const binaryConfig = createMockBinaryConfig();
	const cookieStorage = createMockCookieStorage();
	const secretStorage = createMockSecretStorage();
	const browserTracker = createMockBrowserTracker();
	const dialogTracker = createMockDialogTracker();
	const eventListeners = /* @__PURE__ */ new Map();
	/** Register a listener; returns an unlisten function */
	const listen = async (event, handler) => {
		if (!eventListeners.has(event)) eventListeners.set(event, /* @__PURE__ */ new Set());
		eventListeners.get(event).add(handler);
		return () => {
			eventListeners.get(event)?.delete(handler);
		};
	};
	/** Emit an event to all registered listeners */
	const emit = async (event, payload) => {
		const handlers = eventListeners.get(event);
		if (handlers) for (const handler of handlers) handler({ payload });
	};
	/** Internal helper: emit to listeners synchronously (used by invoke handlers) */
	const emitToListeners = (event, payload) => {
		const handlers = eventListeners.get(event);
		if (handlers) for (const handler of handlers) handler({ payload });
	};
	const invoke = async (cmd, args) => {
		const payload = args;
		switch (cmd) {
			case "read_project_file": {
				const projectPath$1 = payload?.projectPath;
				const relativePath = payload?.relativePath;
				enforceMossFence(payload?.pluginName ?? null, relativePath);
				const fullPath = `${projectPath$1}/${relativePath}`;
				const file = filesystem.getFile(fullPath);
				if (file) return file.content;
				throw new Error(`File not found: ${fullPath}`);
			}
			case "write_project_file": {
				const projectPath$1 = payload?.projectPath;
				const relativePath = payload?.relativePath;
				const content = payload?.data;
				enforceMossFence(payload?.pluginName ?? null, relativePath);
				const fullPath = `${projectPath$1}/${relativePath}`;
				filesystem.setFile(fullPath, content);
				return null;
			}
			case "list_project_files": {
				const projectPath$1 = payload?.projectPath;
				return filesystem.listFiles().filter((p) => p.startsWith(projectPath$1 + "/")).map((p) => p.substring(projectPath$1.length + 1));
			}
			case "list_project_tree": {
				const allPaths = filesystem.listFiles();
				const prefix = projectPath + "/";
				const relPaths = allPaths.filter((p) => p.startsWith(prefix)).map((p) => p.substring(prefix.length));
				const folderFiles = /* @__PURE__ */ new Map();
				for (const p of relPaths) {
					const lastSlash = p.lastIndexOf("/");
					const parent = lastSlash >= 0 ? p.substring(0, lastSlash) : "";
					const filename = lastSlash >= 0 ? p.substring(lastSlash + 1) : p;
					if (!folderFiles.has(parent)) folderFiles.set(parent, []);
					folderFiles.get(parent).push(filename);
				}
				const INDEX_STEMS = [
					"index",
					"readme",
					"_index",
					"main"
				];
				const homePaths = /* @__PURE__ */ new Set();
				for (const [folder, files] of folderFiles) {
					let winner = null;
					for (const stem of INDEX_STEMS) {
						const found = files.find((f) => f.toLowerCase().replace(/\.[^.]+$/, "") === stem);
						if (found) {
							winner = found;
							break;
						}
					}
					if (!winner) {
						const folderName = folder.includes("/") ? folder.substring(folder.lastIndexOf("/") + 1) : folder;
						if (folderName) {
							const found = files.find((f) => f.toLowerCase().replace(/\.[^.]+$/, "") === folderName.toLowerCase());
							if (found) winner = found;
						}
					}
					if (winner) homePaths.add(folder ? `${folder}/${winner}` : winner);
				}
				return relPaths.map((p) => ({
					path: p,
					is_home: homePaths.has(p)
				}));
			}
			case "project_file_exists": {
				const fullPath = `${payload?.projectPath}/${payload?.relativePath}`;
				return filesystem.getFile(fullPath) !== void 0;
			}
			case "read_plugin_file": {
				const pn = payload?.pluginName;
				const fullPath = `${payload?.projectPath}/.moss/plugins/${pn}/${payload?.relativePath}`;
				const file = filesystem.getFile(fullPath);
				if (file) return file.content;
				throw new Error(`Plugin file not found: ${fullPath}`);
			}
			case "write_plugin_file": {
				const pn = payload?.pluginName;
				const pp = payload?.projectPath;
				const rp = payload?.relativePath;
				const content = payload?.content;
				const fullPath = `${pp}/.moss/plugins/${pn}/${rp}`;
				filesystem.setFile(fullPath, content);
				return null;
			}
			case "plugin_file_exists": {
				const pn = payload?.pluginName;
				const fullPath = `${payload?.projectPath}/.moss/plugins/${pn}/${payload?.relativePath}`;
				return filesystem.getFile(fullPath) !== void 0;
			}
			case "html_to_markdown": return payload?.html ?? "";
			case "fetch_url": {
				const url = payload?.url;
				const response = urlConfig.getResponse(url);
				if (response.delay) return new Promise((resolve) => setTimeout(() => resolve({
					status: response.status,
					ok: response.ok,
					body_base64: response.bodyBase64 || "",
					content_type: response.contentType || null
				}), response.delay));
				return {
					status: response.status,
					ok: response.ok,
					body_base64: response.bodyBase64 || "",
					content_type: response.contentType || null
				};
			}
			case "http_post": {
				const url = payload?.url;
				const response = urlConfig.getResponse(url);
				if (response.delay) return new Promise((resolve) => setTimeout(() => resolve({
					status: response.status,
					ok: response.ok,
					body_base64: response.bodyBase64 || "",
					content_type: response.contentType || null
				}), response.delay));
				return {
					status: response.status,
					ok: response.ok,
					body_base64: response.bodyBase64 || "",
					content_type: response.contentType || null
				};
			}
			case "download_asset": {
				const url = payload?.url;
				const targetDir = payload?.targetDir;
				const response = urlConfig.getResponse(url);
				downloadTracker.startDownload(url);
				if (response.status === 0) {
					downloadTracker.endDownload(url, false, "Network error");
					throw new Error("Network timeout");
				}
				const actualPath = response.actualPath || `${targetDir}/${extractFilenameFromUrl(url)}`;
				const result = {
					status: response.status,
					ok: response.ok,
					content_type: response.contentType || null,
					bytes_written: response.bytesWritten || 0,
					actual_path: actualPath
				};
				if (response.delay) return new Promise((resolve) => setTimeout(() => {
					downloadTracker.endDownload(url, response.ok);
					resolve(result);
				}, response.delay));
				downloadTracker.endDownload(url, response.ok);
				return result;
			}
			case "get_plugin_cookie": {
				const pluginName$1 = payload?.pluginName;
				const projectPath$1 = payload?.projectPath;
				return cookieStorage.getCookies(pluginName$1, projectPath$1);
			}
			case "set_plugin_cookie": {
				const pluginName$1 = payload?.pluginName;
				const projectPath$1 = payload?.projectPath;
				const cookies = payload?.cookies;
				if (!cookies || cookies.length === 0) throw new Error("set_plugin_cookie was called with no cookies, so it would store nothing. To remove stored cookies, call clearPluginCookies() instead.");
				cookieStorage.setCookies(pluginName$1, projectPath$1, cookies);
				return null;
			}
			case "clear_plugin_cookies": {
				const pluginName$1 = payload?.pluginName;
				const projectPath$1 = payload?.projectPath;
				cookieStorage.clearCookies(pluginName$1, projectPath$1);
				return null;
			}
			case "set_action_panel_html": {
				const html = payload?.html;
				browserTracker.htmlContent.push(html);
				browserTracker.isOpen = true;
				return null;
			}
			case "open_action_panel": {
				const url = payload?.url;
				browserTracker.openedUrls.push(url);
				browserTracker.isOpen = true;
				return null;
			}
			case "close_action_panel":
				browserTracker.closeCount++;
				browserTracker.isOpen = false;
				return null;
			case "open_system_browser": {
				const url = payload?.url;
				browserTracker.systemBrowserUrls.push(url);
				return null;
			}
			case "execute_binary": {
				const binaryPath = payload?.binaryPath;
				const binaryArgs = payload?.args;
				const streamId = payload?.streamId;
				const result = binaryConfig.getResult(binaryPath, binaryArgs);
				if (streamId && result.stderr) {
					const lines = result.stderr.split("\n").filter((l) => l.length > 0);
					for (const line of lines) emitToListeners("binary-output", {
						streamId,
						line
					});
				}
				return {
					success: result.success,
					exit_code: result.exitCode,
					stdout: result.stdout,
					stderr: result.stderr
				};
			}
			case "get_plugin_secret": return secretStorage.get(pluginName, payload?.key);
			case "reject_plugin_secret": return secretStorage.reject(pluginName, payload?.key);
			case "set_plugin_secret":
				secretStorage.seed(pluginName, payload?.key, payload?.value);
				return null;
			case "plugin_message": return null;
			default:
				console.warn(`Unhandled IPC command: ${cmd}`);
				return null;
		}
	};
	if (typeof globalThis.window === "undefined") globalThis.window = {};
	const win = globalThis.window;
	win.__TAURI__ = {
		core: { invoke },
		event: {
			listen,
			emit
		}
	};
	win.__MOSS_INTERNAL_CONTEXT__ = {
		plugin_name: pluginName,
		project_path: projectPath,
		moss_dir: `${projectPath}/.moss`
	};
	return {
		filesystem,
		downloadTracker,
		urlConfig,
		binaryConfig,
		cookieStorage,
		secretStorage,
		browserTracker,
		dialogTracker,
		projectPath,
		pluginName,
		cleanup: () => {
			delete win.__TAURI__;
			delete win.__MOSS_INTERNAL_CONTEXT__;
			filesystem.clear();
			downloadTracker.reset();
			urlConfig.reset();
			binaryConfig.reset();
			cookieStorage.clear();
			secretStorage.clear();
			browserTracker.reset();
			dialogTracker.reset();
			eventListeners.clear();
		}
	};
}

//#endregion
export { createDownloadTracker, createMockBinaryConfig, createMockBrowserTracker, createMockCookieStorage, createMockDialogTracker, createMockFilesystem, createMockSecretStorage, createMockUrlConfig, enforceMossFence, setupMockTauri };