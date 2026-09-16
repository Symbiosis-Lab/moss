/**
 * Re-export all utilities
 */

// Tauri core (deprecated - use higher-level APIs instead)
export { getTauriCore, type TauriCore } from "./tauri.js";

// Plugin-side env-var access (allow-listed via Rust)
export { getPluginEnvVar } from "./env.js";

// Messaging
export {
  setMessageContext,
  sendMessage,
  reportProgress,
  reportError,
  // PanelTask lifecycle (ADR-015 Phase 2 — T8a). Preferred over
  // reportProgress for new code; legacy API stays supported.
  startTask,
  type StartTaskOptions,
  type TaskHandle,
  type PluginHook,
  type TriggerContext,
  type EscapeSpec,
  // Plugin advisory path (Step 3 Phase 5, §8 + R13). A plugin PROPOSES an
  // advisory via `handle.advise(...)`; moss holds the severity gavel.
  type AdvisoryProposal,
  type AdvisoryScope,
  type AdvisorySeverity,
  type AdvisoryAppOp,
  type AdvisoryAction,
} from "./messaging.js";

// Browser
export {
  openBrowser,
  closeBrowser,
  returnToEditor,
  openSystemBrowser,
  openBrowserWithHtml,
  showBrowserForm,
  type BrowserHandle,
  type BrowserCloseReason,
} from "./browser.js";

// File System (project files)
export {
  readFile,
  writeFile,
  listFiles,
  listProjectTree,
  fileExists,
  readSiteFile,
  listSiteFilesWithSizes,
  type ProjectFileEntry,
  type SiteFileInfo,
} from "./filesystem.js";

// Keys (keystore — scoped to your plugin, no permission needed)
export {
  getKey,
  listKeys,
  signWithKey,
  type KeyAlgorithm,
  type KeyInfo,
} from "./keystore.js";

// Plugin Storage (plugin's private directory)
export {
  readPluginFile,
  writePluginFile,
  pluginFileExists,
} from "./plugin-storage.js";

// HTTP
export {
  fetchUrl,
  httpPost,
  httpGet,
  httpPostMultipart,
  htmlToMarkdown,
  downloadAsset,
  type FetchOptions,
  type FetchResult,
  type PostOptions,
  type GetOptions,
  type MultipartTextField,
  type MultipartFilePart,
  type MultipartPostOptions,
  type DownloadOptions,
  type DownloadResult,
} from "./http.js";

// Binary Execution
export {
  executeBinary,
  type ExecuteOptions,
  type ExecuteResult,
} from "./binary.js";

// Platform Detection
export {
  getPlatformInfo,
  type OSType,
  type ArchType,
  type PlatformKey,
  type PlatformInfo,
} from "./platform.js";

// Cookie Management
export {
  getPluginCookie,
  setPluginCookie,
  clearPluginCookies,
  type Cookie,
} from "./cookies.js";

// Event Communication
export { emitEvent, onEvent } from "./events.js";

// Toast Notifications
export {
  showToast,
  dismissToast,
  TOAST_EVENT,
  TOAST_UPDATE_EVENT,
  TOAST_DISMISS_EVENT,
  type ToastVariant,
  type ToastAction,
  type ToastOptions,
  type ToastType, // deprecated
} from "./toast.js";
