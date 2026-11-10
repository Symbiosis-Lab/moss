import type { Takeover } from '../editor/session/node-class';

/**
 * EventBus for cross-component communication
 *
 * This module provides a simple pub/sub event system to decouple
 * components that need to communicate without direct references.
 *
 * Usage:
 * ```typescript
 * import { eventBus, Events } from "./context/event-bus";
 *
 * // Subscribe to events
 * const unsubscribe = eventBus.on(Events.SYNDICATION_STATE_CHANGED, (data) => {
 *   console.log("Enabled platforms:", data.enabledPlatforms);
 * });
 *
 * // Emit events
 * eventBus.emit(Events.SYNDICATION_STATE_CHANGED, { enabledPlatforms: ["matters"] });
 *
 * // Cleanup
 * unsubscribe();
 * ```
 */

type EventCallback<T = unknown> = (data: T) => void;

class EventBus {
  private listeners = new Map<string, Set<EventCallback>>();

  /**
   * Subscribe to an event
   * @returns Unsubscribe function
   */
  on<T>(event: string, callback: EventCallback<T>): () => void {
    if (!this.listeners.has(event)) {
      this.listeners.set(event, new Set());
    }
    this.listeners.get(event)!.add(callback as EventCallback);

    // Return unsubscribe function
    return () => this.off(event, callback);
  }

  /**
   * Unsubscribe from an event
   */
  off<T>(event: string, callback: EventCallback<T>): void {
    this.listeners.get(event)?.delete(callback as EventCallback);
  }

  /**
   * Emit an event to all subscribers
   */
  emit<T>(event: string, data: T): void {
    this.listeners.get(event)?.forEach((cb) => cb(data));
  }

  /**
   * Remove all listeners for an event (useful for cleanup)
   */
  clear(event?: string): void {
    if (event) {
      this.listeners.delete(event);
    } else {
      this.listeners.clear();
    }
  }

  /**
   * Get the number of listeners for an event (useful for testing)
   */
  listenerCount(event: string): number {
    return this.listeners.get(event)?.size ?? 0;
  }
}

// Singleton instance
export const eventBus = new EventBus();

/**
 * Event type definitions for type-safe event handling
 */
export const Events = {
  /** Emitted when syndication platform state changes */
  SYNDICATION_STATE_CHANGED: "syndication:stateChanged",

  /** Emitted when deploy state changes */
  DEPLOY_STATE_CHANGED: "deploy:stateChanged",

  /** Emitted when the folder path changes */
  FOLDER_PATH_CHANGED: "folder:pathChanged",

  /** Emitted when preview is ready */
  PREVIEW_READY: "preview:ready",

  /** Emitted when the preview URL changes (navigation within preview iframe) */
  PREVIEW_URL_CHANGED: "preview:urlChanged",

  /** Emitted when a plugin is installed via the plugin installer */
  PLUGIN_INSTALLED: "plugin:installed",

  /** Emitted when a plugin is uninstalled via the plugin installer */
  PLUGIN_UNINSTALLED: "plugin:uninstalled",

  /** Emitted when a built-in channel install/uninstall completes */
  CHANNEL_INSTALL_CHANGED: "channel:install-changed",

  /** Emitted after plugin buttons are reloaded (e.g., folder switch, install/uninstall) */
  PLUGINS_RELOADED: "plugins:reloaded",

  /** Emitted when the plugin installer panel opens */
  PLUGIN_INSTALLER_OPENED: "plugin-installer:opened",

  /** Emitted when the plugin installer panel closes */
  PLUGIN_INSTALLER_CLOSED: "plugin-installer:closed",

  /** Emitted when deployment progress changes */
  DEPLOY_PROGRESS: "deploy:progress",

  /** Emitted when the settings modal opens */
  SETTINGS_MODAL_OPENED: "settings-modal:opened",

  /** Emitted when the settings modal closes */
  SETTINGS_MODAL_CLOSED: "settings-modal:closed",

  /**
   * Emitted to request the settings modal to open, optionally at a section.
   * Consumed by app.ts which holds the SettingsModal reference.
   */
  OPEN_SETTINGS_MODAL: "settings-modal:open",

  /** Emitted when first publish completes (mosspub.com or GitHub) */
  FIRST_PUBLISH_COMPLETE: "deploy:firstPublishComplete",

  /** Emitted after a newsletter send attempt completes */
  EMAIL_SEND_RESULT: "email:sendResult",

  /**
   * Emitted after the subscriber list has been mutated (add / remove /
   * lang change / import). Lets dependent UI (e.g. the email channel button
   * tooltip) re-read the audience count without polling.
   */
  EMAIL_SUBSCRIBERS_CHANGED: "email:subscribersChanged",

  /**
   * Emitted after the email send mode has been changed via the settings UI.
   * Lets dependent UI re-read the mode (e.g. switch between "all", "scope",
   * "test" tooltips on the email channel button).
   */
  EMAIL_SEND_MODE_CHANGED: "email:sendModeChanged",

  /**
   * Emitted after the email api_key has been saved (or cleared) via the
   * settings UI. Lets the email-tab dot re-derive immediately without
   * waiting for SETTINGS_MODAL_CLOSED — the dot must flip the moment the
   * user types a key and saves, not on modal close.
   *
   * Bridged from a Rust `app.emit_to("preview", "email-api-key-changed", ())`
   * inside the save command (see `domain::email_api_key::save_email_api_key`
   * and `domain::commands::save_service_config_value`).
   */
  EMAIL_API_KEY_CHANGED: "email:apiKeyChanged",

  /**
   * Emitted when a config surface that may flip the email-tab dot's verdict
   * changes (deploy method switch, channel install state change captured via
   * the more specific events above). Currently fired by app.ts after a
   * channel-install or deploy-method change so the tab dot re-derives even
   * when those specific event predicates miss.
   */
  BUILD_CONFIG_CHANGED: "build-config:changed",

  /**
   * Asks the preview to rebuild the open folder. The app-op dispatcher emits it
   * after a tool download, so the rebuild finds the new binary; the preview
   * manager runs its own `startBuild` for the folder it holds.
   */
  REBUILD_OPEN_FOLDER: "preview:rebuildOpenFolder",

  /**
   * Emitted by deploy-handler (and other non-panel producers) to surface a
   * degraded notice in the ProgressPanel advisory zone without going through
   * a toast. Payload is an AdvisoryEntry; ProgressPanel.init() subscribes and
   * sets it in `this.advisories` keyed by task (clears at next build-start
   * like other advisories). Keyed so it never duplicates.
   */
  PUSH_NOTICE: "panel:pushNotice",

  /**
   * The scoped-clear verb paired with PUSH_NOTICE: the producer that posted a
   * keyed notice removes it again.
   * The panel deletes the key, re-renders, and runs its auto-close check so a
   * cleared verdict lets a settled panel retract. This — not a blanket clear —
   * is how a `sticky` notice ends.
   */
  CLEAR_NOTICE: "panel:clearNotice",

  /**
   * A file named somewhere in the app chrome was clicked and should open in the
   * editor. Emitted by the notices rail, where every advisory that names a file
   * names one the author has to go and edit; handled in app.ts, which owns the
   * editor panel. The path may be site-root-relative (what advisories carry) or
   * absolute — the handler resolves it against the open folder.
   */
  OPEN_FILE_IN_EDITOR: "editor:openFile",
} as const;

/**
 * Type definitions for event payloads
 */
export interface OpenFileInEditorEvent {
  /** Site-root-relative (e.g. `travel/kyoto.md`) or absolute. */
  path: string;
  /** Completed-build provenance for an exact future editor reveal. */
  buildGeneration?: number;
  sourceRevision?: string;
  sourceSpan?: { start_byte: number; end_byte: number; line: number };
}
export interface SyndicationStateChangedEvent {
  enabledPlatforms: string[];
  isOnArticle: boolean;
  /**
   * Absolute source file of the current article (the source resolved ONCE at
   * the navigation boundary — PREVIEW_URL_CHANGED.sourcePath), or null when the
   * current page is not an article. The deploy-handler consumes THIS for the
   * email send instead of re-deriving the source from the preview URL.
   */
  articleSourcePath: string | null;
  /** This post's chosen places, other than the site, that publishing would send to now. */
  copiesDue?: string[];
}

export interface DeployStateChangedEvent {
  isDeploying: boolean;
  success?: boolean;
}

export interface FolderPathChangedEvent {
  path: string;
}

export interface PreviewReadyEvent {
  port: number;
  url: string;
}

/**
 * Payload for PREVIEW_URL_CHANGED: the settled answer to which source backs
 * the page the preview shows (see `preview/page-source.ts`).
 */
export interface PreviewUrlChangedEvent {
  /**
   * Null when this is an answer. `lookup`: moss has none for the page yet (the
   * lookup failed, or the build has not mapped it). `download`: its source is
   * still coming down from the cloud.
   */
  pending: "lookup" | "download" | null;
  /** URL path from preview (e.g., "posts/my-article.html") */
  urlPath: string;
  /** Absolute path to the source markdown file, or null */
  sourcePath: string | null;
  /** Whether this page is a directory listing (homepage, section index) */
  isDir: boolean;
  /** Whether this URL corresponds to a syndicatable article */
  isArticle: boolean;
  /** URLs where this article has been syndicated (from frontmatter) */
  syndicated: string[];
  /** What would replace this page when no source backs it (a home note, a claim file), else null */
  takeover: Takeover | null;
  /**
   * True when this answers the page already on screen again — a same-URL
   * rebuild echo, or a fresh lookup of that page — rather than a navigation.
   * The first load and any navigation to a new URL are NOT reloads.
   *
   * Consumers must NOT treat a reload as "the user navigated here": the editor
   * link forwards one to the editor only while the editor waits on this page,
   * so a home-page rebuild triggered by creating a new file cannot stomp the
   * editor off the brand-new file it just opened.
   */
  isReload: boolean;
}

/**
 * Payload for PLUGIN_INSTALLED event
 * Emitted when a plugin is installed via the plugin installer
 */
export interface PluginInstalledEvent {
  /** The plugin ID that was installed */
  pluginId: string;
}

/**
 * Payload for PLUGIN_UNINSTALLED event
 * Emitted when a plugin is uninstalled via the plugin installer
 */
export interface PluginUninstalledEvent {
  /** The plugin ID that was uninstalled */
  pluginId: string;
}

/**
 * Payload for CHANNEL_INSTALL_CHANGED event
 * Emitted when a built-in channel is installed or uninstalled
 */
export interface ChannelInstallChangedEvent {
  /** The channel ID that was installed or uninstalled */
  channelId: string;
  /** Whether the channel is now installed */
  installed: boolean;
}

/**
 * Payload for PLUGINS_RELOADED event
 * Emitted after reloadPlugins() completes (folder switch, install, uninstall)
 */
export interface PluginsReloadedEvent {}

/**
 * Payload for PLUGIN_INSTALLER_OPENED event
 */
export interface PluginInstallerOpenedEvent {}

/**
 * Payload for PLUGIN_INSTALLER_CLOSED event
 */
export interface PluginInstallerClosedEvent {}

/**
 * Payload for DEPLOY_PROGRESS event
 * Emitted when deployment progress changes
 */
export interface DeployProgressEvent {
  isDeploying: boolean;
  status?: "pending" | "in_progress" | "success" | "error";
}

/**
 * Payload for SETTINGS_MODAL_OPENED event
 */
export interface SettingsModalOpenedEvent {}

/**
 * Payload for SETTINGS_MODAL_CLOSED event
 */
export interface SettingsModalClosedEvent {}

/**
 * Payload for OPEN_SETTINGS_MODAL event
 * Emitted by components that want to open the settings modal at a section.
 */
export interface OpenSettingsModalEvent {
  /** Optional section ID to activate (e.g. 'deploy', 'overview') */
  section?: string;
}

/**
 * Payload for FIRST_PUBLISH_COMPLETE event
 */
export interface FirstPublishCompleteEvent {
  url: string;
  method: string; // "moss" | "github-pages"
  /**
   * The site this first publish belongs to. Setup is long enough for the
   * author to open another site while it runs, and every listener here writes
   * per-site state; "" when the modal was opened without one.
   */
  folder: string;
}

/**
 * Payload for EMAIL_SEND_RESULT event
 * Emitted after a newsletter send attempt (success or partial failure)
 */
export interface EmailSendResultEvent {
  sent: number;
  failed: number;
}

/**
 * Payload for EMAIL_SUBSCRIBERS_CHANGED event
 * Empty — listeners re-read the list via `commands.emailListSubscribers`.
 * Avoids shipping the whole subscriber array through the bus.
 */
export interface EmailSubscribersChangedEvent {}

/**
 * Payload for EMAIL_SEND_MODE_CHANGED event
 * Empty — listeners re-read the mode via `commands.emailGetSendMode`.
 */
export interface EmailSendModeChangedEvent {}

/**
 * Payload for PUSH_NOTICE event.
 * Carries an AdvisoryEntry so ProgressPanel can surface it as a degraded
 * notice in the advisory zone (same model as BackgroundProgress advisories).
 * The `task` field is used as the advisory key; a second emit with the same
 * task replaces the first (last-write-wins, no duplication).
 */
export interface PushNoticeEvent {
  task: string;
  /**
   * The file this notice is about, or `null` for a site-wide one. A notice that
   * names a file gets it rendered as the row's item, which is how the missing-media
   * refusal shows one row per page the author has to open.
   */
  item: string | null;
  reason: string;
  severity: "NeedsAction" | "ShippedDegraded" | "Blocking";
  scope: "Remote" | "File" | "Config" | "Environment" | "Account";
  action?: unknown;
  /**
   * A sticky notice survives the panel's build-start blanket clears and is
   * removed only by CLEAR_NOTICE (or panel destroy). For notices scoped to
   * something OTHER than a build — the `publish-verify` advisory is scoped to
   * a publish generation — an ordinary file-edit rebuild mid-check must not
   * silently wipe the verdict surface before the author has read it.
   */
  sticky?: boolean;
}

/**
 * Payload for CLEAR_NOTICE: the advisory key (`PushNoticeEvent.task`) to
 * remove from the panel's store.
 */
export interface ClearNoticeEvent {
  task: string;
}
