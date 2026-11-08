// Panel Manager - Manages the right control panel visibility

import { warn } from "../log";

export class PanelManager {
  private panel: HTMLElement;
  private panelShadow: HTMLElement | null = null;

  constructor(
    panelId: string = "moss-panel",
    paletteId: string = "command-palette",
    backdropId: string = "command-backdrop"
  ) {
    const panel = document.getElementById(panelId);
    if (!panel) throw new Error(`Panel element '${panelId}' not found`);

    this.panel = panel;
    this.panelShadow = document.getElementById("moss-panel-shadow");
  }

  // Panel visibility management
  showPanel(): void {
    this.panel.classList.remove("hidden");
    this.panelShadow?.classList.remove("hidden");
  }

  hidePanel(): void {
    this.panel.classList.add("hidden");
    this.panelShadow?.classList.add("hidden");
  }

  togglePanel(): void {
    if (this.isPanelVisible()) {
      this.hidePanel();
    } else {
      this.showPanel();
    }
  }

  isPanelVisible(): boolean {
    return !this.panel.classList.contains("hidden");
  }

  // Empty implementations for compatibility with existing code
  showCommandPalette(): void {
    // Command palette functionality removed
  }

  hideCommandPalette(): void {
    // Command palette functionality removed
  }

  toggleCommandPalette(): void {
    // Command palette functionality removed
  }

  isCommandPaletteVisible(): boolean {
    return false;
  }

  executeCommand(action: string): void {
    warn(`⚠️ [PanelManager] Unknown command: ${action}`);
  }

  // Public API for compatibility
  getActionHandlers(): any {
    return {
      execute: (action: string) => this.executeCommand(action),
    };
  }

  getCommandRegistry(): any {
    return {
      getAll: () => [],
    };
  }

  getKeyboardManager(): any {
    return {
      // Empty keyboard manager for compatibility
    };
  }
}
