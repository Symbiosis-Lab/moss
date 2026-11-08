/**
 * ActionPanelManager — one-view-at-a-time manager for the action panel.
 *
 * The action panel is a single-slot container on the left side of the window.
 * Only one PanelView can be active at a time. No tabs, no history stack.
 *
 * Switching views:
 * 1. Check hasUnsavedChanges() on the current view
 * 2. If unsaved: prompt user to confirm (cancel aborts the switch)
 * 3. unmount() current view
 * 4. mount() new view
 *
 * Closing the panel: unmount current, done.
 */

import { debug, uiEvent } from "../log";
import { mossConfirm } from "../shared/ui/moss-alert";
import type { PanelView } from './panel-view';
import { t } from "../i18n/index";

export class ActionPanelManager {
  private currentView: PanelView | null = null;

  /**
   * Switch to a new view. If a view is already active, it is unmounted first.
   * Resolves true if the switch succeeded, false if cancelled by the user
   * (e.g., due to unsaved changes).
   *
   * Async because the confirm is: a moss alert (moss-alert.ts) resolves a
   * Promise<boolean>, never a synchronous `window.confirm`.
   */
  async switchTo(view: PanelView): Promise<boolean> {
    // If the same view is already active, do nothing
    if (this.currentView === view) {
      return true;
    }

    // Check for unsaved changes in the current view
    if (this.currentView?.hasUnsavedChanges?.()) {
      // Destructive: discarding in-editor changes has no undo and no trash.
      const proceed = await mossConfirm(t('panel.unsaved_switch'), {
        confirmLabel: t('panel.discard'),
        destructive: true,
      });
      if (!proceed) {
        // Production filters DEBUG out, and "the user clicked something and it
        // silently did nothing" is the gap this event exists to show.
        uiEvent('panel.switch-refused', {
          reason: 'unsaved-changes',
          from: this.currentView.constructor.name,
          to: view.constructor.name,
        });
        return false;
      }
    }

    // Unmount current view
    if (this.currentView) {
      // Only views that declare a flush are awaited: an extra microtask here
      // would let a racing caller observe the old currentView.
      if (this.currentView.flush) await this.flushCurrent();
      debug('[ActionPanelManager] Unmounting current view');
      this.currentView.unmount();
    }

    // Mount new view
    const fromView = this.currentView ? this.currentView.constructor.name : 'none';
    this.currentView = view;
    debug('[ActionPanelManager] Mounting new view');
    // `constructor.name` (not a dedicated PanelView.name field — no
    // implementer carries one, and adding one is a bigger interface change
    // than a diagnostic log needs): survives moss's own build, which does
    // not rename classes, and at worst degrades to a less-specific label if
    // that ever changes.
    uiEvent('panel.switch', { outcome: 'mounted', from: fromView, to: view.constructor.name });
    view.mount();
    return true;
  }

  /**
   * Close the panel by unmounting the current view.
   * Does nothing if no view is active.
   * Resolves true if the panel was closed, false if cancelled (unsaved
   * changes). Async for the reason given on `switchTo`.
   */
  async close(): Promise<boolean> {
    if (!this.currentView) return true;

    if (this.currentView.hasUnsavedChanges?.()) {
      const proceed = await mossConfirm(t('panel.unsaved_close'), {
        confirmLabel: t('panel.discard'),
        destructive: true,
      });
      if (!proceed) {
        debug('[ActionPanelManager] Close cancelled — unsaved changes');
        return false;
      }
    }

    debug('[ActionPanelManager] Closing panel');
    if (this.currentView.flush) await this.flushCurrent();
    this.currentView.unmount();
    this.currentView = null;
    return true;
  }

  /** Let the current view save pending work before its webview is destroyed. */
  private async flushCurrent(): Promise<void> {
    const view = this.currentView;
    try {
      await view?.flush?.();
    } catch {
      // A view that cannot flush must not trap the user in the panel.
    }
  }

  /**
   * Get the currently active view, or null if the panel is closed.
   */
  getCurrentView(): PanelView | null {
    return this.currentView;
  }

  /**
   * Clear the current view reference without calling unmount().
   *
   * Used when the panel is closed externally (e.g., by the Rust backend
   * emitting 'action-panel-closed'). The webview is already gone, so
   * calling unmount() would be redundant.
   */
  clearCurrentView(): void {
    this.currentView = null;
  }

  /**
   * Restore the current-view mirror WITHOUT calling mount().
   *
   * Used when an action-panel webview is opened OUTSIDE `switchTo` — e.g. the
   * matters plugin (or any moss-api caller) opens the editor via the Rust
   * `open_editor`/`open_action_panel` path directly. In that case the webview
   * exists but `currentView` is still null (it was cleared by the preceding
   * `ActionPanelClosed`), so `editor:load-source` and other
   * `getCurrentView() === editorPanelView` gates silently fail. The
   * `ActionPanelOpened` handler calls this to re-sync the mirror to the live
   * webview, restoring the invariant `currentView ⟺ open panel`.
   *
   * Idempotent: a no-op when `view` is already current. Does NOT mount/unmount —
   * the webview is already live; this only fixes the bookkeeping.
   */
  setCurrentView(view: PanelView): void {
    this.currentView = view;
  }
}
