/**
 * EditorPanelView — PanelView implementation for the editor.
 *
 * Opens the editor beside the preview through the host's `editorPanel`.
 * The editor webview has its own lifecycle (editor-main.ts) and handles
 * auto-save internally with a 2-second debounce.
 */

import type { Takeover } from '@symbiosis-lab/moss-ui';
import { host } from "../host";
import { error, debug } from "../log";
import type { PanelView } from './panel-view';
import { HERO_WORDS } from './session/instrument-events';

export interface EditorPanelViewOptions {
  /** Called when mount fails (openEditor rejects). Allows the caller to revert UI state. */
  onMountError?: (err: unknown) => void;
}

export class EditorPanelView implements PanelView {
  private onMountError?: (err: unknown) => void;
  private sourcePath: string | null = null;
  private isRoot = false;
  private takeover: Takeover | null = null;

  constructor(options?: EditorPanelViewOptions) {
    this.onMountError = options?.onMountError;
  }

  /** Set the resolved node before calling mount(). */
  setNode(node: { sourcePath: string | null; isRoot: boolean; takeover: Takeover | null }): void {
    this.sourcePath = node.sourcePath;
    this.isRoot = node.isRoot;
    this.takeover = node.takeover;
  }

  /** Get the current source file path (null if none). */
  getSourcePath(): string | null {
    return this.sourcePath;
  }

  /**
   * Open the editor webview in the action-panel slot.
   * The resolved node (source/isRoot/takeover) is passed to the backend
   * and baked into the editor URL so the editor can render the correct
   * surface immediately on init — no event race.
   */
  mount(): void {
    debug('[EditorPanelView] Mounting editor');
    host.editorPanel?.open(this.sourcePath, this.isRoot, this.takeover).catch((err) => {
      error(`[EditorPanelView] Failed to open editor: ${err instanceof Error ? err.message : String(err)}`);
      this.onMountError?.(err);
    });
  }

  /**
   * Close the editor panel: flush, then HIDE
   * (close_editor_panel), not a webview destroy — the warm panel survives
   * for the next open. Destruction happens only in workspace teardown or
   * when a plugin claims the slot (Rust-side, unchanged this phase).
   */
  unmount(): void {
    debug('[EditorPanelView] Unmounting editor (flush-then-hide)');
    host.bus.emit(HERO_WORDS, { line: null });
    host.editorPanel?.close().catch(() => {
      // Ignore — panel may already be hidden/closed
    });
  }

  /**
   * The editor uses auto-save with a 2-second debounce (editor-main.ts).
   * Content is persisted automatically, so data loss on switch is minimal.
   * Cross-webview state queries are not implemented to keep things simple.
   */
  hasUnsavedChanges(): boolean {
    return false;
  }
}
