/**
 * PanelView interface — the contract for views that appear in the action panel.
 *
 * The action panel is a separate Tauri webview (not part of the main window's
 * DOM). Views open their webview via Rust commands in mount() and close it in
 * unmount(). One view at a time, no tabs, no history stack.
 *
 * The ActionPanelManager switches between views by calling unmount() on the
 * current view and mount() on the new one. Before switching, it checks
 * hasUnsavedChanges() to prevent accidental data loss.
 */
export interface PanelView {
  /** Open the view's webview in the action-panel slot. */
  mount(): void;

  /** Tear down everything: close webview, clean up event listeners. */
  unmount(): void;

  /**
   * Check whether the view has unsaved work.
   * If not implemented, the manager assumes no unsaved changes.
   */
  hasUnsavedChanges?(): boolean;

  /**
   * Persist pending work in the view's webview and resolve once it is saved.
   * The manager awaits it before `unmount()`, because unmount destroys the
   * webview and a fire-and-forget flush loses the race. Views whose unmount
   * already flushes on the Rust side (the editor: `close_editor_panel`) omit it.
   */
  flush?(): Promise<void>;
}
