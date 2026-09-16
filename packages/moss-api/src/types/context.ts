/**
 * Hook context types - data provided to plugins during hook execution
 *
 * Note: Paths (project_path, moss_dir, output_dir) are NOT included here.
 * Plugins should use the filesystem APIs (readFile, writeFile, etc.)
 * and plugin storage APIs (readPluginFile, writePluginFile, etc.)
 * which automatically resolve paths from the internal context.
 */

import type { ProjectInfo } from "./plugin.js";
// `import type` is erased at compile time, so this does not create a runtime cycle.
import type { TriggerContext } from "../utils/messaging.js";

/**
 * Base context shared by all hooks
 *
 * Contains only business data - no paths.
 * Use readFile(), writeFile() for project files.
 * Use readPluginFile(), writePluginFile() for plugin storage.
 *
 * @category Hook contexts
 */
export interface BaseContext {
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
export interface ProcessContext extends BaseContext {
  trigger?: TriggerContext;
}

/**
 * Context for on_deploy hook (deployer plugins)
 *
 * @category Hook contexts
 */
export interface DeployContext extends BaseContext {
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
export interface ConfigureDomainContext extends BaseContext {
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
export interface SetupContext {
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
export interface SyndicateContext extends BaseContext {
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
export interface ArticleInfo {
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
export interface DeploymentInfo {
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
export type AddressKind = "cid" | "ipns" | "gateway" | "domain";

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
export interface DeployAddress {
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
export interface DnsRecord {
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
export interface DnsTarget {
  /** List of DNS records to configure */
  records: DnsRecord[];
}
