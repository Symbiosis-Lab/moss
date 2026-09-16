/**
 * Internal context utilities for moss plugins
 *
 * This module provides access to the plugin execution context that is
 * set by the plugin runtime before each hook execution.
 *
 * INTERNAL USE ONLY - not exported to plugins.
 * Plugins should use the higher-level APIs (readFile, writeFile, etc.)
 * which use this context internally.
 */

// ============================================================================
// Types
// ============================================================================

/**
 * Internal plugin execution context
 *
 * Set by the plugin runtime (window.__MOSS_INTERNAL_CONTEXT__)
 * before each hook execution. Cleared after hook completes.
 */
export interface InternalPluginContext {
  /** Plugin name from manifest */
  plugin_name: string;
  /** Absolute path to the project directory */
  project_path: string;
  /** Absolute path to the .moss directory */
  moss_dir: string;
}

/**
 * Window interface with internal context
 */
interface ContextWindow {
  __MOSS_INTERNAL_CONTEXT__?: InternalPluginContext;
}

// ============================================================================
// Functions
// ============================================================================

/**
 * Get the internal plugin context
 *
 * This is used internally by moss-api utilities to resolve paths
 * and plugin identity. Plugins should not call this directly.
 *
 * @returns The current plugin execution context
 * @throws Error if called outside of a plugin hook execution
 *
 * @internal
 */
export function getInternalContext(): InternalPluginContext {
  const context = (window as unknown as ContextWindow).__MOSS_INTERNAL_CONTEXT__;

  if (!context) {
    throw new Error(
      "This function must be called from within a plugin hook. " +
        "Ensure you're calling this from process(), generate(), deploy(), or syndicate()."
    );
  }

  return context;
}

/**
 * Check if we're currently inside a plugin hook execution
 *
 * @returns true if inside a hook, false otherwise
 *
 * @internal
 */
export function hasContext(): boolean {
  const context = (window as unknown as ContextWindow).__MOSS_INTERNAL_CONTEXT__;
  return context !== undefined;
}
