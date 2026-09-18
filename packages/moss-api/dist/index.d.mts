//#region src/types/plugin.d.ts
/**
 * Base plugin types shared across all moss plugins
 */
/**
 * Wire truth: `ProjectInfo` in moss's `crates/moss-build/src/plugins/types.rs`.
 * `project_type` and `content_folders` were removed there (2026); this type
 * carried them for months after — keep the two in sync.
 *
 * @category Plugin manifest
 */
interface ProjectInfo {
  total_files: number;
  homepage_file?: string;
  /**
   * Root folder basename (e.g. "刘果"). Plugins that generate a folder home
   * should name it self-named (`<folder_name>.md`) with a `home: true` marker
   * to match moss's folder-home convention.
   */
  folder_name?: string;
  site_name?: string;
  /** BCP-47 language code detected from content (e.g. "en", "zh-hant"). Always sent. */
  lang: string;
}
/** @category Plugin manifest */
interface PluginManifest {
  name: string;
  version: string;
  entry: string;
  category: PluginCategory;
  global_name?: string;
  icon?: string;
  domain?: string;
  config?: Record<string, unknown>;
}
/** @category Plugin manifest */
type PluginCategory = "generator" | "deployer" | "syndicator" | "enhancer" | "processor";
//#endregion
//#region src/types/messages.d.ts
/**
 * Plugin message types for communication with moss
 */
/**
 * Messages that plugins can send to moss
 * @category Messages
 */
type PluginMessage = LogMessage | ProgressMessage | ErrorMessage | CompleteMessage;
/** @category Messages */
interface LogMessage {
  type: "log";
  level: "log" | "warn" | "error";
  message: string;
}
/** @category Messages */
interface ProgressMessage {
  type: "progress";
  phase: string;
  current: number;
  total: number;
  message?: string;
}
/** @category Messages */
interface ErrorMessage {
  type: "error";
  error: string;
  context?: string;
  fatal: boolean;
}
/** @category Messages */
interface CompleteMessage {
  type: "complete";
  success: boolean;
  error?: string;
  result?: unknown;
}
//#endregion
//#region src/utils/messaging.d.ts
/**
 * Set the message context for subsequent messages
 * This is typically called automatically by the plugin runtime
 * @category Messaging
 */
declare function setMessageContext(pluginName: string, hookName: string): void;
/**
 * Send a message to moss
 *
 * Log and progress messages use events (fire-and-forget) to avoid blocking IPC.
 * Complete and error messages use commands (request-response) for acknowledgment.
 * @category Messaging
 */
declare function sendMessage(message: PluginMessage): Promise<void>;
/**
 * Report progress to moss
 * @category Messaging
 */
declare function reportProgress(phase: string, current: number, total: number, message?: string): Promise<void>;
/**
 * Report an error to moss
 * @category Messaging
 */
declare function reportError(error: string, context?: string, fatal?: boolean): Promise<void>;
/**
 * `PluginHook` mirrors the closed Rust enum in
 * `src-tauri/src/plugins/types.rs`. The router (T1) cross-products
 * `PluginHook × TriggerContext` to pick a UI surface for the task.
 * Plugin authors pick the hook that matches what they're doing; they
 * do NOT pick the surface (the router owns that).
 * @category Messaging
 */
type PluginHook = "import" | "publish" | "deploy" | "syndicate" | "process";
/**
 * `TriggerContext` mirrors the closed Rust enum. Tells the router *why*
 * the task was invoked so it can pick a surface that matches the user's
 * focus context (onboarding cards → ActionPanel; background sync →
 * Workspace).
 * @category Messaging
 */
type TriggerContext = "onboarding_flow" | "settings_manual" | "background" | "manual_one";
/**
 * Escape kind for `awaiting()` calls. The string variants take a
 * free-text affordance label after the colon, mirroring the dev harness
 * dialect (e.g., `"resend:Resend email"`, `"recheck:Recheck DNS"`).
 * Plain `"cancel"` carries no label.
 * @category Messaging
 */
type EscapeSpec = "cancel" | `resend:${string}` | `recheck:${string}`;
/**
 * Which axis of the system an advisory is about. Mirrors the Rust
 * `advisory::Scope` (externally-tagged unit enum → bare string).
 * @category Messaging
 */
type AdvisoryScope = "File" | "Config" | "Environment" | "Remote" | "Account";
/**
 * How serious the plugin proposes an advisory is. moss CLAMPS this (R13): a
 * `Blocking` proposal with no actionable affordance is demoted to a quiet
 * `NeedsAction` hairline dot. Mirrors the Rust `advisory::Severity`.
 * @category Messaging
 */
type AdvisorySeverity = "ShippedDegraded" | "NeedsAction" | "Blocking";
/**
 * A closed set of in-app operations an `AdvisoryAction.InApp` can request.
 * Mirrors the Rust `advisory::AppOp`.
 * @category Messaging
 */
type AdvisoryAppOp = "MoveFile" | "OpenBilling" | "SignIn" | "RecheckDns";
/**
 * The recovery affordance for an advisory, expressed as data — mirrors the
 * Rust `advisory::Action` (externally-tagged: `"None"` for the unit variant,
 * `{ Variant: {...} }` for data variants). `Action !== "None"` is the gavel's
 * deciding input for whether a `Blocking` proposal may pop the panel.
 * @category Messaging
 */
type AdvisoryAction = "None" | {
  Command: {
    run: string;
    label: string;
  };
} | {
  InApp: {
    op: AdvisoryAppOp;
    args: unknown;
    label: string;
  };
} | {
  Link: {
    href: string;
    label: string;
  };
};
/**
 * A plugin's PROPOSED advisory (pre-clamp). Mirrors the Rust
 * `plugins::types::PluginAdvisory` wire shape exactly so it deserializes
 * directly into `PluginTaskLifecycle::Succeeded/Failed { advisories }`. moss
 * is the only constructor of a final `Advisory` — a plugin can never hand moss
 * one (R13).
 * @category Messaging
 */
interface AdvisoryProposal {
  /** Which axis of the system this advisory is about. */
  scope: AdvisoryScope;
  /** The severity the plugin REQUESTS. moss clamps it (R13). */
  severity: AdvisorySeverity;
  /**
   * The site-relative path of the file this advisory is about, e.g.
   * `posts/2026/hello.md` — omit it (`null`) for a build-wide notice. moss
   * resolves it by joining it onto the open folder to let the reader click
   * straight to the file, so an absolute path or one containing `..` is
   * dropped rather than trusted; the advisory then renders build-wide.
   */
  item: string | null;
  /** What happened (free text). */
  what: string;
  /** The recovery affordance the plugin proposes. */
  action: AdvisoryAction;
}
/** @category Messaging */
interface StartTaskOptions {
  /**
   * Hook the task belongs to. Defaults to "import" — the most common
   * onboarding case. Plugins should pass an explicit hook for non-import
   * work (e.g., a syndicator passes "syndicate").
   */
  hook?: PluginHook;
  /**
   * Trigger context. Defaults to "background" — the safest fallback
   * because Background routes to Workspace+Ambient, the quietest surface.
   * Plugins running inside the onboarding flow should pass
   * "onboarding_flow" explicitly so they reach the ActionPanel hairline.
   */
  trigger?: TriggerContext;
  /**
   * Hint to the router that this task will emit `progress()` updates
   * with fractions. Routers and renderers MAY use this to choose between
   * "fills" vs "pulses" visualizations. Defaults to true.
   */
  hasProgress?: boolean;
  /**
   * Whether the user can cancel this task from the UI. Cancellation
   * plumbing lands in a later ADR phase; the flag is recorded today so
   * renderers can show / hide a cancel affordance.
   */
  cancellable?: boolean;
  /**
   * Plugin-local job id referencing `contributes.jobs[id]` in the plugin's
   * manifest (Step 3 Phase 5, §8 + R13). When set, moss looks up the declared
   * descriptor (a past-tense `verb` + an amount `noun`), normalizes the verb
   * (`Verb::normalized` — moss owns capitalization/length/glyphs), and on a
   * `succeeded(receipt, count)` stamps its OWN typed `Verb` + `Amount { count,
   * noun }` on the Job — rendering "Syndicated · N posts" from moss's value
   * objects, never the plugin's pre-formatted `receipt` string. Omit it for
   * free-text-receipt tasks (the legacy path, byte-identical).
   */
  job?: string;
}
/**
 * Lifecycle handle returned by `startTask()`. Calls are fire-and-await:
 * each method returns a promise that resolves once the Rust side has
 * applied the transition to the PanelTask registry. Terminal calls
 * (`succeeded`, `failed`, `cancelled`) remove the task from the registry's
 * tracking store; calling any further method on the same handle will
 * reject with "unknown plugin task id".
 *
 * The state machine matches ADR-015 § Layer 2:
 *
 *   Running ↔ Awaiting → Succeeded | Failed | Cancelled
 *
 * `progress()` after `awaiting()` implicitly transitions back to Running
 * (no explicit `resumed()`).
 * @category Messaging
 */
interface TaskHandle {
  /**
   * In-process task id minted by the Rust registry on `Started`.
   * Exposed for log correlation and tests.
   *
   * Rust-side this is `u64`; specta types `u64` as `string` because
   * JS numbers lose precision above 2^53. The handle carries the
   * exact string through subsequent transitions so no precision is
   * lost in the round-trip.
   */
  readonly id: string;
  /** Push a progress update. `fraction` in [0,1] if known, else undefined for indeterminate. */
  progress(fraction?: number, message?: string): Promise<void>;
  /**
   * Pause for an out-of-band user action. `directive` describes what
   * the user needs to do ("click the link in your email"); `venue`
   * names where ("your email") — both feed the Awaiting renderer's
   * "Waiting for you to [directive] in [venue]" copy.
   *
   * `escape` defaults to "cancel". For non-cancel escapes, pass
   * `"resend:<label>"` or `"recheck:<label>"`.
   */
  awaiting(directive: string, venue: string, escape?: EscapeSpec): Promise<void>;
  /**
   * PROPOSE an advisory on this task (Step 3 Phase 5, §8 + R13). Accumulates
   * the proposal on the handle; it is flushed into the next terminal call
   * (`succeeded`/`failed`) as `advisories: PluginAdvisory[]`. moss holds the
   * severity gavel server-side: a `Blocking` proposal with no actionable
   * `action` is clamped to a quiet `NeedsAction` dot; an actionable `Blocking`
   * on a `succeeded()` flips the run to `Failed` (invariant #1 — the smart
   * constructor decides, not the plugin).
   *
   * `advise()` does NOT emit on its own; advisories ride the terminal IPC so
   * moss applies them atomically with the success/failure transition. Calling
   * `advise()` after a terminal call has no effect — the handle is spent (the
   * terminal methods set a spent flag and `advise()` no-ops once spent).
   */
  advise(advisory: AdvisoryProposal): Promise<void>;
  /**
   * Terminal: success.
   *
   * @param receipt Optional human-readable receipt (the legacy free-text path).
   *   IGNORED for the verb/amount when the task declared a `job` descriptor —
   *   moss renders the receipt from its OWN normalized verb + amount instead.
   * @param amount Optional success COUNT. Only meaningful when `startTask` was
   *   given a `job` id: moss pairs this count with the descriptor's `noun` to
   *   stamp `Amount { count, noun }` and renders "Syndicated · N posts" from its
   *   value objects.
   */
  succeeded(receipt?: string, amount?: number): Promise<void>;
  /**
   * Terminal: failure. `recoverable=false` (default) also fires the
   * toast subscriber (ADR-015 § "Plugin-originated failure toasts").
   */
  failed(error: string, recoverable?: boolean): Promise<void>;
  /** Terminal: explicit user cancellation. */
  cancelled(): Promise<void>;
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
 * supported until ADR-015 Phase 3 sweeps all 151 call sites.
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
declare function startTask(label: string, options?: StartTaskOptions): Promise<TaskHandle>;
//#endregion
//#region src/types/context.d.ts
/**
 * Base context shared by all hooks
 *
 * Contains only business data - no paths.
 * Use readFile(), writeFile() for project files.
 * Use readPluginFile(), writePluginFile() for plugin storage.
 *
 * @category Hook contexts
 */
interface BaseContext {
  project_info: ProjectInfo;
  config: Record<string, unknown>;
}
/**
 * Context for before_build hook (process capability)
 *
 * `trigger` is stamped by moss (ADR-015): the plugin reads it to declare task
 * intent via `startTask`, it does NOT guess it. Onboarding card → "onboarding_flow"
 * (drives the ambient hairline); every build/preview rebuild → "background".
 * Optional for backward compatibility; absent ⇒ treat as "background".
 *
 * @category Hook contexts
 */
interface ProcessContext extends BaseContext {
  trigger?: TriggerContext;
}
/**
 * Context for on_deploy hook (deployer plugins)
 *
 * @category Hook contexts
 */
interface DeployContext extends BaseContext {
  site_files: string[];
  /** Custom domain from .moss/config.toml [deployment] section (if configured) */
  domain?: string;
}
/**
 * Context for configure_domain hook (custom domain setup on deploy platform)
 *
 * Called after DNS records are configured via moss-oracle. Allows deploy plugins
 * to perform platform-specific domain setup (e.g., GitHub Pages CNAME configuration).
 *
 * This is NOT a separate capability - it's an optional hook on Deploy-capable plugins.
 *
 * ## Idempotency Contract
 *
 * The domain orchestrator calls this hook at multiple lifecycle points:
 * 1. After DNS records are configured (site may not be live yet)
 * 2. After the site is verified live via HTTP 200
 *
 * **Plugins MUST implement this hook as idempotent.** The plugin should:
 * - Check current platform state
 * - Do only the next needed step
 * - Return success as a no-op if already fully configured
 *
 * Example (GitHub Pages):
 * - Call 1: Sets CNAME file via API
 * - Call 2: Verifies CNAME is set, enforces HTTPS via API
 * - Call 3+: Both already done, returns success without changes
 *
 * @category Hook contexts
 */
interface ConfigureDomainContext extends BaseContext {
  /** The custom domain being configured (e.g., "example.com") */
  domain: string;
  /** Deployment information from the last deploy */
  deployment: DeploymentInfo;
}
/**
 * Context for the optional `check_setup` hook (deploy plugins).
 *
 * Deliberately not a {@link BaseContext}: this runs on the Publish click,
 * before the build, so it carries no project scan — only where the folder is,
 * the plugin's resolved settings, and what the user just submitted.
 *
 * `action` is what makes the probe a conversation rather than a verdict. The
 * first call arrives without one; submitting a blocker's form calls the same
 * hook again with that blocker's `id` as `action` and the form's `values`,
 * and the plugin does the work and answers with the next state. Every call is
 * cold — re-derive the current step from durable state, never from memory.
 *
 * ```typescript
 * export async function check_setup(ctx: SetupContext): Promise<HookResult> {
 *   if (ctx.action === "start_daemon") await startDaemon();
 *   if (await daemonIsUp()) return { success: true, setup: { status: "ready" } };
 *   return {
 *     success: true,
 *     setup: {
 *       status: "blocked",
 *       blockers: [{
 *         id: "start_daemon",
 *         message:
 *           "The IPFS daemon isn't running. It keeps running after moss quits.",
 *         form: { fields: [], submit: "Start it" },
 *       }],
 *     },
 *   };
 * }
 * ```
 *
 * @category Hook contexts
 */
interface SetupContext {
  /** Absolute path to the project folder about to be published */
  project_path: string;
  /** The plugin's resolved plain settings (declared defaults ∪ saved values) */
  settings: Record<string, unknown>;
  /** @deprecated The same map as {@link SetupContext.settings}, pre-contract name. */
  config: Record<string, unknown>;
  /** The id of the blocker whose form was submitted, absent on the first call */
  action?: string;
  /** The submitted form values riding with `action`; empty for a button */
  values?: Record<string, unknown>;
}
/**
 * Context for after_deploy hook (syndicator plugins)
 *
 * `trigger` is stamped by moss (ADR-015), same contract as
 * {@link ProcessContext.trigger}. Syndication has exactly one production
 * caller — the Publish click — so this is always `"manual_one"`; absent
 * (older moss) ⇒ treat as `"background"`.
 *
 * @category Hook contexts
 */
interface SyndicateContext extends BaseContext {
  site_files: string[];
  articles: ArticleInfo[];
  deployment?: DeploymentInfo;
  trigger?: TriggerContext;
}
/**
 * Article information for syndication
 *
 * @category Hook contexts
 */
interface ArticleInfo {
  source_path: string;
  title: string;
  content: string;
  /** Rendered HTML content (article body, no page template) */
  html_content?: string;
  frontmatter: Record<string, unknown>;
  url_path: string;
  date?: string;
  tags: string[];
}
/**
 * Deployment result information
 *
 * @category Hook contexts
 */
interface DeploymentInfo {
  method: string;
  url: string;
  deployed_at: string;
  metadata: Record<string, string>;
  /** DNS target for custom domain configuration */
  dns_target?: DnsTarget;
  /**
   * Every way to reach what was just published. moss keeps these in the
   * deployment record and lists them in the deploy tab whenever it is open,
   * so an address returned here outlives the toast that announced it.
   */
  addresses?: DeployAddress[];
}
/**
 * What one address IS, which decides how moss offers it.
 *
 * A kind moss does not recognise still renders — as a labelled row with its
 * value — so a plugin may name one outside this list.
 *
 * @category Hook contexts
 */
type AddressKind = "cid" | "ipns" | "gateway" | "domain";
/**
 * One way to reach the published site.
 *
 * A publish usually produces several: the CID that names these exact bytes,
 * the IPNS name that will name the next ones too, the gateway URL that makes
 * either reachable from a browser.
 *
 * Give an address a `url` when it opens in a browser, a `value` when it is
 * also (or only) worth copying — moss shows both when both are given, a
 * copy button when only `value` is set, and an open link when only `url` is.
 *
 * @category Hook contexts
 */
interface DeployAddress {
  kind: AddressKind | (string & {});
  /** Shown verbatim as the row's label, e.g. "IPFS CID". */
  label: string;
  /** Openable in a browser. */
  url?: string;
  /** The literal string to copy — beside `url`, or on its own. */
  value?: string;
  /** One line of context shown beside it, e.g. "Public gateway, may be slow". */
  note?: string;
}
/**
 * A single DNS record provided by deploy plugins
 *
 * @category Hook contexts
 */
interface DnsRecord {
  /** Record type: "A", "AAAA", "CNAME", "TXT", etc. */
  record_type: string;
  /** Record name: "@" for apex, "www", etc. */
  name: string;
  /** Record value: IP address or hostname */
  value: string;
  /** Optional TTL in seconds */
  ttl?: number;
}
/**
 * DNS configuration provided by deploy plugins
 *
 * Plugins are responsible for generating the appropriate DNS records
 * for their platform. moss just passes these through to DNS configuration.
 *
 * @category Hook contexts
 */
interface DnsTarget {
  /** List of DNS records to configure */
  records: DnsRecord[];
}
//#endregion
//#region src/types/hooks.d.ts
/**
 * Outcome notification described by a hook result.
 *
 * This is data about what happened, not a rendering instruction: moss owns
 * every status surface (per its plugin-architecture boundary) and maps
 * `outcome` to its own toast severity, timing, and suppression rules — e.g.
 * a surface that already shows the outcome (the first-publish wizard) can
 * swallow it entirely.
 *
 * @category Hooks
 */
interface HookToast {
  /** What happened: "success" | "info" | "error" */
  outcome: "success" | "info" | "error";
  /** Short display text (e.g., "Live on Tor", "No changes to deploy") */
  title: string;
  /** Optional clickable URL (e.g., the deployed site URL) */
  url?: string | null;
}
/**
 * Standard result returned from hook execution
 *
 * ## Design Principles
 *
 * 1. **Single completion path**: Return value only, no explicit reporting
 * 2. **Flow control only**: `success` tells moss whether to continue
 * 3. **Outcome UX is data, not calls**: describe the outcome in `toast`;
 *    moss decides how (and whether) to present it. A hook must succeed with
 *    no UI attached at all — CLI and headless hosts run the same hooks.
 *
 * ## Usage Pattern
 *
 * ```typescript
 * async function deploy(context): Promise<HookResult> {
 *   // Do work...
 *
 *   // Return result; `toast` describes the outcome for moss to render
 *   return {
 *     success: true,
 *     deployment: {...},
 *     toast: { outcome: "success", title: "Deployed!", url },
 *   };
 * }
 * ```
 * @category Hooks
 */
interface HookResult {
  /** Whether the operation succeeded */
  success: boolean;
  /** Detailed message for logs/debugging */
  message?: string;
  /** Outcome notification for moss to present (moss controls rendering) */
  toast?: HookToast | null;
  /** Deployment info (populated by deploy hooks) */
  deployment?: DeploymentInfo;
  /** Setup verdict (populated by the optional `check_setup` hook) */
  setup?: SetupVerdict | LegacySetupVerdict;
}
/**
 * One field of a {@link SetupForm} — the same vocabulary a manifest's
 * `settings[]` declares, minus `when` (in a stepped flow, the steps are the
 * conditionality). moss draws it; the plugin supplies no pixels.
 *
 * @category Hooks
 */
interface SetupField {
  /** The key the submitted value arrives under in `SetupContext.values` */
  key: string;
  type: "string" | "number" | "boolean" | "secret";
  label?: string;
  /** One sentence under the field */
  description?: string;
  /** A suggestion moss pre-fills — a claimed-name candidate, say */
  default?: unknown;
  /** Present on a `string` field, it closes the value set: moss draws a select */
  options?: {
    value: string;
    label: string;
    description?: string;
  }[];
  placeholder?: string;
  help_url?: string;
  /** Checked per keystroke; requires `pattern_message` */
  pattern?: string;
  pattern_message?: string;
}
/**
 * A {@link SetupBlocker}'s form. A zero-field form is a button, and the
 * blocker's `message` is what the person reads before pressing it.
 *
 * @category Hooks
 */
interface SetupForm {
  fields: SetupField[];
  /** Submit button label */
  submit: string;
}
/**
 * One reason the contribution is not ready, and what to do about it.
 *
 * `id` is the routing verb: when the user submits the form, `check_setup`
 * runs again with this id as `SetupContext.action`. Ids beginning `moss:`
 * are reserved for moss's own blockers (a failed manifest `need`) and are
 * never dispatched to your hook.
 *
 * @category Hooks
 */
interface SetupBlocker {
  id: string;
  /** What the person reads. Omit only when a form says it all. */
  message?: string;
  /** Present when there is something to submit */
  form?: SetupForm;
  /**
   * Per-field errors keyed by field `key`. Returning the SAME blocker again
   * with these marks the verdict a re-ask: moss re-shows the form with the
   * errors and does NOT persist the submitted values.
   */
  field_errors?: Record<string, string>;
}
/**
 * The `check_setup` verdict: `ready`, or `blocked` with blockers. There is no
 * third verdict — a hook that cannot tell answers `blocked` with a message
 * that says so and a zero-field retry form.
 *
 * On any answering verdict that is not a `field_errors` re-ask, moss persists
 * each submitted value whose key matches a declared setting: a `secret` into
 * the keystore, anything else into your config. Values your flow derived
 * (rather than the user typed) belong in your own state file.
 *
 * @category Hooks
 */
type SetupVerdict = {
  status: "ready";
} | {
  status: "blocked";
  blockers: SetupBlocker[];
};
/**
 * @deprecated The pre-contract verdict shape. moss still accepts it — each
 * need's action becomes a blocker whose zero-field form carries the action's
 * label, and `consent` folds into the message — but new code answers with
 * {@link SetupVerdict}.
 *
 * @category Hooks
 */
interface LegacySetupVerdict {
  ready: boolean;
  needs?: {
    id: string;
    message: string;
    actions?: {
      id: string;
      label: string;
      consent?: string;
    }[];
  }[];
}
//#endregion
//#region src/types/social.d.ts
/** One comment in the .moss/data/social/*.json shared standard.
 *  See moss/docs/reference/social-data-standard.md.
 *  @category Social */
interface SocialComment {
  id: string;
  source: string;
  content: string;
  createdAt: string;
  author: {
    displayName: string;
    url?: string;
  };
  replyToId?: string;
  /**
   * Moderation state. Absent or "active" = visible; anything else is filtered
   * at read time. Well-known values: "active" | "removed" | "archived" |
   * "banned" | "collapsed". The Rust side treats this as an open string so
   * future states do not require a client update.
   */
  state?: string;
}
/**
 * The social data for a single article — currently its comments.
 * @category Social
 */
interface SocialArticleData {
  comments: SocialComment[];
}
/**
 * A `.moss/data/social/*.json` file: the schema version plus every article's
 * social data, keyed by article path.
 * @category Social
 */
interface SocialDataFile {
  schemaVersion: string;
  articles: Record<string, SocialArticleData>;
}
//#endregion
//#region src/utils/tauri.d.ts
/**
 * Tauri core utilities for plugin communication
 * @category Tauri core (deprecated)
 */
interface TauriCore {
  invoke: <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
}
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
declare function getTauriCore(): TauriCore;
//#endregion
//#region src/utils/env.d.ts
/**
 * Plugin-side env-var access.
 *
 * Plugins run inside a webview (no Node `process.env`), so reading host
 * environment variables requires a Rust→TS bridge. The
 * `get_plugin_env_var` Tauri command in `src-tauri/src/plugins/runtime.rs`
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
declare function getPluginEnvVar(name: string): Promise<string | undefined>;
//#endregion
//#region src/utils/browser.d.ts
/**
 * Browser utilities for plugins
 * Abstracts Tauri browser commands to decouple plugins from internal APIs
 */
/**
 * Reason why the browser window was closed
 * @category Browser
 */
type BrowserCloseReason = {
  type: "user";
} | {
  type: "timeout";
} | {
  type: "programmatic";
};
/**
 * Handle returned by openBrowser for tracking window lifecycle
 * @category Browser
 */
interface BrowserHandle {
  /**
   * Promise that resolves when the browser window is closed.
   * Use this to detect when the user closes the window.
   */
  closed: Promise<BrowserCloseReason>;
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
declare function openBrowser(url: string): Promise<BrowserHandle>;
/**
 * Close the action panel
 * @category Browser
 */
declare function closeBrowser(): Promise<void>;
/**
 * Ask the app shell to restore the editor in the action panel after a login
 * flow cancel or failure. Clears the onboarding latch and re-mounts the
 * editor (empty-folder onboarding cards) in the action panel slot.
 *
 * Call this after `promptLogin()` returns false on an import (binding /
 * prompt_login) path so the user isn't left with an empty action panel.
 * @category Browser
 */
declare function returnToEditor(): Promise<void>;
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
declare function openSystemBrowser(url: string): Promise<void>;
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
 *       </script>
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
declare function openBrowserWithHtml(html: string): Promise<void>;
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
 *       </script>
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
declare function showBrowserForm<T>(html: string, options?: {
  timeoutMs?: number;
  closeDelayMs?: number;
}): Promise<T | null>;
//#endregion
//#region src/utils/filesystem.d.ts
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
declare function readFile(relativePath: string): Promise<string>;
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
declare function writeFile(relativePath: string, content: string): Promise<void>;
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
declare function listFiles(): Promise<string[]>;
/**
 * A project file with home-file annotation from Rust's detect_home_file_in_folder
 * @category Filesystem
 */
interface ProjectFileEntry {
  path: string;
  is_home: boolean;
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
declare function listProjectTree(): Promise<ProjectFileEntry[]>;
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
declare function fileExists(relativePath: string): Promise<boolean>;
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
declare function readSiteFile(relativePath: string): Promise<string>;
/**
 * File path and size from the compiled site directory
 * @category Filesystem
 */
interface SiteFileInfo {
  path: string;
  size: number;
}
/**
 * List all files in the compiled site directory with their sizes
 *
 * @returns Array of file info objects with path and size in bytes
 * @category Filesystem
 */
declare function listSiteFilesWithSizes(): Promise<SiteFileInfo[]>;
//#endregion
//#region src/utils/keystore.d.ts
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
/**
 * A signing algorithm for a key.
 *
 * - `ed25519` — EdDSA. Signature: raw 64 bytes. Public key: 32 bytes. The right
 *   choice for IPNS (its `MUST` key type) and most new protocols.
 * - `secp256k1-schnorr` — BIP-340. Signature: 64 bytes. Public key: x-only 32
 *   bytes. For Nostr-family protocols.
 *
 * @category Keys
 */
type KeyAlgorithm = "ed25519" | "secp256k1-schnorr";
/**
 * A key's public face. Never includes private material.
 * @category Keys
 */
interface KeyInfo {
  /** The name you gave the key, within your plugin's scope. */
  name: string;
  algorithm: KeyAlgorithm;
  /** The public key bytes, in the algorithm's standard encoding. */
  publicKey: Uint8Array;
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
declare function getKey(name: string, algorithm: KeyAlgorithm): Promise<KeyInfo>;
/**
 * List your keys.
 * @category Keys
 */
declare function listKeys(): Promise<KeyInfo[]>;
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
declare function signWithKey(name: string, payload: Uint8Array): Promise<Uint8Array>;
/**
 * Your secret stored under `key`, or `null` if there is none.
 *
 * `null` is the normal first-run answer, not an error. If your manifest
 * declares the key under `setup.credentials`, moss has already asked for it
 * before your hook ran, so `null` here means the user cancelled.
 *
 * @category Secrets
 */
declare function getSecret(key: string): Promise<string | null>;
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
declare function rejectSecret(key: string, options?: {
  detail?: string;
}): Promise<string | null>;
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
declare function setSecret(key: string, value: string): Promise<void>;
//#endregion
//#region src/utils/plugin-storage.d.ts
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
declare function readPluginFile(relativePath: string): Promise<string>;
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
declare function writePluginFile(relativePath: string, content: string): Promise<void>;
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
declare function pluginFileExists(relativePath: string): Promise<boolean>;
//#endregion
//#region src/utils/http.d.ts
/**
 * HTTP operations for moss plugins
 *
 * These functions provide HTTP capabilities that bypass browser CORS
 * restrictions by using Rust's HTTP client under the hood.
 *
 * Project path for downloads is auto-detected from the runtime context.
 */
/**
 * Options for HTTP fetch requests
 * @category HTTP
 */
interface FetchOptions {
  /** Timeout in milliseconds (default: 30000) */
  timeoutMs?: number;
}
/**
 * Result from an HTTP fetch operation
 * @category HTTP
 */
interface FetchResult {
  /** HTTP status code */
  status: number;
  /** Whether the request was successful (2xx status) */
  ok: boolean;
  /** Content-Type header from response */
  contentType: string | null;
  /** Response body as Uint8Array */
  body: Uint8Array;
  /** Get response body as text */
  text(): string;
}
/**
 * Options for asset download
 * @category HTTP
 */
interface DownloadOptions {
  /** Timeout in milliseconds (default: 30000) */
  timeoutMs?: number;
}
/**
 * Result from an asset download operation
 * @category HTTP
 */
interface DownloadResult {
  /** HTTP status code */
  status: number;
  /** Whether the request was successful (2xx status) */
  ok: boolean;
  /** Content-Type header from response */
  contentType: string | null;
  /** Number of bytes written to disk */
  bytesWritten: number;
  /** Actual path where file was saved (relative to project) */
  actualPath: string;
}
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
declare function fetchUrl(url: string, options?: FetchOptions): Promise<FetchResult>;
/**
 * Options for HTTP POST requests
 * @category HTTP
 */
interface PostOptions {
  /** Timeout in milliseconds (default: 30000) */
  timeoutMs?: number;
  /** Additional headers */
  headers?: Record<string, string>;
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
declare function httpPost(url: string, body: Record<string, unknown>, options?: PostOptions): Promise<FetchResult>;
/**
 * Convert an HTML fragment to Markdown via moss's bundled `htmd` converter —
 * the same converter the rest of the app uses. Plugins call this instead of
 * shipping their own HTML→Markdown pass, so output (notably hard breaks, which
 * htmd renders as two trailing spaces rather than a lone backslash) is
 * consistent app-wide. Returns the input HTML unchanged if conversion fails.
 * @category HTTP
 */
declare function htmlToMarkdown(html: string): Promise<string>;
/**
 * Options for HTTP GET requests
 * @category HTTP
 */
interface GetOptions {
  /** Timeout in milliseconds (default: 30000) */
  timeoutMs?: number;
  /** Additional headers */
  headers?: Record<string, string>;
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
declare function httpGet(url: string, options?: GetOptions): Promise<FetchResult>;
/**
 * One ordered text field in a multipart/form-data POST.
 *
 * Order is preserved because the GraphQL multipart request spec requires
 * `operations` before `map` before the file parts.
 * @category HTTP
 */
interface MultipartTextField {
  /** Form field name (e.g. "operations", "map"). */
  name: string;
  /** Field value (e.g. the JSON-encoded GraphQL operation). */
  value: string;
}
/**
 * One file part in a multipart/form-data POST. Bytes are passed base64-encoded —
 * e.g. straight from `readSiteFile`, which already returns base64.
 * @category HTTP
 */
interface MultipartFilePart {
  /** Form field name for this file (e.g. "0" per the GraphQL multipart spec). */
  field: string;
  /** File name reported in the part's Content-Disposition. */
  filename: string;
  /** MIME type for the part's Content-Type header. */
  contentType: string;
  /** File contents, base64-encoded. */
  contentBase64: string;
}
/**
 * Options for a multipart POST request.
 * @category HTTP
 */
interface MultipartPostOptions {
  /** Timeout in milliseconds (default: 30000) */
  timeoutMs?: number;
  /** Additional headers (Content-Type is set automatically and cannot be overridden) */
  headers?: Record<string, string>;
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
declare function httpPostMultipart(url: string, parts: {
  textFields?: MultipartTextField[];
  files?: MultipartFilePart[];
}, options?: MultipartPostOptions): Promise<FetchResult>;
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
declare function downloadAsset(url: string, targetDir: string, options?: DownloadOptions): Promise<DownloadResult>;
//#endregion
//#region src/utils/binary.d.ts
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
 * Options for executing a binary
 * @category Binary execution
 */
interface ExecuteOptions {
  /** Path to the binary (can be just the name if in PATH) */
  binaryPath: string;
  /** Arguments to pass to the binary */
  args: string[];
  /** Timeout in milliseconds (default: 60000) */
  timeoutMs?: number;
  /** Additional environment variables */
  env?: Record<string, string>;
  /** Data to pass to stdin (useful for commands like `git credential fill`) */
  stdin?: string;
  /** Working directory relative to project root (default: project root itself) */
  workingDir?: string;
  /**
   * Callback for real-time stderr output. When provided, stderr lines are
   * streamed from the Rust backend via Tauri events as they are produced.
   * Useful for long-running processes like `git push` where you want to
   * show progress to the user.
   */
  onStderr?: (line: string) => void;
}
/**
 * Result from binary execution
 * @category Binary execution
 */
interface ExecuteResult {
  /** Whether the command succeeded (exit code 0) */
  success: boolean;
  /** Exit code from the process */
  exitCode: number;
  /** Standard output from the process */
  stdout: string;
  /** Standard error output from the process */
  stderr: string;
}
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
declare function executeBinary(options: ExecuteOptions): Promise<ExecuteResult>;
//#endregion
//#region src/utils/platform.d.ts
/**
 * Platform detection utilities for moss plugins
 *
 * Detects the current operating system and architecture to enable
 * platform-specific binary downloads and operations.
 */
/**
 * Supported operating systems
 * @category Platform
 */
type OSType = "darwin" | "linux" | "windows";
/**
 * Supported architectures
 * @category Platform
 */
type ArchType = "arm64" | "x64";
/**
 * Platform key combining OS and architecture
 * @category Platform
 */
type PlatformKey = "darwin-arm64" | "darwin-x64" | "linux-x64" | "windows-x64";
/**
 * Complete platform information
 * @category Platform
 */
interface PlatformInfo {
  /** Operating system */
  os: OSType;
  /** CPU architecture */
  arch: ArchType;
  /** Combined platform key for binary selection */
  platformKey: PlatformKey;
}
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
declare function getPlatformInfo(): Promise<PlatformInfo>;
//#endregion
//#region src/utils/cookies.d.ts
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
 * A cookie stored for plugin authentication
 *
 * @category Cookies
 */
interface Cookie {
  /** Cookie name */
  name: string;
  /** Cookie value */
  value: string;
  /** Optional domain for the cookie */
  domain?: string;
  /** Optional path for the cookie */
  path?: string;
}
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
declare function getPluginCookie(): Promise<Cookie[] | null>;
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
declare function setPluginCookie(cookies: Cookie[]): Promise<void>;
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
declare function clearPluginCookies(): Promise<void>;
//#endregion
//#region src/utils/events.d.ts
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
declare function emitEvent(event: string, payload?: unknown): Promise<void>;
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
declare function onEvent<T>(event: string, handler: (payload: T) => void): Promise<() => void>;
//#endregion
//#region src/utils/toast.d.ts
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
 * Toast variant determines the visual style
 * @category Toast
 */
type ToastVariant = "success" | "error" | "info" | "warning";
/**
 * Action button in a toast
 * @category Toast
 */
interface ToastAction {
  /** Button text, e.g., "View site" */
  label: string;
  /** URL to open in system browser when clicked */
  url: string;
}
/**
 * Options for showing a toast notification
 *
 * @example
 * ```typescript
 * // Simple success toast
 * showToast({ message: "Saved!" });
 *
 * // Toast with action
 * showToast({
 *   message: "Deployed!",
 *   variant: "success",
 *   actions: [{ label: "View site", url: "https://..." }],
 *   duration: 8000
 * });
 *
 * // Persistent progress toast
 * showToast({
 *   message: "Deploying...",
 *   variant: "info",
 *   id: "deploy-progress",
 *   persistent: true,
 *   dismissible: false
 * });
 * ```
 * @category Toast
 */
interface ToastOptions {
  /** The message to display (required) */
  message: string;
  /** Visual style hint (default: "info") */
  variant?: ToastVariant;
  /** Action buttons (opens URL in system browser) */
  actions?: ToastAction[];
  /** Duration in ms before auto-dismiss (bounded by frontend to 2000-30000) */
  duration?: number;
  /** If true, toast stays until user dismisses or plugin calls dismissToast() */
  persistent?: boolean;
  /** If false, hide the X close button (default: true) */
  dismissible?: boolean;
  /** Unique ID for update-in-place pattern (used with updateToast/dismissToast) */
  id?: string;
}
/**
 * @deprecated Use ToastVariant instead
 * @category Toast
 */
type ToastType = ToastVariant;
/**
 * Event for showing a new toast
 * @category Toast
 */
declare const TOAST_EVENT = "show-toast";
/**
 * Event for updating an existing toast by ID
 * @category Toast
 */
declare const TOAST_UPDATE_EVENT = "show-toast-update";
/**
 * Event for dismissing a toast by ID
 * @category Toast
 */
declare const TOAST_DISMISS_EVENT = "show-toast-dismiss";
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
declare function showToast(options: ToastOptions | string): Promise<void>;
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
declare function dismissToast(id: string): Promise<void>;
//#endregion
export { AddressKind, AdvisoryAction, AdvisoryAppOp, AdvisoryProposal, AdvisoryScope, AdvisorySeverity, ArchType, ArticleInfo, BaseContext, BrowserCloseReason, BrowserHandle, CompleteMessage, ConfigureDomainContext, Cookie, DeployAddress, DeployContext, DeploymentInfo, DnsRecord, DnsTarget, DownloadOptions, DownloadResult, ErrorMessage, EscapeSpec, ExecuteOptions, ExecuteResult, FetchOptions, FetchResult, GetOptions, HookResult, HookToast, KeyAlgorithm, KeyInfo, LegacySetupVerdict, LogMessage, MultipartFilePart, MultipartPostOptions, MultipartTextField, OSType, PlatformInfo, PlatformKey, PluginCategory, PluginHook, PluginManifest, PluginMessage, PostOptions, ProcessContext, ProgressMessage, ProjectFileEntry, ProjectInfo, SetupBlocker, SetupContext, SetupField, SetupForm, SetupVerdict, SiteFileInfo, SocialArticleData, SocialComment, SocialDataFile, StartTaskOptions, SyndicateContext, TOAST_DISMISS_EVENT, TOAST_EVENT, TOAST_UPDATE_EVENT, TaskHandle, TauriCore, ToastAction, ToastOptions, ToastType, ToastVariant, TriggerContext, clearPluginCookies, closeBrowser, dismissToast, downloadAsset, emitEvent, executeBinary, fetchUrl, fileExists, getKey, getPlatformInfo, getPluginCookie, getPluginEnvVar, getSecret, getTauriCore, htmlToMarkdown, httpGet, httpPost, httpPostMultipart, listFiles, listKeys, listProjectTree, listSiteFilesWithSizes, onEvent, openBrowser, openBrowserWithHtml, openSystemBrowser, pluginFileExists, readFile, readPluginFile, readSiteFile, rejectSecret, reportError, reportProgress, returnToEditor, sendMessage, setMessageContext, setPluginCookie, setSecret, showBrowserForm, showToast, signWithKey, startTask, writeFile, writePluginFile };