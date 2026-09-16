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
export interface ProjectInfo {
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
export interface PluginManifest {
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
export type PluginCategory =
  | "generator"
  | "deployer"
  | "syndicator"
  | "enhancer"
  | "processor";
