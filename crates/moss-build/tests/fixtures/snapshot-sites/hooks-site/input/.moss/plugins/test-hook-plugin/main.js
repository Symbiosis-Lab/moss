/**
 * Test Hook Plugin
 *
 * This plugin creates a marker file when the process hook is called,
 * allowing tests to verify that plugin hooks are being executed.
 *
 * Uses __MOSS_INTERNAL_CONTEXT__ for path info and Tauri invoke for file writing,
 * matching how the moss-api SDK works internally.
 */

window.TestHookPlugin = {
  /**
   * Process hook - creates a marker file to signal hook execution
   * @param {Object} context - The ProcessContext from Moss (sanitized, no paths)
   * @returns {Object} Hook result
   */
  process: async function(context) {
    try {
      console.log('[TestHookPlugin] process hook called');

      // Get path info from internal context (set by plugin runtime before hook execution)
      const internalCtx = window.__MOSS_INTERNAL_CONTEXT__;
      if (!internalCtx) {
        throw new Error('Internal context not available');
      }

      console.log('[TestHookPlugin] Project path:', internalCtx.project_path);

      // Write marker file using Tauri invoke (same as moss-api SDK)
      const { invoke } = window.__TAURI__.core;

      const timestamp = new Date().toISOString();
      const content = `process hook executed at ${timestamp}\nproject_path: ${internalCtx.project_path}`;

      // Write the marker to the plugin's own private storage
      // (.moss/plugins/<plugin_name>/hook-executed.marker) via write_plugin_file.
      // This mirrors the moss-api SDK's writePluginFile() exactly: plugin_name and
      // project_path come from __MOSS_INTERNAL_CONTEXT__. The project-file door into
      // .moss/ was sealed in be19f4c79 (plugins may not read/write .moss/ — it holds
      // the identity secret and every plugin's manifest); plugin storage is the
      // sandbox-allowed path for a plugin's own files.
      await invoke('write_plugin_file', {
        pluginName: internalCtx.plugin_name,
        projectPath: internalCtx.project_path,
        relativePath: 'hook-executed.marker',
        content,
      });

      console.log('[TestHookPlugin] Marker file created');

      return {
        success: true,
        message: 'process hook executed successfully'
      };
    } catch (error) {
      console.error('[TestHookPlugin] Error in process hook:', error);
      return {
        success: false,
        message: 'process hook failed: ' + error.message
      };
    }
  }
};

console.log('[TestHookPlugin] Plugin loaded');
