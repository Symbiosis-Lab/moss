/**
 * Types for the content slots system.
 *
 * Plugins declare what content to inject and where using these types.
 * Rust handles insertion during template rendering.
 */

/**
 * Context passed to a plugin's `enhance()` method.
 * @category Enhance
 */
export interface EnhanceContext {
  project_path: string;
  moss_dir: string;
  output_dir: string;
  config: Record<string, unknown>;
  project_info: { site_name: string; lang: string };
  article_map: Record<string, unknown>;
}

/**
 * Content declaration for a single slot.
 * @category Enhance
 */
export type EnhanceContent =
  | { type: "static"; html: string }
  | { type: "per-page"; pages: Record<string, string> };

/**
 * Result returned from a plugin's `enhance()` call.
 * @category Enhance
 */
export interface EnhanceResult {
  success: boolean;
  slots: Record<string, EnhanceContent>;
}
