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
   * Root folder basename (e.g. "My Site"). Plugins that generate a folder home
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
  /** Omit only for a plugin that declares nothing but `contributes.derivations`. */
  entry?: string;
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

/**
 * A pinned binary source, as a stack declares it. A `download` source must
 * carry a `sha256`.
 *
 * @category Plugin manifest
 */
export type ArtifactSource =
  | { kind: "path"; platform?: string; binary: string }
  | {
      kind: "download";
      platform?: string;
      url: string;
      sha256: string;
      archive_format: string;
      executable?: string;
    };

/**
 * `contributes.derivations[id]`: a file type the plugin turns into embeddable
 * fragments by running a pinned one-shot binary.
 *
 * @category Plugin manifest
 */
export interface DerivationContribution {
  /** Lowercase file extensions without the dot. */
  accepts: string[];
  noun: { one: string; other: string };
  /** Fragments the binary may produce, by name. Only "markdown" exists. */
  fragments: Record<string, "markdown">;
  /** Placeholder lines: each is empty or `#<fragment name>`. */
  stub?: string[];
  run: { binary: string; artifact: ArtifactSource[] };
}
