/**
 * Navigation History Manager (Electron-compatible API)
 *
 * Implements a subset of Electron's NavigationHistory API for tracking
 * iframe navigation state. This enables proper back/forward button management.
 *
 * Why track our own history?
 * - Browser's history.length doesn't indicate current position in stack
 * - No native API to check if forward and backward navigation is available
 *
 * How it works:
 * - Maintain _stack[] of visited URLs and _index of current position
 * - New navigation: truncate forward entries, push new URL, increment index
 * - Back: decrement index (forward entries preserved)
 * - Forward: increment index
 *
 * Note: This runs in the parent window (NavigationManager), NOT in the iframe,
 * because JavaScript globals don't persist across iframe page navigations.
 */

export interface NavigationHistory {
  /** Internal: array of visited URLs */
  _stack: string[];
  /** Internal: current position in stack */
  _index: number;
  /** Internal: whether history has been initialized with a real URL */
  _initialized: boolean;

  /** Electron API: Returns whether the browser can go back to previous web page */
  canGoBack(): boolean;
  /** Electron API: Returns whether the browser can go forward to next web page */
  canGoForward(): boolean;
  /** Electron API: Returns the number of entries in the navigation history */
  length(): number;
  /** Electron API: Returns the index of the current page in the navigation history */
  getActiveIndex(): number;
  /** Returns the current URL */
  getCurrentUrl(): string;
  /** Internal: set the initial URL (first navigation, not a "new" navigation) */
  _setInitialUrl(url: string): void;
  /** Internal: called when new navigation occurs (not back/forward) */
  _onNewNavigation(url: string): void;
}

/**
 * Create a new NavigationHistory instance.
 *
 * The history starts uninitialized (empty stack). Call _setInitialUrl() with
 * the first real URL to initialize it, then use _onNewNavigation() for
 * subsequent navigations.
 *
 * @returns A NavigationHistory object implementing Electron-compatible API
 */
export function createNavigationHistory(): NavigationHistory {
  return {
    _stack: [],
    _index: -1,
    _initialized: false,

    canGoBack(): boolean {
      return this._index > 0;
    },

    canGoForward(): boolean {
      return this._index < this._stack.length - 1;
    },

    length(): number {
      return this._stack.length;
    },

    getActiveIndex(): number {
      return this._index;
    },

    getCurrentUrl(): string {
      return this._stack[this._index] ?? "";
    },

    _setInitialUrl(url: string): void {
      this._stack = [url];
      this._index = 0;
      this._initialized = true;
    },

    _onNewNavigation(url: string): void {
      // Truncate any forward history entries
      this._stack = this._stack.slice(0, this._index + 1);
      this._stack.push(url);
      this._index++;
    },
  };
}
