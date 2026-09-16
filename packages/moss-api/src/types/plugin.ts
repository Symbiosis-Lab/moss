/**
 * Base plugin types shared across all moss plugins
 */

/** @category Plugin manifest */
export interface ProjectInfo {
  project_type: string;
  content_folders: string[];
  total_files: number;
  homepage_file?: string;
  /**
   * Root folder basename (e.g. "刘果"). Plugins that generate a folder home
   * should name it self-named (`<folder_name>.md`) with a `home: true` marker
   * to match moss's folder-home convention.
   */
  folder_name?: string;
  site_name?: string;
  lang?: string;
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
