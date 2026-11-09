import { describe, test, expect, beforeEach, afterEach, vi } from "vitest";
import { createNavigationHistory } from "../navigation-history";
import { engine } from "../../engine";

// Mocks for NavigationManager class (used in reload() tests)
vi.mock("../../log", () => ({
  uiEvent: vi.fn(),
  info: vi.fn(),
  warn: vi.fn(),
  error: vi.fn(),
  debug: vi.fn(),
}));
vi.mock('../../engine', () => ({
  engine: {
    resolvePageSource: vi.fn().mockResolvedValue({ status: "ok", data: { source_path: null, is_dir: false, is_article: false } }),
    listPages: vi.fn().mockResolvedValue({ status: "ok", data: [] }),
  },
}));
vi.mock("../../context/event-bus", () => ({
  eventBus: { emit: vi.fn() },
  Events: { PREVIEW_URL_CHANGED: "preview-url-changed" },
}));

import { NavigationManager, sameUrlPathname } from "../navigation-manager";
import { eventBus, Events } from "../../context/event-bus";
import { info } from "../../log";
import { host } from "../../host";

// No native swipe stream: the indicator falls back to the bridge's wheel heuristic.
host.swipePhases = async () => false;

/**
 * Tests for NavigationManager button state management.
 *
 * These tests verify that the back/forward buttons are correctly
 * enabled/disabled based on the canGoBack/canGoForward flags
 * received from the iframe's NavigationHistory API.
 */

interface NavigationMessage {
  type: "moss-navigation";
  url: string;
  title?: string;
  canGoBack?: boolean;
  canGoForward?: boolean;
}

/**
 * Mock button element for testing
 */
function createMockButton(): HTMLButtonElement {
  return {
    disabled: false,
    addEventListener: vi.fn(),
  } as unknown as HTMLButtonElement;
}

/**
 * Simulates the updateButtonStates logic from NavigationManager
 */
function updateButtonStates(
  backButton: HTMLButtonElement | null,
  forwardButton: HTMLButtonElement | null,
  canGoBack: boolean,
  canGoForward: boolean
): void {
  if (backButton) {
    backButton.disabled = !canGoBack;
  }
  if (forwardButton) {
    forwardButton.disabled = !canGoForward;
  }
}

describe("NavigationManager button states", () => {
  let backButton: HTMLButtonElement;
  let forwardButton: HTMLButtonElement;

  beforeEach(() => {
    backButton = createMockButton();
    forwardButton = createMockButton();
  });

  describe("initial state (fresh page load)", () => {
    test("both buttons disabled when canGoBack and canGoForward are false", () => {
      updateButtonStates(backButton, forwardButton, false, false);

      expect(backButton.disabled).toBe(true);
      expect(forwardButton.disabled).toBe(true);
    });
  });

  describe("after navigating to a new page", () => {
    test("back button enabled, forward button disabled", () => {
      updateButtonStates(backButton, forwardButton, true, false);

      expect(backButton.disabled).toBe(false);
      expect(forwardButton.disabled).toBe(true);
    });
  });

  describe("after going back", () => {
    test("back button disabled at root, forward button enabled", () => {
      updateButtonStates(backButton, forwardButton, false, true);

      expect(backButton.disabled).toBe(true);
      expect(forwardButton.disabled).toBe(false);
    });

    test("both buttons enabled when in middle of history", () => {
      updateButtonStates(backButton, forwardButton, true, true);

      expect(backButton.disabled).toBe(false);
      expect(forwardButton.disabled).toBe(false);
    });
  });

  describe("null button handling", () => {
    test("handles null back button gracefully", () => {
      expect(() => {
        updateButtonStates(null, forwardButton, true, false);
      }).not.toThrow();
      expect(forwardButton.disabled).toBe(true);
    });

    test("handles null forward button gracefully", () => {
      expect(() => {
        updateButtonStates(backButton, null, false, true);
      }).not.toThrow();
      expect(backButton.disabled).toBe(true);
    });

    test("handles both buttons null gracefully", () => {
      expect(() => {
        updateButtonStates(null, null, true, true);
      }).not.toThrow();
    });
  });

  describe("default values for undefined flags", () => {
    test("treats undefined canGoBack as false", () => {
      const data: NavigationMessage = {
        type: "moss-navigation",
        url: "http://localhost/",
        // canGoBack not set
      };
      const canGoBack = data.canGoBack ?? false;
      updateButtonStates(backButton, forwardButton, canGoBack, false);

      expect(backButton.disabled).toBe(true);
    });

    test("treats undefined canGoForward as false", () => {
      const data: NavigationMessage = {
        type: "moss-navigation",
        url: "http://localhost/",
        // canGoForward not set
      };
      const canGoForward = data.canGoForward ?? false;
      updateButtonStates(backButton, forwardButton, false, canGoForward);

      expect(forwardButton.disabled).toBe(true);
    });
  });
});

describe("getCurrentUrl", () => {
  // Test the getCurrentUrl logic using NavigationHistory directly
  // (NavigationManager.getCurrentUrl() delegates to history.getCurrentUrl())

  test("returns null when history is not initialized", () => {
    // Simulating NavigationManager.getCurrentUrl() before any navigation
    const history = createNavigationHistory();

    // getCurrentUrl() returns empty string when uninitialized
    const url = history.getCurrentUrl() || null;
    expect(url).toBe(null);
  });

  test("returns URL after initialization", () => {
    const history = createNavigationHistory();

    history._setInitialUrl("http://localhost:3000/index.html");

    expect(history.getCurrentUrl()).toBe("http://localhost:3000/index.html");
  });

  test("returns current URL after navigation", () => {
    const history = createNavigationHistory();

    history._setInitialUrl("http://localhost:3000/");
    history._onNewNavigation("http://localhost:3000/文章/post.html");

    expect(history.getCurrentUrl()).toBe("http://localhost:3000/文章/post.html");
  });

  test("returns correct URL after back navigation", () => {
    const history = createNavigationHistory();

    history._setInitialUrl("http://localhost:3000/");
    history._onNewNavigation("http://localhost:3000/page1.html");
    history._onNewNavigation("http://localhost:3000/page2.html");

    // Simulate back navigation (index--)
    history._index--;

    expect(history.getCurrentUrl()).toBe("http://localhost:3000/page1.html");
  });
});

describe("NavigationMessage interface", () => {
  test("accepts message with all fields", () => {
    const message: NavigationMessage = {
      type: "moss-navigation",
      url: "http://localhost/page",
      title: "Test Page",
      canGoBack: true,
      canGoForward: false,
    };

    expect(message.type).toBe("moss-navigation");
    expect(message.url).toBe("http://localhost/page");
    expect(message.title).toBe("Test Page");
    expect(message.canGoBack).toBe(true);
    expect(message.canGoForward).toBe(false);
  });

  test("accepts message with only required fields", () => {
    const message: NavigationMessage = {
      type: "moss-navigation",
      url: "http://localhost/",
    };

    expect(message.type).toBe("moss-navigation");
    expect(message.url).toBe("http://localhost/");
    expect(message.title).toBeUndefined();
    expect(message.canGoBack).toBeUndefined();
    expect(message.canGoForward).toBeUndefined();
  });
});

/**
 * Tests for RPC fallback navigation behavior.
 *
 * When the iframe shows non-HTML content (like RSS feeds), the iframe-bridge
 * script is not injected, so RPC calls will timeout. In this case, the
 * NavigationManager should fallback to direct iframe.src navigation.
 */
describe("RPC fallback navigation", () => {
  /**
   * Simulates the goBack fallback logic that should be implemented.
   * When RPC fails, it should update history and navigate directly.
   */
  function goBackWithFallback(
    history: ReturnType<typeof createNavigationHistory>,
    iframe: { src: string } | null,
    rpcFails: boolean
  ): { navigatedDirectly: boolean; newUrl: string | null } {
    if (!history.canGoBack()) {
      return { navigatedDirectly: false, newUrl: null };
    }

    if (rpcFails) {
      // Fallback: update history index and navigate directly
      history._index--;
      const prevUrl = history.getCurrentUrl();
      if (prevUrl && iframe) {
        iframe.src = prevUrl;
        return { navigatedDirectly: true, newUrl: prevUrl };
      }
    }

    return { navigatedDirectly: false, newUrl: null };
  }

  test("falls back to direct iframe.src navigation when RPC fails", () => {
    const history = createNavigationHistory();
    history._setInitialUrl("http://localhost:8080/");
    history._onNewNavigation("http://localhost:8080/feed.xml");

    const mockIframe = { src: "http://localhost:8080/feed.xml" };

    // Simulate RPC failure (e.g., timeout because no bridge script)
    const result = goBackWithFallback(history, mockIframe, true);

    expect(result.navigatedDirectly).toBe(true);
    expect(result.newUrl).toBe("http://localhost:8080/");
    expect(mockIframe.src).toBe("http://localhost:8080/");
  });

  test("updates history index when using fallback navigation", () => {
    const history = createNavigationHistory();
    history._setInitialUrl("http://localhost:8080/");
    history._onNewNavigation("http://localhost:8080/page1.html");
    history._onNewNavigation("http://localhost:8080/page2.html");

    const mockIframe = { src: "http://localhost:8080/page2.html" };

    // Before fallback: index is at page2
    expect(history.getCurrentUrl()).toBe("http://localhost:8080/page2.html");

    // Simulate RPC failure
    goBackWithFallback(history, mockIframe, true);

    // After fallback: index should be at page1
    expect(history.getCurrentUrl()).toBe("http://localhost:8080/page1.html");
  });

  test("does nothing when canGoBack is false", () => {
    const history = createNavigationHistory();
    history._setInitialUrl("http://localhost:8080/");

    const mockIframe = { src: "http://localhost:8080/" };

    const result = goBackWithFallback(history, mockIframe, true);

    expect(result.navigatedDirectly).toBe(false);
    expect(mockIframe.src).toBe("http://localhost:8080/");
  });

  test("handles null iframe gracefully", () => {
    const history = createNavigationHistory();
    history._setInitialUrl("http://localhost:8080/");
    history._onNewNavigation("http://localhost:8080/feed.xml");

    const result = goBackWithFallback(history, null, true);

    // Should not crash, but also not navigate
    expect(result.navigatedDirectly).toBe(false);
  });
});

/**
 * Tests for URL autocomplete integration.
 *
 * When the user double-clicks the URL text, an autocomplete dropdown
 * should appear allowing them to search and select a page to navigate to.
 */
/**
 * Tests for navigation history tracking when iframe-bridge sends navigation events.
 *
 * BUG: After user clicks a link in the iframe, back/forward buttons remain disabled.
 * Expected: After navigating from page A to page B, back button should be enabled.
 *
 * The issue is that handleNavigationUpdate receives events from two sources:
 * 1. iframe.onload event (handleIframeLoad)
 * 2. iframe-bridge postMessage (moss-navigation)
 *
 * Both can fire for the same navigation, and the order matters.
 */
describe("navigation history updates on link clicks", () => {
  test("back button enabled after navigating from homepage to article", () => {
    const history = createNavigationHistory();

    // Initial page load - homepage
    history._setInitialUrl("http://localhost:8080/");
    expect(history.canGoBack()).toBe(false);
    expect(history.canGoForward()).toBe(false);

    // User clicks link to article - this is a NEW navigation
    history._onNewNavigation("http://localhost:8080/posts/article.html");

    // Back should now be enabled
    expect(history.canGoBack()).toBe(true);
    expect(history.canGoForward()).toBe(false);
  });

  test("history correctly tracks multiple navigations", () => {
    const history = createNavigationHistory();

    // Page 1: homepage
    history._setInitialUrl("http://localhost:8080/");

    // Page 2: user clicks to article
    history._onNewNavigation("http://localhost:8080/posts/article.html");
    expect(history.canGoBack()).toBe(true);

    // Page 3: user clicks to another article
    history._onNewNavigation("http://localhost:8080/posts/another.html");
    expect(history.canGoBack()).toBe(true);
    expect(history.getActiveIndex()).toBe(2);
    expect(history.length()).toBe(3);
  });

  test("duplicate URL events should not double-count as navigation", () => {
    const history = createNavigationHistory();

    // Initial load
    history._setInitialUrl("http://localhost:8080/");

    // Same URL received again (e.g., from both onload and postMessage)
    // Should NOT create a new history entry
    const currentUrl = history.getCurrentUrl();
    if (currentUrl !== "http://localhost:8080/") {
      history._onNewNavigation("http://localhost:8080/");
    }

    // Should still have only 1 entry
    expect(history.length()).toBe(1);
    expect(history.canGoBack()).toBe(false);
  });

  /**
   * Simulates the handleNavigationUpdate logic from NavigationManager.
   * This is the critical function that receives navigation events.
   */
  function simulateHandleNavigationUpdate(
    history: ReturnType<typeof createNavigationHistory>,
    newUrl: string,
    pendingNavType: "back" | "forward" | null
  ): { canGoBack: boolean; canGoForward: boolean } {
    const currentUrl = newUrl;

    if (!history._initialized) {
      history._setInitialUrl(currentUrl);
    } else if (currentUrl !== history.getCurrentUrl()) {
      if (pendingNavType === "back") {
        history._index--;
      } else if (pendingNavType === "forward") {
        history._index++;
      } else {
        // New navigation (link click)
        history._onNewNavigation(currentUrl);
      }
    }

    return {
      canGoBack: history.canGoBack(),
      canGoForward: history.canGoForward(),
    };
  }

  test("handleNavigationUpdate enables back button after link click", () => {
    const history = createNavigationHistory();

    // First navigation event - homepage
    let state = simulateHandleNavigationUpdate(history, "http://localhost:8080/", null);
    expect(state.canGoBack).toBe(false);
    expect(state.canGoForward).toBe(false);

    // Second navigation event - user clicked a link
    state = simulateHandleNavigationUpdate(history, "http://localhost:8080/posts/article.html", null);
    expect(state.canGoBack).toBe(true);
    expect(state.canGoForward).toBe(false);
  });

  test("handleNavigationUpdate handles duplicate events correctly", () => {
    const history = createNavigationHistory();

    // Initial navigation
    simulateHandleNavigationUpdate(history, "http://localhost:8080/", null);

    // Navigate to article
    simulateHandleNavigationUpdate(history, "http://localhost:8080/posts/article.html", null);

    // Same URL received again (duplicate event) - should not create new entry
    const state = simulateHandleNavigationUpdate(history, "http://localhost:8080/posts/article.html", null);

    expect(history.length()).toBe(2);
    expect(state.canGoBack).toBe(true);
  });

  /**
   * Test the exact scenario that happens when user clicks a link:
   * 1. iframe load event fires with OLD iframe.src (programmatically set URL)
   * 2. moss-navigation message arrives with NEW location.href
   *
   * Expected: Back button should be enabled after moss-navigation
   */
  test("iframe load followed by moss-navigation updates history correctly", () => {
    const history = createNavigationHistory();

    // Initial page load - both handleIframeLoad and moss-navigation fire with same URL
    // (in practice, iframe.src equals location.href for the initial load)
    simulateHandleNavigationUpdate(history, "http://localhost:8080/", null);
    simulateHandleNavigationUpdate(history, "http://localhost:8080/", null); // duplicate from moss-navigation

    expect(history.length()).toBe(1);
    expect(history.canGoBack()).toBe(false);

    // User clicks a link to /posts/article.html
    // 1. iframe load event fires - but iframe.src is still the OLD URL
    simulateHandleNavigationUpdate(history, "http://localhost:8080/", null);

    // History should still be length 1 (no change yet)
    expect(history.length()).toBe(1);

    // 2. moss-navigation arrives with the NEW URL
    const state = simulateHandleNavigationUpdate(history, "http://localhost:8080/posts/article.html", null);

    // NOW history should have 2 entries and back should be enabled
    expect(history.length()).toBe(2);
    expect(state.canGoBack).toBe(true);
    expect(state.canGoForward).toBe(false);
  });

  /**
   * Test navigation with cache-busting query parameters.
   * The preview manager adds ?_t=timestamp to force reload.
   * moss-navigation sends the URL with the query param.
   */
  test("handles URLs with cache-busting query parameters", () => {
    const history = createNavigationHistory();

    // Initial load with cache-busting param
    simulateHandleNavigationUpdate(history, "http://localhost:8080/?_t=1234567890", null);
    expect(history.length()).toBe(1);

    // User clicks link (no cache-bust param in the new URL)
    const state = simulateHandleNavigationUpdate(history, "http://localhost:8080/posts/article.html", null);

    expect(history.length()).toBe(2);
    expect(state.canGoBack).toBe(true);
  });
});

describe("URL autocomplete integration", () => {
  // Mock elements for testing autocomplete behavior
  interface AutocompleteState {
    isOpen: boolean;
    urlTextElement: HTMLElement | null;
    autocompleteContainer: HTMLElement | null;
    currentPath: string;
  }

  /**
   * Simulates entering autocomplete mode
   */
  function enterAutocompleteMode(state: AutocompleteState): void {
    if (!state.urlTextElement) return;

    state.isOpen = true;
    // Hide the URL text
    state.urlTextElement.style.display = "none";

    // Show the autocomplete container (would be created by UrlAutocomplete)
    if (state.autocompleteContainer) {
      state.autocompleteContainer.style.display = "block";
    }
  }

  /**
   * Simulates exiting autocomplete mode (on cancel or select)
   */
  function exitAutocompleteMode(state: AutocompleteState): void {
    state.isOpen = false;
    // Show the URL text again
    if (state.urlTextElement) {
      state.urlTextElement.style.display = "";
    }

    // Hide the autocomplete container
    if (state.autocompleteContainer) {
      state.autocompleteContainer.style.display = "none";
    }
  }

  /**
   * Simulates handling a page selection from autocomplete
   */
  function handlePageSelection(
    iframe: { src: string } | null,
    baseUrl: string,
    selectedPath: string
  ): string | null {
    if (!iframe) return null;

    const newUrl = `${baseUrl}/${selectedPath}`;
    iframe.src = newUrl;
    return newUrl;
  }

  test("double-click on URL text enters autocomplete mode", () => {
    const state: AutocompleteState = {
      isOpen: false,
      urlTextElement: { style: { display: "" } } as HTMLElement,
      autocompleteContainer: { style: { display: "none" } } as HTMLElement,
      currentPath: "/posts/hello.html",
    };

    // Simulate double-click
    enterAutocompleteMode(state);

    expect(state.isOpen).toBe(true);
    expect(state.urlTextElement?.style.display).toBe("none");
    expect(state.autocompleteContainer?.style.display).toBe("block");
  });

  test("canceling autocomplete restores URL display", () => {
    const state: AutocompleteState = {
      isOpen: true,
      urlTextElement: { style: { display: "none" } } as HTMLElement,
      autocompleteContainer: { style: { display: "block" } } as HTMLElement,
      currentPath: "/posts/hello.html",
    };

    // Simulate cancel (Escape key)
    exitAutocompleteMode(state);

    expect(state.isOpen).toBe(false);
    expect(state.urlTextElement?.style.display).toBe("");
    expect(state.autocompleteContainer?.style.display).toBe("none");
  });

  test("selecting a page navigates iframe to selected URL", () => {
    const mockIframe = { src: "http://localhost:8080/" };
    const baseUrl = "http://localhost:8080";
    const selectedPath = "posts/new-article.html";

    const newUrl = handlePageSelection(mockIframe, baseUrl, selectedPath);

    expect(newUrl).toBe("http://localhost:8080/posts/new-article.html");
    expect(mockIframe.src).toBe("http://localhost:8080/posts/new-article.html");
  });

  test("selecting a page exits autocomplete mode", () => {
    const state: AutocompleteState = {
      isOpen: true,
      urlTextElement: { style: { display: "none" } } as HTMLElement,
      autocompleteContainer: { style: { display: "block" } } as HTMLElement,
      currentPath: "/",
    };

    // Simulate page selection
    exitAutocompleteMode(state);

    expect(state.isOpen).toBe(false);
    expect(state.urlTextElement?.style.display).toBe("");
  });

  test("handles null iframe gracefully on page selection", () => {
    const newUrl = handlePageSelection(null, "http://localhost:8080", "test.html");

    expect(newUrl).toBeNull();
  });

  test("handles null urlTextElement gracefully when entering autocomplete", () => {
    const state: AutocompleteState = {
      isOpen: false,
      urlTextElement: null,
      autocompleteContainer: { style: { display: "none" } } as HTMLElement,
      currentPath: "/",
    };

    // Should not throw
    expect(() => enterAutocompleteMode(state)).not.toThrow();
    // State should not change since urlTextElement is null
    expect(state.isOpen).toBe(false);
  });
});

/**
 * Tests for NavigationManager.reset() functionality.
 *
 * When switching between website folders (e.g., yi-website → guo-website),
 * the navigation history must be cleared to prevent stale state.
 *
 * BUG FIX: Previously, when switching folders:
 * - NavigationManager history persisted from old folder
 * - Display showed stale path until manual refresh
 * - Back/forward buttons had inconsistent state
 */
describe("NavigationManager reset()", () => {
  /**
   * Simulates the reset() method that should be added to NavigationManager.
   * This is what we're testing before implementing.
   */
  function simulateReset(
    history: ReturnType<typeof createNavigationHistory>,
    state: {
      currentPath: string;
      currentTitle: string;
      pendingNavType: "back" | "forward" | null;
    }
  ): {
    history: ReturnType<typeof createNavigationHistory>;
    currentPath: string;
    currentTitle: string;
    pendingNavType: "back" | "forward" | null;
  } {
    // Reset history to fresh state
    const newHistory = createNavigationHistory();

    return {
      history: newHistory,
      currentPath: "/",
      currentTitle: "",
      pendingNavType: null,
    };
  }

  test("clears navigation history when reset() is called", () => {
    const history = createNavigationHistory();

    // Setup: navigate to create history
    history._setInitialUrl("http://localhost:8080/");
    history._onNewNavigation("http://localhost:8080/page1.html");
    history._onNewNavigation("http://localhost:8080/page2.html");
    expect(history.length()).toBe(3);
    expect(history._initialized).toBe(true);

    // Act: simulate reset
    const state = {
      currentPath: "/page2.html",
      currentTitle: "Page 2",
      pendingNavType: null as "back" | "forward" | null,
    };
    const result = simulateReset(history, state);

    // Assert: history should be cleared
    expect(result.history.length()).toBe(0);
    expect(result.history._initialized).toBe(false);
  });

  test("resets path display to '/'", () => {
    const history = createNavigationHistory();
    history._setInitialUrl("http://localhost:8080/about");

    const state = {
      currentPath: "/about",
      currentTitle: "About",
      pendingNavType: null as "back" | "forward" | null,
    };

    const result = simulateReset(history, state);

    expect(result.currentPath).toBe("/");
  });

  test("clears current title", () => {
    const history = createNavigationHistory();
    history._setInitialUrl("http://localhost:8080/");

    const state = {
      currentPath: "/",
      currentTitle: "My Website",
      pendingNavType: null as "back" | "forward" | null,
    };

    const result = simulateReset(history, state);

    expect(result.currentTitle).toBe("");
  });

  test("clears pending navigation type", () => {
    const history = createNavigationHistory();
    history._setInitialUrl("http://localhost:8080/");

    const state = {
      currentPath: "/",
      currentTitle: "",
      pendingNavType: "back" as "back" | "forward" | null,
    };

    const result = simulateReset(history, state);

    expect(result.pendingNavType).toBe(null);
  });

  test("after reset, canGoBack returns false", () => {
    const history = createNavigationHistory();
    history._setInitialUrl("http://localhost:8080/");
    history._onNewNavigation("http://localhost:8080/page1.html");
    expect(history.canGoBack()).toBe(true);

    const state = {
      currentPath: "/page1.html",
      currentTitle: "",
      pendingNavType: null as "back" | "forward" | null,
    };
    const result = simulateReset(history, state);

    expect(result.history.canGoBack()).toBe(false);
  });

  test("after reset, canGoForward returns false", () => {
    const history = createNavigationHistory();
    history._setInitialUrl("http://localhost:8080/");
    history._onNewNavigation("http://localhost:8080/page1.html");
    history._index--; // Go back
    expect(history.canGoForward()).toBe(true);

    const state = {
      currentPath: "/",
      currentTitle: "",
      pendingNavType: null as "back" | "forward" | null,
    };
    const result = simulateReset(history, state);

    expect(result.history.canGoForward()).toBe(false);
  });

  test("after reset, first navigation initializes history correctly", () => {
    const history = createNavigationHistory();
    history._setInitialUrl("http://localhost:8080/old-site/");
    history._onNewNavigation("http://localhost:8080/old-site/page.html");

    const state = {
      currentPath: "/old-site/page.html",
      currentTitle: "Old Page",
      pendingNavType: null as "back" | "forward" | null,
    };
    const result = simulateReset(history, state);

    // Simulate first navigation after reset (new folder)
    result.history._setInitialUrl("http://localhost:8080/");

    expect(result.history.length()).toBe(1);
    expect(result.history._initialized).toBe(true);
    expect(result.history.getCurrentUrl()).toBe("http://localhost:8080/");
  });
});

/**
 * Tests for folder switching scenario.
 *
 * This tests the complete flow when user switches from one website folder
 * to another via the context menu.
 */
/**
 * Tests for NavigationManager.reload() — in-place iframe reload via RPC.
 *
 * When a file changes, instead of setting iframe.src (which resets scroll),
 * we call location.reload() inside the iframe via RPC. This preserves
 * scroll position because modern browsers restore scroll on reload.
 */
describe("NavigationManager.reload()", () => {
  let mockRpcCall: ReturnType<typeof vi.fn>;
  let navManager: NavigationManager;

  beforeEach(() => {
    // Setup minimal DOM for NavigationManager constructor
    document.body.innerHTML = `
      <button id="btn-nav-back"></button>
      <button id="btn-nav-forward"></button>
      <span id="moss-url-text">/</span>
      <div class="moss-device-toggle"></div>
      <div class="moss-preview-container"></div>
    `;

    // Create a mock iframe with a contentWindow for RPC
    const iframe = document.createElement("iframe");
    document.body.appendChild(iframe);

    // Create NavigationManager with real iframe
    navManager = new NavigationManager(iframe);

    // Access the internal rpc and spy on its call method
    mockRpcCall = vi.fn();
    // @ts-ignore — accessing private for testing
    navManager.rpc.call = mockRpcCall;
  });

  afterEach(() => {
    document.body.innerHTML = "";
  });

  test("returns true when RPC call succeeds", async () => {
    mockRpcCall.mockResolvedValue(undefined);

    const result = await navManager.reload();

    expect(result).toBe(true);
    expect(mockRpcCall).toHaveBeenCalledWith("location.reload");
  });

  test("returns false when RPC is not available (no iframe)", async () => {
    // Create a NavigationManager with null iframe (no RPC)
    const noIframeManager = new NavigationManager(null);

    const result = await noIframeManager.reload();

    expect(result).toBe(false);
  });

  test("returns false when RPC call fails (e.g., timeout on non-HTML content)", async () => {
    mockRpcCall.mockRejectedValue(new Error("RPC timeout: location.reload"));

    const result = await navManager.reload();

    expect(result).toBe(false);
  });

  test("calls location.reload exactly once", async () => {
    mockRpcCall.mockResolvedValue(undefined);

    await navManager.reload();

    expect(mockRpcCall).toHaveBeenCalledTimes(1);
  });
});

/**
 * Tests for NavigationManager.morph() — the in-place idiomorph path.
 * morph() posts `moss-morph` to the iframe bridge and resolves on the bridge's
 * `moss-morph-applied` ack (true), a `moss-morph-failed` (false), a timeout
 * (false), or immediately false when there is no iframe. The actual DOM
 * reconciliation runs inside the bridge and is covered by the Playwright e2e
 * keystone spec; here we lock the parent-side contract.
 */
describe("NavigationManager.morph()", () => {
  let iframe: HTMLIFrameElement;
  let navManager: NavigationManager;
  let postSpy: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    document.body.innerHTML = `
      <button id="btn-nav-back"></button>
      <button id="btn-nav-forward"></button>
      <span id="moss-url-text">/</span>
      <div class="moss-device-toggle"></div>
      <div class="moss-preview-container"></div>
    `;
    iframe = document.createElement("iframe");
    document.body.appendChild(iframe);
    navManager = new NavigationManager(iframe);
    // Spy on the bridge target so we can assert the request + simulate the ack.
    postSpy = vi.spyOn(iframe.contentWindow as Window, "postMessage").mockImplementation(() => {});
  });

  afterEach(() => {
    vi.useRealTimers();
    document.body.innerHTML = "";
    vi.restoreAllMocks();
  });

  test("returns false immediately when there is no iframe", async () => {
    const noIframe = new NavigationManager(null);
    await expect(noIframe.morph()).resolves.toBe(false);
  });

  test("posts moss-morph with the generation and resolves true on the ack", async () => {
    const p = navManager.morph();
    expect(postSpy).toHaveBeenCalledWith({ type: "moss-morph", gen: 1 }, "*");
    // Bridge acks success.
    window.dispatchEvent(new MessageEvent("message", { source: iframe.contentWindow, data: { gen: (postSpy.mock.calls.at(-1)![0] as { gen: number }).gen, type: "moss-morph-applied" } }));
    await expect(p).resolves.toBe(true);
  });

  test("resolves false on moss-morph-failed", async () => {
    const p = navManager.morph();
    window.dispatchEvent(
      new MessageEvent("message", { source: iframe.contentWindow, data: { gen: (postSpy.mock.calls.at(-1)![0] as { gen: number }).gen, type: "moss-morph-failed", error: "boom" } }),
    );
    await expect(p).resolves.toBe(false);
  });

  test("ignores unrelated messages, then resolves on the real ack", async () => {
    const p = navManager.morph();
    window.dispatchEvent(new MessageEvent("message", { source: iframe.contentWindow, data: { type: "unrelated-navigation", url: "/x" } }));
    window.dispatchEvent(new MessageEvent("message", { source: iframe.contentWindow, data: { type: "something-else" } }));
    window.dispatchEvent(new MessageEvent("message", { source: iframe.contentWindow, data: { gen: (postSpy.mock.calls.at(-1)![0] as { gen: number }).gen, type: "moss-morph-applied" } }));
    await expect(p).resolves.toBe(true);
  });

  test("resolves false after the timeout when no ack arrives", async () => {
    vi.useFakeTimers();
    const p = navManager.morph();
    await vi.advanceTimersByTimeAsync(4000);
    await expect(p).resolves.toBe(false);
  });

  test("syncs the nav-pill title from the morph ack (heals a stuck title)", async () => {
    const pill = document.getElementById("moss-url-text")!;
    // Simulate the pill stuck on the 404 title from a prior real navigation.
    navManager.applyMorphedTitle("Not Found");
    expect(pill.textContent).toBe("Not Found");

    // A refresh morph re-fetches the now-healed page; the ack carries its title.
    const p = navManager.morph();
    window.dispatchEvent(
      new MessageEvent("message", { source: iframe.contentWindow,
        data: { gen: (postSpy.mock.calls.at(-1)![0] as { gen: number }).gen, type: "moss-morph-applied", title: "奥林匹克国家公园" },
      }),
    );
    await expect(p).resolves.toBe(true);
    expect(pill.textContent).toBe("奥林匹克国家公园");
  });

  test("applyMorphedTitle is a no-op for an empty or unchanged title", () => {
    const pill = document.getElementById("moss-url-text")!;
    navManager.applyMorphedTitle("Article");
    expect(pill.textContent).toBe("Article");
    // Empty/undefined must not blank the pill.
    navManager.applyMorphedTitle("");
    navManager.applyMorphedTitle(undefined);
    expect(pill.textContent).toBe("Article");
  });
  test("ignores acknowledgements from another morph on the same page", async () => {
    const first = navManager.morph();
    const firstGen = (postSpy.mock.calls.at(-1)![0] as { gen: number }).gen;
    const second = navManager.morph();
    const secondGen = (postSpy.mock.calls.at(-1)![0] as { gen: number }).gen;
    expect(secondGen).not.toBe(firstGen);
    window.dispatchEvent(new MessageEvent("message", { source: iframe.contentWindow, data: { type: "moss-morph-applied", gen: firstGen, title: "First" } }));
    await expect(first).resolves.toBe(true);
    let secondSettled = false;
    void second.then(() => { secondSettled = true; });
    await Promise.resolve();
    expect(secondSettled).toBe(false);
    window.dispatchEvent(new MessageEvent("message", { source: iframe.contentWindow, data: { type: "moss-morph-applied", gen: secondGen, title: "Second" } }));
    await expect(second).resolves.toBe(true);
  });

  test("a late morph acknowledgement cannot change the title after navigation intent changes", async () => {
    navManager.applyMorphedTitle("Current");
    const morph = navManager.morph();
    const gen = (postSpy.mock.calls.at(-1)![0] as { gen: number }).gen;
    navManager.beginNavigation("http://localhost:8080/posts/");
    window.dispatchEvent(new MessageEvent("message", { source: iframe.contentWindow, data: { type: "moss-morph-applied", gen, title: "Obsolete" } }));
    await expect(morph).resolves.toBe(false);
    expect(document.getElementById("moss-url-text")!.textContent).toBe("Current");
  });

});

/**
 * The navigation-result listener is the parent-side hook that drives the rename
 * 404-retry: it fires after every bridge-echoed navigation with whether the page
 * was the server's synthetic 404. See preview-actions.ts `decideRenameRetry`.
 */
describe("NavigationManager navigation-result listener", () => {
  let navManager: NavigationManager;

  beforeEach(() => {
    document.body.innerHTML = `
      <button id="btn-nav-back"></button>
      <button id="btn-nav-forward"></button>
      <span id="moss-url-text">/</span>
      <div class="moss-device-toggle"></div>
      <div class="moss-preview-container"></div>
    `;
    const iframe = document.createElement("iframe");
    document.body.appendChild(iframe);
    navManager = new NavigationManager(iframe);
  });

  afterEach(() => {
    document.body.innerHTML = "";
  });

  function navMessage(url: string, notFound?: boolean): void {
    window.dispatchEvent(
      new MessageEvent("message", {
        data: { type: "moss-navigation", url, notFound },
      }),
    );
  }

  test("reports notFound=true and the shell-marker-stripped URL", () => {
    const seen: Array<{ url: string; notFound: boolean }> = [];
    navManager.setNavigationResultListener((info) => seen.push(info));
    navMessage("http://localhost:8080/writings/x/index.html?__moss_shell=1", true);
    expect(seen).toEqual([
      { url: "http://localhost:8080/writings/x/index.html", notFound: true },
    ]);
  });

  test("coerces a missing notFound to false (older bridge / non-HTML fallback)", () => {
    const seen: Array<{ url: string; notFound: boolean }> = [];
    navManager.setNavigationResultListener((info) => seen.push(info));
    navMessage("http://localhost:8080/served/index.html"); // no notFound field
    expect(seen).toEqual([
      { url: "http://localhost:8080/served/index.html", notFound: false },
    ]);
  });

  test("passing null clears the listener", () => {
    const seen: unknown[] = [];
    navManager.setNavigationResultListener((info) => seen.push(info));
    navManager.setNavigationResultListener(null);
    navMessage("http://localhost:8080/whatever/index.html", true);
    expect(seen).toHaveLength(0);
  });

  test("does NOT report a result on the synthesized non-HTML fallback", () => {
    // The iframe load → 500ms fallback synthesizes a navigation from iframe.src
    // with no notFound info. It must NOT fire the result listener: a false
    // notFound=false would prematurely cancel an in-flight rename retry (e.g. a
    // slow 404 whose bridge echo lands after the 500ms timeout). Regression
    // guard for the adversarial-review finding.
    vi.useFakeTimers();
    const seen: unknown[] = [];
    navManager.setNavigationResultListener((info) => seen.push(info));
    const iframe = document.querySelector("iframe")!;
    navManager.beginNavigation("http://localhost:8080/feed.xml"); // arms the fallback on the next load
    iframe.src = "http://localhost:8080/feed.xml";
    iframe.dispatchEvent(new Event("load")); // no moss-navigation will arrive
    vi.advanceTimersByTime(500);             // fallback fires (synthesized)
    expect(seen).toHaveLength(0);
    vi.useRealTimers();
  });
});

/**
 * getCurrentUrl() must reflect the page actually displayed (`activePage`), not
 * the back/forward history. An editor-driven follow (PreviewFollower) lands as
 * an ECHO that updates activePage but deliberately does NOT push history. If
 * getCurrentUrl() read history, SmartDiff would compare a rebuilt page against
 * the pre-follow URL and never refresh the preview — the user-visible symptom
 * of "editing a folder index opened from the file tree doesn't refresh".
 */
describe("getCurrentUrl reflects editor-driven follow (activePage, not history)", () => {
  let navManager: NavigationManager;

  beforeEach(() => {
    document.body.innerHTML = `
      <button id="btn-nav-back"></button>
      <button id="btn-nav-forward"></button>
      <span id="moss-url-text">/</span>
      <div class="moss-device-toggle"></div>
      <div class="moss-preview-container"></div>
    `;
    const iframe = document.createElement("iframe");
    document.body.appendChild(iframe);
    navManager = new NavigationManager(iframe);
  });

  afterEach(() => {
    document.body.innerHTML = "";
  });

  function navMessage(url: string, canGoBack = false): void {
    window.dispatchEvent(
      new MessageEvent("message", {
        data: { type: "moss-navigation", url, canGoBack, canGoForward: false },
      }),
    );
  }

  test("getCurrentUrl returns the followed URL after an editor-driven echo", () => {
    const home = "http://localhost:8080/";
    const research = "http://localhost:8080/research/";

    // Preview starts on home (real navigation: history + activePage = home).
    navMessage(home);
    expect(navManager.getCurrentUrl()).toBe(home);

    // Editor opens a self-named folder index from the file tree → PreviewFollower
    // issues an editor-driven nav to /research/ and pre-registers the echo token.
    navManager.markEditorNavigation(research);
    // The iframe bridge bounces back moss-navigation(/research/) → recognized as
    // an echo: activePage becomes /research/, history is intentionally NOT pushed.
    navMessage(research, /* canGoBack */ true);

    // getCurrentUrl() must report the displayed page so SmartDiff refreshes it.
    expect(navManager.getCurrentUrl()).toBe(research);
    // History stays on home (editor follows don't pollute back/forward).
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    expect((navManager as any).history.getCurrentUrl()).toBe(home);
  });

  test("getCurrentUrl falls back to history when no page is active yet", () => {
    // Before any navigation, activePage is null; getCurrentUrl returns null
    // rather than throwing.
    expect(navManager.getCurrentUrl()).toBeNull();
  });
});

/**
 * Tests for post-rebuild scroll restoration (sync healing).
 *
 * After a file-change-triggered rebuild, the preview iframe reloads.
 * The bridge re-initializes and reports moss-navigation with the same URL.
 * NavigationManager detects this same-URL event and pushes the cached
 * editor position back via scrollToSourceLine.
 */
describe("post-rebuild scroll restoration", () => {
  test("setLastEditorPosition caches the position", () => {
    document.body.innerHTML = `
      <button id="btn-nav-back"></button>
      <button id="btn-nav-forward"></button>
      <span id="moss-url-text">/</span>
      <div class="moss-device-toggle"></div>
      <div class="moss-preview-container"></div>
    `;
    const iframe = document.createElement("iframe");
    document.body.appendChild(iframe);
    const navManager = new NavigationManager(iframe);

    navManager.setLastEditorPosition(42, "section-3");

    // @ts-ignore — accessing private for testing
    expect(navManager.lastEditorPosition).toEqual({ line: 42, anchorId: "section-3", atTop: false, atBottom: false });
    document.body.innerHTML = "";
  });

  test("setLastEditorPosition with null anchorId works", () => {
    document.body.innerHTML = `
      <button id="btn-nav-back"></button>
      <button id="btn-nav-forward"></button>
      <span id="moss-url-text">/</span>
      <div class="moss-device-toggle"></div>
      <div class="moss-preview-container"></div>
    `;
    const iframe = document.createElement("iframe");
    document.body.appendChild(iframe);
    const navManager = new NavigationManager(iframe);

    navManager.setLastEditorPosition(100, null);

    // @ts-ignore — accessing private for testing
    expect(navManager.lastEditorPosition).toEqual({ line: 100, anchorId: null, atTop: false, atBottom: false });
    document.body.innerHTML = "";
  });

  test("reset() clears cached editor position", () => {
    document.body.innerHTML = `
      <button id="btn-nav-back"></button>
      <button id="btn-nav-forward"></button>
      <span id="moss-url-text">/</span>
      <div class="moss-device-toggle"></div>
      <div class="moss-preview-container"></div>
    `;
    const iframe = document.createElement("iframe");
    document.body.appendChild(iframe);
    const navManager = new NavigationManager(iframe);

    navManager.setLastEditorPosition(42, "section-3");
    navManager.reset();

    // @ts-ignore — accessing private for testing
    expect(navManager.lastEditorPosition).toBeNull();
    document.body.innerHTML = "";
  });

  test("same-URL handleNavigationUpdate triggers scroll restoration", () => {
    document.body.innerHTML = `
      <button id="btn-nav-back"></button>
      <button id="btn-nav-forward"></button>
      <span id="moss-url-text">/</span>
      <div class="moss-device-toggle"></div>
      <div class="moss-preview-container"></div>
    `;
    const iframe = document.createElement("iframe");
    document.body.appendChild(iframe);
    const navManager = new NavigationManager(iframe);

    const mockRpcCall = vi.fn().mockResolvedValue(undefined);
    // @ts-ignore — accessing private for testing
    navManager.rpc.call = mockRpcCall;

    // Simulate initial navigation
    // @ts-ignore — calling private for testing
    navManager.handleNavigationUpdate({ url: "http://localhost:8080/article.html", title: "Article" });

    // Cache editor position
    navManager.setLastEditorPosition(42, "section-3");

    // Simulate same-URL (rebuild reload)
    // @ts-ignore — calling private for testing
    navManager.handleNavigationUpdate({ url: "http://localhost:8080/article.html", title: "Article" });

    // Should call scrollToSourceLine via RPC with atTop forwarded
    expect(mockRpcCall).toHaveBeenCalledWith("scrollToSourceLine", 42, false, false, false);
    document.body.innerHTML = "";
  });

  test("different-URL navigation does not trigger scroll restoration", () => {
    document.body.innerHTML = `
      <button id="btn-nav-back"></button>
      <button id="btn-nav-forward"></button>
      <span id="moss-url-text">/</span>
      <div class="moss-device-toggle"></div>
      <div class="moss-preview-container"></div>
    `;
    const iframe = document.createElement("iframe");
    document.body.appendChild(iframe);
    const navManager = new NavigationManager(iframe);

    const mockRpcCall = vi.fn().mockResolvedValue(undefined);
    // @ts-ignore — accessing private for testing
    navManager.rpc.call = mockRpcCall;

    // Simulate initial navigation
    // @ts-ignore — calling private for testing
    navManager.handleNavigationUpdate({ url: "http://localhost:8080/", title: "" });

    navManager.setLastEditorPosition(42, null);

    // Navigate to a DIFFERENT page (not a reload)
    // @ts-ignore — calling private for testing
    navManager.handleNavigationUpdate({ url: "http://localhost:8080/other.html", title: "Other" });

    // scrollToSourceLine should NOT be called for new navigation
    expect(mockRpcCall).not.toHaveBeenCalled();
    document.body.innerHTML = "";
  });

  test("navigating to a new page clears the stale cached position, so a later same-URL reload does not resurrect it", () => {
    document.body.innerHTML = `
      <button id="btn-nav-back"></button>
      <button id="btn-nav-forward"></button>
      <span id="moss-url-text">/</span>
      <div class="moss-device-toggle"></div>
      <div class="moss-preview-container"></div>
    `;
    const iframe = document.createElement("iframe");
    document.body.appendChild(iframe);
    const navManager = new NavigationManager(iframe);

    const mockRpcCall = vi.fn().mockResolvedValue(undefined);
    // @ts-ignore — accessing private for testing
    navManager.rpc.call = mockRpcCall;

    // On page A, the editor was scrolled to the bottom.
    // @ts-ignore — calling private for testing
    navManager.handleNavigationUpdate({ url: "http://localhost:8080/page-a.html", title: "A" });
    navManager.setLastEditorPosition(99, null, false, true);

    // User clicks a link to page B (a real navigation, not an echo).
    // @ts-ignore — calling private for testing
    navManager.handleNavigationUpdate({ url: "http://localhost:8080/page-b.html", title: "B" });

    // @ts-ignore — accessing private for testing
    expect(navManager.lastEditorPosition).toBeNull();

    // A rebuild reloads page B (same URL) before the editor ever reports a
    // position on it — must NOT scroll to page A's stale bottom-of-page.
    // @ts-ignore — calling private for testing
    navManager.handleNavigationUpdate({ url: "http://localhost:8080/page-b.html", title: "B" });

    // scrollToSourceLine's RPC signature is (name, line, atTop, atBottom,
    // onlyIfOffscreen) — no
    // anchorId (see scrollToSourceLine() below). Without the fix, the stale
    // cache would have driven exactly this call.
    expect(mockRpcCall).not.toHaveBeenCalledWith("scrollToSourceLine", 99, false, true, false);
    document.body.innerHTML = "";
  });
});

/**
 * Tests for the `isReload` flag on the emitted PREVIEW_URL_CHANGED event.
 *
 * A same-URL bridge re-report (a rebuild RELOAD) must be distinguishable from
 * a genuine navigation. Consumers (app.ts) use this to suppress the
 * editor:load-source forward on a reload — otherwise a rebuild of the home
 * page after a file-create stomps the editor off the brand-new file it just
 * opened (BUG 4: "new file flashes in the breadcrumb then vanishes").
 *
 * Semantics:
 *   - first load (previousUrl null / history uninitialized) → isReload=false
 *   - genuine navigation to a NEW url                       → isReload=false
 *   - same-URL re-report AFTER an initial load (rebuild)    → isReload=true
 */
describe("PREVIEW_URL_CHANGED isReload flag", () => {
  let navManager: NavigationManager;

  beforeEach(() => {
    document.body.innerHTML = `
      <button id="btn-nav-back"></button>
      <button id="btn-nav-forward"></button>
      <span id="moss-url-text">/</span>
      <div class="moss-device-toggle"></div>
      <div class="moss-preview-container"></div>
    `;
    const iframe = document.createElement("iframe");
    document.body.appendChild(iframe);
    navManager = new NavigationManager(iframe);
    vi.mocked(eventBus.emit).mockClear();
  });

  afterEach(() => {
    document.body.innerHTML = "";
  });

  // folderPath is intentionally unset, so emitUrlChangedEvent runs
  // synchronously (no resolvePageSource await) and emits the default event.
  function lastUrlChangedEvent(): { isReload?: boolean } | undefined {
    const calls = vi
      .mocked(eventBus.emit)
      .mock.calls.filter((c) => c[0] === Events.PREVIEW_URL_CHANGED);
    return calls.length
      ? (calls[calls.length - 1]![1] as { isReload?: boolean })
      : undefined;
  }

  test("first load emits isReload=false (previousUrl is null)", () => {
    // @ts-ignore — calling private for testing
    navManager.handleNavigationUpdate({ url: "http://localhost:8080/", title: "Home" });
    expect(lastUrlChangedEvent()?.isReload).toBe(false);
  });

  test("genuine navigation to a new URL emits isReload=false", () => {
    // @ts-ignore — calling private for testing
    navManager.handleNavigationUpdate({ url: "http://localhost:8080/", title: "Home" });
    // @ts-ignore — calling private for testing
    navManager.handleNavigationUpdate({ url: "http://localhost:8080/posts/a/", title: "A" });
    expect(lastUrlChangedEvent()?.isReload).toBe(false);
  });

  test("same-URL rebuild reload emits isReload=true (distinct from a genuine nav)", () => {
    // @ts-ignore — calling private for testing
    navManager.handleNavigationUpdate({ url: "http://localhost:8080/", title: "Home" });
    // Rebuild: the bridge re-reports the SAME url after the site rebuilds.
    // @ts-ignore — calling private for testing
    navManager.handleNavigationUpdate({ url: "http://localhost:8080/", title: "Home" });
    expect(lastUrlChangedEvent()?.isReload).toBe(true);
  });
});

/**
 * Tests for the canonical `activePage` state on NavigationManager.
 *
 * `activePage` tracks what page the preview iframe is currently showing,
 * as a { url, sourceFile } pair. Later tasks use this for identity
 * validation of cross-pane messages (e.g., stale scroll-sync events).
 */
describe("activePage state", () => {
  beforeEach(() => {
    document.body.innerHTML = `
      <button id="btn-nav-back"></button>
      <button id="btn-nav-forward"></button>
      <span id="moss-url-text">/</span>
      <div class="moss-device-toggle"></div>
      <div class="moss-preview-container"></div>
    `;
  });

  afterEach(() => {
    document.body.innerHTML = "";
  });

  test("activePage is null before any navigation", () => {
    const iframe = document.createElement("iframe");
    document.body.appendChild(iframe);
    const manager = new NavigationManager(iframe);

    expect(manager.getActivePage()).toBeNull();
  });

  test("activePage is set after moss-navigation message", () => {
    const iframe = document.createElement("iframe");
    document.body.appendChild(iframe);
    const manager = new NavigationManager(iframe);

    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/posts/foo/", title: "Foo" });

    expect(manager.getActivePage()).toEqual({
      url: "http://localhost:8080/posts/foo/",
      sourceFile: null, // sourceFile resolved asynchronously; null at sync time
    });
  });

  test("sourceFile populated on successful resolvePageSource", async () => {
    const iframe = document.createElement("iframe");
    document.body.appendChild(iframe);
    const manager = new NavigationManager(iframe);
    manager.setFolderPath("/tmp/some-folder");

    const resolveMock = vi.mocked(engine.resolvePageSource);
    resolveMock.mockResolvedValueOnce({
      status: "ok",
      data: {
        source_path: "posts/foo.md",
        is_dir: false,
        is_article: true,
        syndicated: [],
      },
    } as any);

    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/posts/foo/", title: "Foo" });

    // Flush pending microtasks so the resolvePageSource promise settles.
    await Promise.resolve();
    await Promise.resolve();

    expect(manager.getActivePage()).toEqual({
      url: "http://localhost:8080/posts/foo/",
      sourceFile: "posts/foo.md",
    });
  });

  test("a fresh navigation reports its URL at beginNavigation and at the echo, not again after the source resolves", async () => {
    const iframe = document.createElement("iframe");
    document.body.appendChild(iframe);
    const manager = new NavigationManager(iframe);
    manager.setFolderPath("/tmp/some-folder");
    const focus = vi.fn();
    manager.setPreviewRequirementListener(focus);
    vi.mocked(engine.resolvePageSource).mockResolvedValueOnce({
      status: "ok",
      data: { source_path: null, source_pending: false, is_dir: true, is_article: false },
    } as any);

    manager.beginNavigation("http://localhost:8080/places/");
    expect(focus).toHaveBeenLastCalledWith("/places/");
    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/places/", title: "Places" });
    expect(focus).toHaveBeenCalledTimes(2);
    await Promise.resolve();
    await Promise.resolve();
    expect(focus).toHaveBeenCalledTimes(2);
  });

  test("a same-page reload does not report again when its source is unchanged", async () => {
    const iframe = document.createElement("iframe");
    document.body.appendChild(iframe);
    const manager = new NavigationManager(iframe);
    manager.setFolderPath("/tmp/some-folder");
    const focus = vi.fn();
    manager.setPreviewRequirementListener(focus);
    vi.mocked(engine.resolvePageSource).mockResolvedValue({
      status: "ok",
      data: { source_path: "/tmp/some-folder/one.md", source_pending: false, is_dir: false, is_article: true },
    } as any);

    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/one/", title: "One" });
    await Promise.resolve();
    await Promise.resolve();
    // the echo, then the lookup learning a source the page did not have
    expect(focus).toHaveBeenCalledTimes(2);

    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/one/", title: "One" });
    await Promise.resolve();
    await Promise.resolve();
    expect(focus).toHaveBeenCalledTimes(2);
  });

  test("a same-page reload whose resolved source changed reports the URL once more", async () => {
    const iframe = document.createElement("iframe");
    document.body.appendChild(iframe);
    const manager = new NavigationManager(iframe);
    manager.setFolderPath("/tmp/some-folder");
    const focus = vi.fn();
    manager.setPreviewRequirementListener(focus);
    vi.mocked(engine.resolvePageSource)
      .mockResolvedValueOnce({ status: "ok", data: { source_path: "/tmp/some-folder/one.md", source_pending: false, is_dir: false, is_article: true } } as any)
      .mockResolvedValueOnce({ status: "ok", data: { source_path: "/tmp/some-folder/two.md", source_pending: false, is_dir: false, is_article: true } } as any);

    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/one/", title: "One" });
    await Promise.resolve();
    await Promise.resolve();
    expect(focus).toHaveBeenCalledTimes(2);

    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/one/", title: "One" });
    await Promise.resolve();
    await Promise.resolve();
    expect(focus).toHaveBeenCalledTimes(3);
    expect(focus).toHaveBeenLastCalledWith("/one/");
  });

  test("stale resolvePageSource changes neither activePage nor emitted source", async () => {
    vi.mocked(eventBus.emit).mockClear();
    const iframe = document.createElement("iframe");
    document.body.appendChild(iframe);
    const manager = new NavigationManager(iframe);
    manager.setFolderPath("/tmp/some-folder");
    const focus = vi.fn();
    manager.setPreviewRequirementListener(focus);

    const resolveMock = vi.mocked(engine.resolvePageSource);

    // First navigation's resolvePageSource will settle LATER (we control it).
    let resolveFirst: (value: any) => void = () => {};
    const firstPending = new Promise((res) => {
      resolveFirst = res;
    });
    resolveMock.mockImplementationOnce(() => firstPending as any);

    // Second navigation resolves immediately with its own source_path.
    resolveMock.mockResolvedValueOnce({
      status: "ok",
      data: {
        source_path: "posts/bar.md",
        is_dir: false,
        is_article: true,
        syndicated: [],
      },
    } as any);

    // Navigate to /posts/foo/ — first resolvePageSource starts but does not settle.
    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/posts/foo/", title: "Foo" });

    // Navigate to /posts/bar/ before the first resolve settles.
    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/posts/bar/", title: "Bar" });

    // Let the second resolve settle.
    await Promise.resolve();
    await Promise.resolve();

    // activePage should reflect the SECOND navigation.
    expect(manager.getActivePage()).toEqual({
      url: "http://localhost:8080/posts/bar/",
      sourceFile: "posts/bar.md",
    });
    expect(focus).toHaveBeenLastCalledWith("/posts/bar/");

    // Now let the stale first resolve finish.
    resolveFirst({
      status: "ok",
      data: {
        source_path: "posts/foo.md",
        is_dir: false,
        is_article: true,
        syndicated: [],
      },
    });
    await Promise.resolve();
    await Promise.resolve();

    // activePage must NOT have been overwritten by the stale first resolution.
    expect(manager.getActivePage()).toEqual({
      url: "http://localhost:8080/posts/bar/",
      sourceFile: "posts/bar.md",
    });
    expect(focus).toHaveBeenLastCalledWith("/posts/bar/");
    expect(focus).toHaveBeenCalledTimes(3); // A echo, B echo, B's lookup learning its source; the stale A lookup reports nothing
    expect(vi.mocked(eventBus.emit).mock.calls.map(([, e]) => (e as any).sourcePath)).toEqual(['posts/bar.md']);
  });

  test("an old A lookup cannot replace a newer A after visiting B", async () => {
    const iframe = document.createElement("iframe");
    document.body.appendChild(iframe);
    const manager = new NavigationManager(iframe);
    manager.setFolderPath("/tmp/some-folder");
    const focus = vi.fn();
    manager.setPreviewRequirementListener(focus);
    let resolveOldA: (value: any) => void = () => {};
    vi.mocked(engine.resolvePageSource)
      .mockImplementationOnce(() => new Promise((resolve) => { resolveOldA = resolve; }) as any)
      .mockResolvedValueOnce({ status: "ok", data: { source_path: "b.md", source_pending: false, is_dir: false, is_article: true } } as any)
      .mockResolvedValueOnce({ status: "ok", data: { source_path: "new-a.md", source_pending: false, is_dir: false, is_article: true } } as any);

    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/a/" });
    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/b/" });
    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/a/" });
    await Promise.resolve();
    await Promise.resolve();
    expect(manager.getActivePage()?.sourceFile).toBe("new-a.md");
    const calls = focus.mock.calls.length;

    resolveOldA({ status: "ok", data: { source_path: "old-a.md", source_pending: false, is_dir: false, is_article: true } });
    await Promise.resolve();
    await Promise.resolve();
    expect(manager.getActivePage()?.sourceFile).toBe("new-a.md");
    expect(focus).toHaveBeenCalledTimes(calls);
  });

  test.each(['return-to-same-url', 'folder-change', 'destroy'])('pending resolution expires on %s', async (replacement) => {
    const manager = new NavigationManager(document.createElement('iframe'));
    manager.setFolderPath('/first');
    const requirement = vi.fn();
    manager.setPreviewRequirementListener(requirement);
    vi.mocked(eventBus.emit).mockClear();
    let finish!: (v: any) => void;
    vi.mocked(engine.resolvePageSource).mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
    // @ts-ignore private transition
    manager.handleNavigationUpdate({ url: 'http://localhost/a/', title: 'Old A' });
    if (replacement === 'return-to-same-url') {
      vi.mocked(engine.resolvePageSource).mockResolvedValue({ status: 'ok', data: { source_path: 'new-a.md', is_dir: false, is_article: true } } as any);
      // @ts-ignore private transition
      manager.handleNavigationUpdate({ url: 'http://localhost/b/' });
      // @ts-ignore private transition
      manager.handleNavigationUpdate({ url: 'http://localhost/a/' });
      await Promise.resolve();
    } else if (replacement === 'folder-change') {
      manager.setFolderPath('/second');
    } else {
      manager.destroy();
    }
    const before = vi.mocked(eventBus.emit).mock.calls.length;
    const requirementCalls = requirement.mock.calls.length;
    finish({ status: 'ok', data: { source_path: 'old-a.md', is_dir: false, is_article: true } });
    await Promise.resolve();
    expect(vi.mocked(eventBus.emit).mock.calls).toHaveLength(before);
    expect(manager.getActivePage()?.sourceFile).not.toBe('old-a.md');
    expect(requirement).toHaveBeenCalledTimes(requirementCalls);
  });

  test("reset() clears activePage", () => {
    const iframe = document.createElement("iframe");
    document.body.appendChild(iframe);
    const manager = new NavigationManager(iframe);

    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/", title: "Home" });
    expect(manager.getActivePage()).not.toBeNull();

    manager.reset();
    expect(manager.getActivePage()).toBeNull();
  });
});

describe("folder switching flow", () => {
  /**
   * Simulates the complete flow when switching folders:
   * 1. PreviewManager.startBuild() is called
   * 2. state.reset() is called (PreviewState)
   * 3. navigationManager.reset() should be called (NEW)
   * 4. loadPreview(url) loads the new folder's content
   * 5. First navigation event initializes history
   */
  function simulateFolderSwitch(
    oldHistory: ReturnType<typeof createNavigationHistory>,
    newFolderUrl: string
  ): ReturnType<typeof createNavigationHistory> {
    // Reset navigation state
    const newHistory = createNavigationHistory();

    // First navigation from new folder
    newHistory._setInitialUrl(newFolderUrl);

    return newHistory;
  }

  test("switching folders clears old history and initializes with new URL", () => {
    // Old folder: guo-website
    const oldHistory = createNavigationHistory();
    oldHistory._setInitialUrl("http://localhost:8080/");
    oldHistory._onNewNavigation("http://localhost:8080/posts/guo-post.html");
    expect(oldHistory.length()).toBe(2);
    expect(oldHistory.getCurrentUrl()).toBe("http://localhost:8080/posts/guo-post.html");

    // Switch to: yi-website
    const newHistory = simulateFolderSwitch(oldHistory, "http://localhost:8080/?_t=12345");

    // New folder should have fresh history
    expect(newHistory.length()).toBe(1);
    expect(newHistory.getCurrentUrl()).toBe("http://localhost:8080/?_t=12345");
    expect(newHistory.canGoBack()).toBe(false);
    expect(newHistory.canGoForward()).toBe(false);
  });

  test("navigation in new folder works correctly after switch", () => {
    // Old folder state
    const oldHistory = createNavigationHistory();
    oldHistory._setInitialUrl("http://localhost:8080/");
    oldHistory._onNewNavigation("http://localhost:8080/page.html");

    // Switch folders
    const newHistory = simulateFolderSwitch(oldHistory, "http://localhost:8080/");

    // Navigate in new folder
    newHistory._onNewNavigation("http://localhost:8080/yi-post.html");

    expect(newHistory.length()).toBe(2);
    expect(newHistory.canGoBack()).toBe(true);
    expect(newHistory.getCurrentUrl()).toBe("http://localhost:8080/yi-post.html");
  });
});

/**
 * Tests for pendingEcho suppression token.
 *
 * When the editor (parent) initiates a navigation by setting iframe.src, the
 * resulting moss-navigation message from the bridge is an acknowledgment, not
 * a new user-driven navigation. The pendingEcho token lets the parent mark
 * its own issuance and recognize the echo when it comes back.
 */
describe("pendingEcho suppression", () => {
  beforeEach(() => {
    document.body.innerHTML = `
      <button id="btn-nav-back"></button>
      <button id="btn-nav-forward"></button>
      <span id="moss-url-text">/</span>
      <div class="moss-device-toggle"></div>
      <div class="moss-preview-container"></div>
    `;
  });

  afterEach(() => {
    document.body.innerHTML = "";
  });

  test("markEditorNavigation records a pending echo for the URL", () => {
    const iframe = document.createElement("iframe");
    document.body.appendChild(iframe);
    const manager = new NavigationManager(iframe);

    manager.markEditorNavigation("http://localhost:8080/posts/bar/");

    expect(manager.hasPendingEchoFor("http://localhost:8080/posts/bar/")).toBe(true);
  });

  test("matching moss-navigation clears pendingEcho (acknowledgment)", () => {
    const iframe = document.createElement("iframe");
    document.body.appendChild(iframe);
    const manager = new NavigationManager(iframe);

    const url = "http://localhost:8080/posts/bar/";
    manager.markEditorNavigation(url);
    expect(manager.hasPendingEchoFor(url)).toBe(true);

    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url, title: "Bar" });

    expect(manager.hasPendingEchoFor(url)).toBe(false);
  });

  test("non-matching moss-navigation does NOT clear pendingEcho", () => {
    const iframe = document.createElement("iframe");
    document.body.appendChild(iframe);
    const manager = new NavigationManager(iframe);

    const urlA = "http://localhost:8080/posts/a/";
    const urlB = "http://localhost:8080/posts/b/";
    manager.markEditorNavigation(urlA);

    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: urlB, title: "B" });

    expect(manager.hasPendingEchoFor(urlA)).toBe(true);
  });

  test("after editor-driven navigation settles, re-assert scroll position once", () => {
    const iframe = document.createElement("iframe");
    document.body.appendChild(iframe);
    const manager = new NavigationManager(iframe);

    const mockRpcCall = vi.fn().mockResolvedValue(undefined);
    // @ts-ignore — accessing private for testing
    manager.rpc.call = mockRpcCall;

    // Spy on the scrollToSourceLine method directly so we can assert both
    // args (line + anchorId) — the RPC layer only receives the line.
    const scrollSpy = vi.spyOn(manager, "scrollToSourceLine");

    // Establish initial page (different from target so target is a "new nav").
    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/old/", title: "Old" });

    // Cache editor position and mark an editor-driven navigation.
    manager.setLastEditorPosition(42, "intro");
    manager.markEditorNavigation("http://localhost:8080/new/");

    // Bridge reports the new URL — this is the echo, and it's a DIFFERENT URL
    // from the previous one (takes the "new navigation" branch).
    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/new/", title: "New" });

    // The post-nav re-assert should have invoked scrollToSourceLine exactly
    // once with line 42, anchorId "intro", and atTop=false (default).
    expect(scrollSpy).toHaveBeenCalledTimes(1);
    expect(scrollSpy).toHaveBeenCalledWith(42, "intro", false, false);
  });

  test("editor-driven same-URL navigation fires scrollToSourceLine exactly once (no double-fire)", () => {
    const iframe = document.createElement("iframe");
    document.body.appendChild(iframe);
    const manager = new NavigationManager(iframe);

    const mockRpcCall = vi.fn().mockResolvedValue(undefined);
    // @ts-ignore — accessing private for testing
    manager.rpc.call = mockRpcCall;

    // Establish initial page at /foo/.
    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/foo/", title: "Foo" });

    // Cache editor position.
    manager.setLastEditorPosition(42, null);

    const baseline = mockRpcCall.mock.calls.filter((c) => c[0] === "scrollToSourceLine").length;

    // Editor navigates to the SAME URL (e.g., user clicked a file already displayed).
    manager.markEditorNavigation("http://localhost:8080/foo/");
    // Bridge echoes the same URL. isEcho=true, but currentUrl === previousUrl
    // so the existing same-URL branch should fire exactly ONE scrollToSourceLine;
    // the new trailing re-assert block must be guarded by a URL-changed check.
    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/foo/", title: "Foo" });

    const after = mockRpcCall.mock.calls.filter((c) => c[0] === "scrollToSourceLine").length;
    expect(after - baseline).toBe(1);
  });

  test("same-URL reload still triggers a single re-assert (regression guard)", () => {
    const iframe = document.createElement("iframe");
    document.body.appendChild(iframe);
    const manager = new NavigationManager(iframe);

    const mockRpcCall = vi.fn().mockResolvedValue(undefined);
    // @ts-ignore — accessing private for testing
    manager.rpc.call = mockRpcCall;

    // Initial navigation to /foo/.
    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/foo/", title: "Foo" });

    manager.setLastEditorPosition(42, "intro");

    const before = mockRpcCall.mock.calls.filter((c) => c[0] === "scrollToSourceLine").length;

    // Same-URL reload (no markEditorNavigation → isEcho is false → existing
    // same-URL branch fires; new re-assert block is skipped).
    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/foo/", title: "Foo" });

    const after = mockRpcCall.mock.calls.filter((c) => c[0] === "scrollToSourceLine").length;
    expect(after - before).toBe(1);
  });

  describe("sameUrlPathname helper", () => {
    test("matches identical pathnames", () => {
      expect(sameUrlPathname(
        "http://localhost:8080/foo/",
        "http://localhost:8080/foo/"
      )).toBe(true);
    });

    test("ignores hash fragment", () => {
      expect(sameUrlPathname(
        "http://localhost:8080/foo/",
        "http://localhost:8080/foo/#section-3"
      )).toBe(true);
    });

    test("ignores cache-bust query string", () => {
      expect(sameUrlPathname(
        "http://localhost:8080/foo/?_t=999",
        "http://localhost:8080/foo/"
      )).toBe(true);
    });

    test("differs on different pathnames", () => {
      expect(sameUrlPathname(
        "http://localhost:8080/foo/",
        "http://localhost:8080/bar/"
      )).toBe(false);
    });

    test("returns false on malformed URL", () => {
      expect(sameUrlPathname("not a url", "http://localhost:8080/foo/")).toBe(false);
      expect(sameUrlPathname("http://localhost:8080/foo/", "")).toBe(false);
    });
  });

  test("navigateToPage with same iframe.src clears pendingEcho (no-op)", () => {
    const iframe = document.createElement("iframe");
    // jsdom resolves relative src against the document base; set an absolute URL.
    // In production, every iframe.src assignment goes through `withShellMarker`
    // which appends `__moss_shell=1` (the chrome-clearance pre-paint marker;
    // see `packages/moss-ui/src/ui/preview/shell-marker.ts`). The no-op comparison in
    // navigateToPage compares marked-vs-marked, so we must seed the iframe
    // with the marked URL here to match production semantics.
    iframe.src = "http://localhost:8080/posts/foo/?__moss_shell=1";
    document.body.appendChild(iframe);
    const manager = new NavigationManager(iframe);

    // markEditorNavigation is called with the unmarked (logical) URL — the
    // marker is stripped from `data.url` on entry to handleNavigationUpdate,
    // so the echo-matching cache stores unmarked URLs.
    const sameUrl = "http://localhost:8080/posts/foo/";
    manager.markEditorNavigation(sameUrl);
    expect(manager.hasPendingEchoFor(sameUrl)).toBe(true);

    // iframe.src already equals the target (in marked form) — navigateToPage
    // should be a no-op AND clear the pendingEcho, so an unrelated in-frame
    // click to the same URL later is not misclassified as an echo.
    manager.navigateToPage("/posts/foo/");

    expect(manager.hasPendingEchoFor(sameUrl)).toBe(false);
    // iframe.src stays at the marked URL since the no-op path didn't reassign.
    expect(iframe.src).toBe("http://localhost:8080/posts/foo/?__moss_shell=1");
  });

  test("navigateToPage to the page a link led to does not reload it", () => {
    const iframe = document.createElement("iframe");
    iframe.src = "http://localhost:8080/?__moss_shell=1";
    document.body.appendChild(iframe);
    const manager = new NavigationManager(iframe);
    const report = vi.fn();
    manager.setPreviewRequirementListener(report);
    // A link click inside the frame: the page changes, iframe.src does not.
    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/posts/foo/", title: "Foo" });
    report.mockClear();

    manager.navigateToPage("/posts/foo/");

    expect(iframe.src).toBe("http://localhost:8080/?__moss_shell=1");
    expect(manager.isNavigationPending()).toBe(false);
    expect(report).not.toHaveBeenCalled();
  });

  test("navigateToPage back to the page iframe.src names, after a link led away, navigates", () => {
    const iframe = document.createElement("iframe");
    iframe.src = "http://localhost:8080/posts/foo/?__moss_shell=1";
    document.body.appendChild(iframe);
    const manager = new NavigationManager(iframe);
    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/posts/bar/", title: "Bar" });

    manager.navigateToPage("/posts/foo/");

    expect(manager.isNavigationPending()).toBe(true);
  });

  test("pendingEcho expires after 1s", () => {
    vi.useFakeTimers();
    try {
      const iframe = document.createElement("iframe");
      document.body.appendChild(iframe);
      const manager = new NavigationManager(iframe);

      const url = "http://localhost:8080/posts/bar/";
      manager.markEditorNavigation(url);
      expect(manager.hasPendingEchoFor(url)).toBe(true);

      vi.advanceTimersByTime(1001);

      expect(manager.hasPendingEchoFor(url)).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });
});

// ── scrollToSourceLine: anchor-fallback guard keyed on atTop ─────────────
//
// The bridge's `scrollToSourceLine(*, atTop=true)` short-circuits to
// scrollTop=0 and returns 0. The parent MUST NOT delegate that to the
// anchor fallback (`element.scrollIntoView`), which would scroll back to
// the heading and re-introduce the page-chrome-hidden bug.

describe("NavigationManager.scrollToSourceLine — anchor-fallback guard (atTop)", () => {
  type RpcCall = { method: string; args: unknown[] };
  function makeStubRpc(returnFor: Record<string, unknown>) {
    const calls: RpcCall[] = [];
    return {
      calls,
      rpc: {
        call: vi.fn(async (method: string, ...args: unknown[]) => {
          calls.push({ method, args });
          return returnFor[method];
        }),
      },
    };
  }

  test("atTop=true does NOT trigger anchor fallback even when rpc returns 0", async () => {
    const { calls, rpc } = makeStubRpc({ scrollToSourceLine: 0 });
    const nm = Object.create(NavigationManager.prototype) as InstanceType<typeof NavigationManager>;
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    (nm as any).rpc = rpc;

    await nm.scrollToSourceLine(5, "some-heading-anchor", true);

    expect(calls.some(c => c.method === "scrollToSourceLine")).toBe(true);
    expect(calls.some(c => c.method === "element.scrollIntoView")).toBe(false);
  });

  test("atTop=true is forwarded to the bridge as args[1]", async () => {
    const { calls, rpc } = makeStubRpc({ scrollToSourceLine: 0 });
    const nm = Object.create(NavigationManager.prototype) as InstanceType<typeof NavigationManager>;
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    (nm as any).rpc = rpc;

    await nm.scrollToSourceLine(5, "anchor", true);

    const rpcCall = calls.find(c => c.method === "scrollToSourceLine");
    expect(rpcCall).toBeDefined();
    // args[3] is onlyIfOffscreen — false here, because this is a SCROLL-driven
    // sync. See NavigationManager.scrollToSourceLine.
    expect(rpcCall!.args).toEqual([5, true, false, false]);
  });

  test("atTop omitted defaults to false; line<=1 alone NO LONGER skips anchor fallback (CRITICAL CHANGE)", async () => {
    // The previous PR's `line > 1` guard is gone. With atTop=false, the
    // fallback fires regardless of line — the protocol is now explicit
    // about "at top" via the named field, not via line-number overload.
    const { calls, rpc } = makeStubRpc({ scrollToSourceLine: 0 });
    const nm = Object.create(NavigationManager.prototype) as InstanceType<typeof NavigationManager>;
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    (nm as any).rpc = rpc;

    await nm.scrollToSourceLine(1, "some-heading-anchor");

    expect(calls.some(c => c.method === "element.scrollIntoView" && c.args[0] === "#some-heading-anchor")).toBe(true);
  });

  test("atTop=false with no match (line >= 2) triggers anchor fallback (regression guard)", async () => {
    const { calls, rpc } = makeStubRpc({ scrollToSourceLine: 0 });
    const nm = Object.create(NavigationManager.prototype) as InstanceType<typeof NavigationManager>;
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    (nm as any).rpc = rpc;

    await nm.scrollToSourceLine(7, "getting-started");

    expect(calls.some(c => c.method === "element.scrollIntoView" && c.args[0] === "#getting-started")).toBe(true);
  });

  test("atTop=false with a match does not trigger anchor fallback (regression guard)", async () => {
    const { calls, rpc } = makeStubRpc({ scrollToSourceLine: 5 });
    const nm = Object.create(NavigationManager.prototype) as InstanceType<typeof NavigationManager>;
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    (nm as any).rpc = rpc;

    await nm.scrollToSourceLine(7, "getting-started");

    expect(calls.some(c => c.method === "element.scrollIntoView")).toBe(false);
  });

  test("setLastEditorPosition caches atTop alongside line and anchorId", () => {
    const nm = Object.create(NavigationManager.prototype) as InstanceType<typeof NavigationManager>;
    nm.setLastEditorPosition(5, "a", true);
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    expect((nm as any).lastEditorPosition).toEqual({ line: 5, anchorId: "a", atTop: true, atBottom: false });
  });

  test("setLastEditorPosition defaults atTop to false when omitted", () => {
    const nm = Object.create(NavigationManager.prototype) as InstanceType<typeof NavigationManager>;
    nm.setLastEditorPosition(5, "a");
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    expect((nm as any).lastEditorPosition).toEqual({ line: 5, anchorId: "a", atTop: false, atBottom: false });
  });
});

/**
 * Tests for scrollIntent — "latest user action wins" over editor scroll sync.
 *
 * A hash-fragment navigation on the same pathname sets scrollIntent = 'user',
 * which suppresses the same-URL-reload scroll re-assertion. The editor
 * reclaims ownership via claimScrollForEditor().
 */
describe("scrollIntent — hash-nav wins", () => {
  function makeManager() {
    document.body.innerHTML = `
      <button id="btn-nav-back"></button>
      <button id="btn-nav-forward"></button>
      <span id="moss-url-text">/</span>
      <div class="moss-device-toggle"></div>
      <div class="moss-preview-container"></div>
    `;
    const iframe = document.createElement("iframe");
    document.body.appendChild(iframe);
    const manager = new NavigationManager(iframe);
    const mockRpcCall = vi.fn().mockResolvedValue(1);
    // @ts-ignore — accessing private for testing
    manager.rpc.call = mockRpcCall;
    return { manager, mockRpcCall, cleanup: () => { document.body.innerHTML = ""; } };
  }

  test("hash-nav on same pathname sets scrollIntent to 'user'", () => {
    const { manager, cleanup } = makeManager();

    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/article.html", title: "Article" });
    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/article.html#section-2", title: "Article" });

    // @ts-ignore — accessing private for testing
    expect(manager.scrollIntent).toBe("user");
    cleanup();
  });

  test("same-URL reload does NOT call scrollToSourceLine when scrollIntent is 'user'", () => {
    const { manager, mockRpcCall, cleanup } = makeManager();

    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/article.html", title: "Article" });
    manager.setLastEditorPosition(42, "section-3");

    // User clicks a hash link — intent flips to 'user'
    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/article.html#section-2", title: "Article" });

    // Simulate a rebuild (same-URL reload)
    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/article.html#section-2", title: "Article" });

    expect(mockRpcCall).not.toHaveBeenCalledWith("scrollToSourceLine", expect.anything(), expect.anything());
    cleanup();
  });

  test("same-URL reload DOES call scrollToSourceLine when scrollIntent is 'editor'", () => {
    const { manager, mockRpcCall, cleanup } = makeManager();

    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/article.html", title: "Article" });
    manager.setLastEditorPosition(42, "section-3");

    // scrollIntent stays 'editor' (no hash nav) — rebuild fires scroll restoration
    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/article.html", title: "Article" });

    expect(mockRpcCall).toHaveBeenCalledWith("scrollToSourceLine", 42, false, false, false);
    cleanup();
  });

  test("claimScrollForEditor() flips scrollIntent back to 'editor'", () => {
    const { manager, cleanup } = makeManager();

    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/article.html", title: "Article" });
    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/article.html#section-2", title: "Article" });

    // @ts-ignore — accessing private for testing
    expect(manager.scrollIntent).toBe("user");

    manager.claimScrollForEditor();

    // @ts-ignore — accessing private for testing
    expect(manager.scrollIntent).toBe("editor");
    cleanup();
  });

  test("after claimScrollForEditor, same-URL reload calls scrollToSourceLine again", () => {
    const { manager, mockRpcCall, cleanup } = makeManager();

    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/article.html", title: "Article" });
    manager.setLastEditorPosition(10, null);

    // Hash nav → user owns it
    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/article.html#intro", title: "Article" });

    // Editor scrolls → reclaims
    manager.claimScrollForEditor();

    // Rebuild (same URL with hash)
    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/article.html#intro", title: "Article" });

    expect(mockRpcCall).toHaveBeenCalledWith("scrollToSourceLine", 10, false, false, false);
    cleanup();
  });

  test("hash nav to a different pathname does NOT set scrollIntent to 'user'", () => {
    const { manager, cleanup } = makeManager();

    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/page-a.html", title: "A" });
    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/page-b.html#section", title: "B" });

    // Different pathname — editor should still own new page
    // @ts-ignore — accessing private for testing
    expect(manager.scrollIntent).toBe("editor");
    cleanup();
  });

  test("editor-driven echo nav to new URL sets scrollIntent to 'editor'", () => {
    const { manager, cleanup } = makeManager();

    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/article.html", title: "Article" });
    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/article.html#section-2", title: "Article" });

    // @ts-ignore — accessing private for testing
    expect(manager.scrollIntent).toBe("user");

    // Editor drives a nav to a new page
    manager.markEditorNavigation("http://localhost:8080/other.html");
    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/other.html", title: "Other" });

    // @ts-ignore — accessing private for testing
    expect(manager.scrollIntent).toBe("editor");
    cleanup();
  });

  test("editor-driven echo nav to hash URL on same page does NOT leave scrollIntent as 'user'", () => {
    // isHashNavOnSamePage fires 'user' for the hash URL, but the echo branch
    // immediately corrects it back to 'editor'. This tests that false-positive
    // correction path.
    const { manager, cleanup } = makeManager();

    // Start on article.html
    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/article.html", title: "Article" });

    // Editor drives a nav to a hash section on the same page
    manager.markEditorNavigation("http://localhost:8080/article.html#editor-anchor");
    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/article.html#editor-anchor", title: "Article" });

    // isHashNavOnSamePage set 'user', but echo branch corrected it to 'editor'
    // @ts-ignore — accessing private for testing
    expect(manager.scrollIntent).toBe("editor");
    cleanup();
  });

  test("reset() restores scrollIntent to 'editor'", () => {
    const { manager, cleanup } = makeManager();

    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/article.html", title: "Article" });
    // @ts-ignore — calling private for testing
    manager.handleNavigationUpdate({ url: "http://localhost:8080/article.html#section", title: "Article" });

    // @ts-ignore — accessing private for testing
    expect(manager.scrollIntent).toBe("user");

    manager.reset();

    // @ts-ignore — accessing private for testing
    expect(manager.scrollIntent).toBe("editor");
    cleanup();
  });
});

/**
 * Tests for the moss-swipe-nav message — trackpad swipe back/forward.
 * The iframe bridge detects the gesture (see bridge/swipe-gesture.ts) and
 * posts { type: "moss-swipe-nav", direction }; the shell routes it into the
 * exact same goBack()/goForward() path as the titlebar buttons, so the
 * canGoBack/canGoForward guards and history bookkeeping apply unchanged.
 */
describe("NavigationManager moss-swipe-nav handling", () => {
  let navManager: NavigationManager;
  let backSpy: ReturnType<typeof vi.spyOn>;
  let forwardSpy: ReturnType<typeof vi.spyOn>;

  let iframe: HTMLIFrameElement;

  beforeEach(() => {
    document.body.innerHTML = `
      <button id="btn-nav-back"></button>
      <button id="btn-nav-forward"></button>
      <span id="moss-url-text">/</span>
      <div class="moss-device-toggle"></div>
      <div class="moss-preview-container"></div>
    `;
    iframe = document.createElement("iframe");
    document.body.appendChild(iframe);
    navManager = new NavigationManager(iframe);
    backSpy = vi.spyOn(navManager, "goBack").mockResolvedValue();
    forwardSpy = vi.spyOn(navManager, "goForward").mockResolvedValue();
  });

  afterEach(() => {
    document.body.innerHTML = "";
    localStorage.removeItem("moss.preview.swipe");
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  function swipe(direction: string): void {
    window.dispatchEvent(
      new MessageEvent("message", {
        data: { type: "moss-swipe-nav", direction },
        source: iframe.contentWindow,
      }),
    );
  }

  test("direction 'back' triggers goBack()", () => {
    swipe("back");
    expect(backSpy).toHaveBeenCalledTimes(1);
    expect(forwardSpy).not.toHaveBeenCalled();
  });

  test("direction 'forward' triggers goForward()", () => {
    swipe("forward");
    expect(forwardSpy).toHaveBeenCalledTimes(1);
    expect(backSpy).not.toHaveBeenCalled();
  });

  test("unknown direction is ignored", () => {
    swipe("sideways");
    expect(backSpy).not.toHaveBeenCalled();
    expect(forwardSpy).not.toHaveBeenCalled();
  });

  test("a message whose source is not the preview iframe is ignored", () => {
    // Preview content is untrusted site JS; a nested third-party embed can
    // post to the shell via window.top. Only the iframe itself may navigate.
    window.dispatchEvent(
      new MessageEvent("message", {
        data: { type: "moss-swipe-nav", direction: "back" },
      }),
    );
    expect(backSpy).not.toHaveBeenCalled();
  });

  test("kill-switch localStorage moss.preview.swipe=0 disables swipe navigation", () => {
    localStorage.setItem("moss.preview.swipe", "0");
    swipe("back");
    expect(backSpy).not.toHaveBeenCalled();
  });

  test("swipes are ignored during the post-navigation cooldown (momentum tail)", () => {
    vi.useFakeTimers();
    // A navigation echo arrives (page just loaded)...
    window.dispatchEvent(
      new MessageEvent("message", {
        data: { type: "moss-navigation", url: "http://localhost:8080/a/", title: "A" },
        source: iframe.contentWindow,
      }),
    );
    // ...leftover trackpad momentum re-fires a swipe right after: dropped.
    swipe("back");
    expect(backSpy).not.toHaveBeenCalled();

    // A deliberate swipe after the cooldown works.
    vi.advanceTimersByTime(700);
    swipe("back");
    expect(backSpy).toHaveBeenCalledTimes(1);
  });
});

/**
 * History reconciliation — the parent stack must track the iframe's REAL
 * session history, not its own guesses.
 *
 * A forward entry the iframe does not have is a wedge: history.forward() no-ops
 * silently (no echo), leaving pendingNavType stuck and the forward button dead
 * forever. Two fixes under
 * test: (1) echoes carry the document's real navigation type
 * (performance 'back_forward' vs 'navigate') and classification trusts it
 * over pendingNavType guessing; (2) a no-echo watchdog after goBack/goForward
 * drops the phantom direction and fixes the buttons.
 */
describe("NavigationManager history reconciliation", () => {
  let navManager: NavigationManager;
  let mockRpcCall: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    document.body.innerHTML = `
      <button id="btn-nav-back"></button>
      <button id="btn-nav-forward"></button>
      <span id="moss-url-text">/</span>
      <div class="moss-device-toggle"></div>
      <div class="moss-preview-container"></div>
    `;
    const iframe = document.createElement("iframe");
    document.body.appendChild(iframe);
    navManager = new NavigationManager(iframe);
    mockRpcCall = vi.fn().mockResolvedValue(undefined);
    // @ts-ignore — private access for testing
    navManager.rpc.call = mockRpcCall;
  });

  afterEach(() => {
    document.body.innerHTML = "";
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  function echo(url: string, navType?: string): void {
    // @ts-ignore — private access for testing
    navManager.handleNavigationUpdate({ url, title: "", navType });
  }

  /** home → b (new nav), then traverse back to home: stack [home, b], index 0. */
  function seedBackAtHome(): void {
    echo("http://localhost:8080/", "navigate");
    echo("http://localhost:8080/b/", "navigate");
    // @ts-ignore — private
    navManager.pendingNavType = "back";
    echo("http://localhost:8080/", "back_forward");
  }

  test("a back_forward echo without pendingNavType moves the index (no truncation)", () => {
    echo("http://localhost:8080/", "navigate");
    echo("http://localhost:8080/b/", "navigate");
    // Traversal echo arrives with NO pendingNavType (e.g. some other actor
    // traversed, or pending was cleared) — must move back, keep forward.
    echo("http://localhost:8080/", "back_forward");
    // @ts-ignore — private
    expect(navManager.history._index).toBe(0);
    // @ts-ignore — private
    expect(navManager.history.canGoForward()).toBe(true);
    expect((document.getElementById("btn-nav-forward") as HTMLButtonElement).disabled).toBe(false);
  });

  test("a navigate echo never consumes a stale pendingNavType as a traversal", () => {
    echo("http://localhost:8080/", "navigate");
    echo("http://localhost:8080/b/", "navigate");
    // Stale pending (its traversal never happened), then a REAL new load:
    // @ts-ignore — private
    navManager.pendingNavType = "forward";
    echo("http://localhost:8080/c/", "navigate");
    // Must be classified as a new navigation pushed after b — NOT index++.
    // @ts-ignore — private
    expect(navManager.history._stack).toEqual([
      "http://localhost:8080/",
      "http://localhost:8080/b/",
      "http://localhost:8080/c/",
    ]);
    // @ts-ignore — private
    expect(navManager.history._index).toBe(2);
  });

  test("goForward with no echo (phantom entry) heals: forward dropped, button disabled", async () => {
    vi.useFakeTimers();
    seedBackAtHome();
    const fwdButton = document.getElementById("btn-nav-forward") as HTMLButtonElement;
    expect(fwdButton.disabled).toBe(false);

    await navManager.goForward(); // RPC resolves, but the iframe never echoes
    vi.advanceTimersByTime(3000);

    expect(fwdButton.disabled).toBe(true);
    // @ts-ignore — private
    expect(navManager.pendingNavType).toBe(null);
    // A later real navigation classifies cleanly as new.
    echo("http://localhost:8080/a/", "navigate");
    // @ts-ignore — private
    expect(navManager.history._stack).toEqual([
      "http://localhost:8080/",
      "http://localhost:8080/a/",
    ]);
  });

  test("the watchdog does not fire when the echo arrives in time", async () => {
    vi.useFakeTimers();
    seedBackAtHome();
    await navManager.goForward();
    echo("http://localhost:8080/b/", "back_forward"); // traversal completed
    vi.advanceTimersByTime(3000);
    const fwdButton = document.getElementById("btn-nav-forward") as HTMLButtonElement;
    // At the top of the stack now — forward disabled because we're at b,
    // but the stack itself is intact (back still available).
    // @ts-ignore — private
    expect(navManager.history._stack.length).toBe(2);
    // @ts-ignore — private
    expect(navManager.history._index).toBe(1);
    expect(fwdButton.disabled).toBe(true);
    expect((document.getElementById("btn-nav-back") as HTMLButtonElement).disabled).toBe(false);
  });
});


describe("NavigationManager request intent", () => {
  test("requested route owns the issue-to-echo gap and obsolete echoes are ignored", () => {
    const iframe = document.createElement("iframe");
    const manager = new NavigationManager(iframe);
    manager.beginNavigation("http://localhost:8080/posts/");
    expect(manager.getCurrentUrl()).toBe("http://localhost:8080/posts/");
    (manager as any).handleNavigationUpdate({ url: "http://localhost:8080/untitled/", notFound: true });
    expect(manager.getCurrentUrl()).toBe("http://localhost:8080/posts/");
    (manager as any).handleNavigationUpdate({ url: "http://localhost:8080/posts/index.html", notFound: false });
    expect(manager.getActivePage()?.url).toBe("http://localhost:8080/posts/index.html");
    manager.destroy();
  });
  test("an expected missing-page echo shows its route instead of an error title in chrome", () => {
    document.body.innerHTML = '<span id="moss-url-text"></span>';
    const manager = new NavigationManager(document.createElement("iframe"));
    manager.beginNavigation("http://localhost:8080/posts/");
    const onResult = vi.fn();
    manager.setNavigationResultListener(onResult);
    (manager as any).handleNavigationUpdate({ url: "http://localhost:8080/posts/", title: "Not Found", notFound: true });
    expect(document.getElementById("moss-url-text")!.textContent).toBe("/posts/");
    expect(onResult).toHaveBeenCalledWith({ url: "http://localhost:8080/posts/", notFound: true });
    (manager as any).handleNavigationUpdate({ url: "http://localhost:8080/posts/", title: "Posts", notFound: false });
    expect(document.getElementById("moss-url-text")!.textContent).toBe("Posts");
    manager.destroy();
    document.body.innerHTML = '';
  });

  test.each([['back', 1, 0], ['forward', 0, 1]] as const)("%s replaces a still-pending navigation intent", async (direction, index, destination) => {
    const iframe = document.createElement("iframe");
    const manager = new NavigationManager(iframe);
    const history = (manager as any).history;
    history._stack = ['http://localhost:8080/one/', 'http://localhost:8080/two/'];
    history._index = index;
    history._initialized = true;
    (manager as any).rpc = { historyBack: vi.fn().mockResolvedValue(undefined), historyForward: vi.fn().mockResolvedValue(undefined), destroy: vi.fn() };
    manager.beginNavigation('http://localhost:8080/pending/');
    if (direction === 'back') await manager.goBack();
    else await manager.goForward();
    (manager as any).handleNavigationUpdate({ url: history._stack[destination], notFound: false });
    expect(manager.getActivePage()?.url).toBe(history._stack[destination]);
    manager.destroy();
  });

});

describe("NavigationManager navigation deadline", () => {
  const origin = "http://localhost:8080";
  const echoAt = (m: NavigationManager, path: string) =>
    (m as any).handleNavigationUpdate({ url: `${origin}${path}`, title: "t" });
  let manager: NavigationManager;
  let required: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    vi.useFakeTimers();
    manager = new NavigationManager(document.createElement("iframe"));
    required = vi.fn();
    manager.setPreviewRequirementListener(required);
  });
  afterEach(() => {
    manager.destroy();
    vi.useRealTimers();
  });

  test("a report from another page neither completes nor disarms a pending navigation; at the deadline the build is told the shown page", () => {
    echoAt(manager, "/y/");
    manager.beginNavigation(`${origin}/x/`);
    vi.advanceTimersByTime(10000);
    echoAt(manager, "/y/"); // mismatched: ignored
    expect(manager.isNavigationPending()).toBe(true);
    required.mockClear();

    vi.advanceTimersByTime(19999);
    expect(manager.isNavigationPending()).toBe(true);
    vi.advanceTimersByTime(1);
    expect(manager.isNavigationPending()).toBe(false);
    expect(required).toHaveBeenLastCalledWith("/y/");

    // the page actually shown is trusted again
    required.mockClear();
    echoAt(manager, "/y/");
    expect(manager.getActivePage()?.url).toBe(`${origin}/y/`);
  });

  test("a traversal confirmed while its history call was in flight leaves nothing to abandon at the timer", async () => {
    const history = (manager as any).history;
    history._stack = [`${origin}/`, `${origin}/b/`];
    history._index = 0;
    history._initialized = true;
    let finish: () => void = () => {};
    (manager as any).rpc = {
      historyForward: vi.fn(() => new Promise<void>((r) => { finish = r; })),
      destroy: vi.fn(),
    };
    const pending = manager.goForward();
    echoAt(manager, "/b/"); // the echo beats the RPC reply
    finish();
    await pending;
    required.mockClear();
    vi.mocked(info).mockClear();
    vi.advanceTimersByTime(2500);
    expect(required).not.toHaveBeenCalled();
    expect(vi.mocked(info).mock.calls.some(([m]) => String(m).includes("abandoned"))).toBe(false);
  });

  test("ordinary navigation puts no page URL in an info line", () => {
    // Info lines reach Send Logs and support log streams; page URLs stay at debug.
    vi.mocked(info).mockClear();
    echoAt(manager, "/");
    echoAt(manager, "/a/");
    manager.beginNavigation(`${origin}/b/`);
    echoAt(manager, "/b/");
    expect(vi.mocked(info).mock.calls.filter(([m]) => String(m).includes(origin))).toEqual([]);
  });

  test("a mismatched report during a pending forward leaves its deadline armed so the rollback still happens", async () => {
    const history = (manager as any).history;
    history._stack = [`${origin}/`, `${origin}/b/`];
    history._index = 0;
    history._initialized = true;
    (manager as any).rpc = { historyForward: vi.fn().mockResolvedValue(undefined), destroy: vi.fn() };
    await manager.goForward();
    vi.advanceTimersByTime(1000);
    echoAt(manager, "/elsewhere/");
    vi.advanceTimersByTime(1500);
    expect(history._stack).toEqual([`${origin}/`]);
  });

  test("an accepted report clears the load fallback, so no synthesized update follows", () => {
    const iframe = document.createElement("iframe");
    const m = new NavigationManager(iframe);
    m.beginNavigation(`${origin}/x/`);
    iframe.src = `${origin}/x/`;
    iframe.dispatchEvent(new Event("load"));
    (m as any).handleNavigationUpdate({ url: `${origin}/x/`, title: "x" });
    const spy = vi.spyOn(m as any, "handleNavigationUpdate");
    vi.advanceTimersByTime(500);
    expect(spy).not.toHaveBeenCalled();
    m.destroy();
  });

  test("a late report from the abandoned page is an ordinary arrival and the build is told it", () => {
    echoAt(manager, "/y/");
    manager.beginNavigation(`${origin}/x/`);
    const before = manager.getRequestRevision();
    vi.advanceTimersByTime(30000);
    expect(manager.getRequestRevision()).toBeGreaterThan(before); // work queued for the dead navigation loses authority
    required.mockClear();
    echoAt(manager, "/x/");
    expect(manager.getActivePage()?.url).toBe(`${origin}/x/`);
    expect(required).toHaveBeenLastCalledWith("/x/");
  });

  test("a pending navigation is not reloaded or morphed over", async () => {
    const call = vi.fn().mockResolvedValue(undefined);
    (manager as any).rpc = { call, destroy: vi.fn() };
    const post = vi.fn();
    (manager as any).iframe = { contentWindow: { postMessage: post }, addEventListener: vi.fn() };
    manager.beginNavigation(`${origin}/x/`);
    await expect(manager.reload()).resolves.toBe(true);
    await expect(manager.morph()).resolves.toBe(false);
    expect(call).not.toHaveBeenCalled();
    expect(post).not.toHaveBeenCalled();
    (manager as any).iframe = null;
  });

  test.each([
    ["/index.html", "/"],
    ["/foo.html", "/foo"],
    ["/%E4%B8%AD%E6%96%87.html", "/中文"],
  ])("the build is told the same spelling at begin and at the echo for %s", (path, shown) => {
    manager.beginNavigation(`${origin}${path}`);
    const atBegin = required.mock.calls.at(-1)![0];
    echoAt(manager, path);
    expect(required.mock.calls.at(-1)![0]).toBe(atBegin);
    expect(atBegin).toBe(shown);
  });

  test("each new navigation re-arms the deadline without abandoning and reports", () => {
    echoAt(manager, "/y/");
    manager.beginNavigation(`${origin}/x/`);
    vi.advanceTimersByTime(25000);
    manager.beginNavigation(`${origin}/x/`); // a recheck of the same page
    vi.advanceTimersByTime(25000);
    expect(manager.isNavigationPending()).toBe(true);
    expect(required.mock.calls.filter(([p]) => p === "/x/")).toHaveLength(2);
    vi.advanceTimersByTime(5000);
    expect(manager.isNavigationPending()).toBe(false);
  });

  test("a navigation to a document that cannot report in ends through the load fallback", () => {
    const iframe = document.createElement("iframe");
    iframe.src = `${origin}/`;
    const m = new NavigationManager(iframe);
    m.beginNavigation(`${origin}/feed.xml`);
    iframe.src = `${origin}/feed.xml`;
    iframe.dispatchEvent(new Event("load"));
    vi.advanceTimersByTime(500);
    expect(m.isNavigationPending()).toBe(false);
    expect(m.getActivePage()?.url).toBe(`${origin}/feed.xml`);
    m.destroy();
  });

  test("the load fallback does not confirm when the iframe address is not the intended page", () => {
    const iframe = document.createElement("iframe");
    const m = new NavigationManager(iframe);
    m.beginNavigation(`${origin}/x/`);
    iframe.src = `${origin}/y/`;
    iframe.dispatchEvent(new Event("load"));
    vi.advanceTimersByTime(500);
    expect(m.isNavigationPending()).toBe(true);
    expect(m.getActivePage()).toBeNull();
    m.destroy();
  });

  test("a requirement dropped at begin is sent again at the echo", () => {
    const m = new NavigationManager(document.createElement("iframe"));
    m.beginNavigation(`${origin}/x/`); // no listener yet: dropped
    const late = vi.fn();
    m.setPreviewRequirementListener(late);
    (m as any).handleNavigationUpdate({ url: `${origin}/x/`, title: "x" });
    expect(late).toHaveBeenCalledWith("/x/");
    m.destroy();
  });

  test("back and forward time out at the traversal duration through the shared timer and roll back history", async () => {
    const history = (manager as any).history;
    history._stack = [`${origin}/`, `${origin}/b/`];
    history._index = 0;
    history._initialized = true;
    (manager as any).rpc = { historyForward: vi.fn().mockResolvedValue(undefined), destroy: vi.fn() };
    await manager.goForward();
    vi.advanceTimersByTime(2499);
    expect(history._stack).toHaveLength(2);
    vi.advanceTimersByTime(1);
    expect(history._stack).toEqual([`${origin}/`]);
    expect((manager as any).pendingNavType).toBeNull();
  });
});
