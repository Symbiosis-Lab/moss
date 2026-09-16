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
 * A node in the universal Page Tree.
 *
 * Every markdown file and every folder produces one PageNode.
 * Folders have is_folder=true and may have children.
 * This is the universal intermediate representation consumed by
 * both the built-in generator and SSG plugins.
 *
 * @category Hook contexts
 */
export interface PageNode {
  /** Relative source path (e.g., "articles/hello.md" or "articles") */
  source_path: string;
  /** Output URL path (e.g., "articles/hello.html") */
  url_path: string;
  /** Page title (from frontmatter, H1, or filename) */
  title: string;
  /** URL-safe slug */
  slug: string;
  /** Rendered HTML content (empty string for auto-generated folder pages) */
  content_html: string;

  /** Whether this node represents a folder */
  is_folder: boolean;
  /** Child nodes (populated for folders) */
  children: PageNode[];

  /** Publication date (ISO string) */
  date?: string;
  /** Cover image path */
  cover?: string;
  /** Whether this page appears in header navigation */
  nav: boolean;
  /** Navigation ordering (lower = first) */
  nav_weight?: number;
  /** Whether this page is a draft — rendered and published at its direct URL but hidden from listings, feeds, and navigation. */
  draft: boolean;
  /** Recursively list all nested content */
  flatten: boolean;
  /** How children display: "list", "grid", or "sidebar" */
  list_style: "list" | "grid" | "sidebar";
  /** Folder paths where this article also appears in child lists */
  also_in: string[];

  /** Raw frontmatter for plugin-specific fields */
  frontmatter: Record<string, unknown>;
}

/**
 * Context for on_build hook (generator plugins)
 *
 * @category Hook contexts
 */
export interface GenerateContext extends BaseContext {
  source_files: SourceFiles;
  /** Resolved Page Tree — universal content model for all generators */
  page_tree?: PageNode;
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
 * Context for after_deploy hook (syndicator plugins)
 *
 * @category Hook contexts
 */
export interface SyndicateContext extends BaseContext {
  site_files: string[];
  articles: ArticleInfo[];
  deployment?: DeploymentInfo;
}

/**
 * Source files categorized by type
 *
 * @category Hook contexts
 */
export interface SourceFiles {
  markdown: string[];
  pages: string[];
  docx: string[];
  other: string[];
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
