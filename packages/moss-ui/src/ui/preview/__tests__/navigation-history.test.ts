import { describe, test, expect, beforeEach } from "vitest";
import {
  createNavigationHistory,
  NavigationHistory,
} from "../navigation-history";

/**
 * Tests for NavigationHistory - Electron-compatible navigation tracking.
 *
 * This tests the ACTUAL implementation in navigation-history.ts,
 * which is used by NavigationManager in the parent window.
 *
 * Note: Navigation history is tracked in the parent window because
 * JavaScript globals don't persist across iframe page navigations.
 */

describe("NavigationHistory", () => {
  let navHistory: NavigationHistory;

  beforeEach(() => {
    navHistory = createNavigationHistory();
    navHistory._setInitialUrl("http://localhost/");
  });

  describe("uninitialized state", () => {
    test("starts uninitialized", () => {
      const fresh = createNavigationHistory();
      expect(fresh._initialized).toBe(false);
    });

    test("cannot go back when uninitialized", () => {
      const fresh = createNavigationHistory();
      expect(fresh.canGoBack()).toBe(false);
    });

    test("cannot go forward when uninitialized", () => {
      const fresh = createNavigationHistory();
      expect(fresh.canGoForward()).toBe(false);
    });

    test("has length of 0 when uninitialized", () => {
      const fresh = createNavigationHistory();
      expect(fresh.length()).toBe(0);
    });

    test("getCurrentUrl returns empty string when uninitialized", () => {
      const fresh = createNavigationHistory();
      expect(fresh.getCurrentUrl()).toBe("");
    });
  });

  describe("initial state (after _setInitialUrl)", () => {
    test("is initialized", () => {
      expect(navHistory._initialized).toBe(true);
    });

    test("cannot go back on initial load", () => {
      expect(navHistory.canGoBack()).toBe(false);
    });

    test("cannot go forward on initial load", () => {
      expect(navHistory.canGoForward()).toBe(false);
    });

    test("has length of 1", () => {
      expect(navHistory.length()).toBe(1);
    });

    test("has active index of 0", () => {
      expect(navHistory.getActiveIndex()).toBe(0);
    });

    test("getCurrentUrl returns initial URL", () => {
      expect(navHistory.getCurrentUrl()).toBe("http://localhost/");
    });
  });

  describe("after single navigation", () => {
    beforeEach(() => {
      navHistory._onNewNavigation("http://localhost/page1");
    });

    test("can go back", () => {
      expect(navHistory.canGoBack()).toBe(true);
    });

    test("cannot go forward", () => {
      expect(navHistory.canGoForward()).toBe(false);
    });

    test("has length of 2", () => {
      expect(navHistory.length()).toBe(2);
    });

    test("has active index of 1", () => {
      expect(navHistory.getActiveIndex()).toBe(1);
    });

    test("getCurrentUrl returns new URL", () => {
      expect(navHistory.getCurrentUrl()).toBe("http://localhost/page1");
    });
  });

  describe("after multiple navigations", () => {
    beforeEach(() => {
      navHistory._onNewNavigation("http://localhost/page1");
      navHistory._onNewNavigation("http://localhost/page2");
      navHistory._onNewNavigation("http://localhost/page3");
    });

    test("can go back", () => {
      expect(navHistory.canGoBack()).toBe(true);
    });

    test("cannot go forward", () => {
      expect(navHistory.canGoForward()).toBe(false);
    });

    test("has correct length", () => {
      expect(navHistory.length()).toBe(4);
    });

    test("has correct active index", () => {
      expect(navHistory.getActiveIndex()).toBe(3);
    });
  });

  describe("simulated back navigation (index decrement)", () => {
    beforeEach(() => {
      navHistory._onNewNavigation("http://localhost/page1");
      navHistory._onNewNavigation("http://localhost/page2");
    });

    test("decrementing index enables forward", () => {
      navHistory._index--;
      expect(navHistory.getActiveIndex()).toBe(1);
      expect(navHistory.canGoForward()).toBe(true);
    });

    test("preserves stack length after back", () => {
      navHistory._index--;
      expect(navHistory.length()).toBe(3);
    });

    test("cannot go back when at start", () => {
      navHistory._index--;
      navHistory._index--;
      expect(navHistory.getActiveIndex()).toBe(0);
      expect(navHistory.canGoBack()).toBe(false);
    });
  });

  describe("simulated forward navigation (index increment)", () => {
    beforeEach(() => {
      navHistory._onNewNavigation("http://localhost/page1");
      navHistory._onNewNavigation("http://localhost/page2");
      navHistory._index--; // back to page1
      navHistory._index--; // back to root
    });

    test("incrementing index enables back", () => {
      navHistory._index++;
      expect(navHistory.getActiveIndex()).toBe(1);
      expect(navHistory.canGoBack()).toBe(true);
    });

    test("can go forward multiple times", () => {
      navHistory._index++;
      navHistory._index++;
      expect(navHistory.getActiveIndex()).toBe(2);
      expect(navHistory.canGoForward()).toBe(false);
    });
  });

  describe("new navigation truncates forward history", () => {
    test("truncates forward entries when navigating from middle of stack", () => {
      navHistory._onNewNavigation("http://localhost/page1");
      navHistory._onNewNavigation("http://localhost/page2");
      navHistory._onNewNavigation("http://localhost/page3");
      // Stack: [/, page1, page2, page3], index: 3

      navHistory._index--; // index: 2 (at page2)
      navHistory._index--; // index: 1 (at page1)
      // Stack still: [/, page1, page2, page3], but we can go forward

      expect(navHistory.canGoForward()).toBe(true);

      // New navigation from page1
      navHistory._onNewNavigation("http://localhost/newpage");
      // Stack should now be: [/, page1, newpage], index: 2

      expect(navHistory.length()).toBe(3);
      expect(navHistory.getActiveIndex()).toBe(2);
      expect(navHistory.canGoForward()).toBe(false);
      expect(navHistory.canGoBack()).toBe(true);
    });

    test("preserves history before current position", () => {
      navHistory._onNewNavigation("http://localhost/page1");
      navHistory._onNewNavigation("http://localhost/page2");
      navHistory._index--; // at page1

      navHistory._onNewNavigation("http://localhost/newpage");

      // Should still be able to go back to root
      navHistory._index--;
      expect(navHistory.getActiveIndex()).toBe(1);
      navHistory._index--;
      expect(navHistory.getActiveIndex()).toBe(0);
      expect(navHistory.canGoBack()).toBe(false);
    });
  });

  describe("getCurrentUrl after navigation", () => {
    test("returns correct URL at each position", () => {
      navHistory._onNewNavigation("http://localhost/page1");
      navHistory._onNewNavigation("http://localhost/page2");

      expect(navHistory.getCurrentUrl()).toBe("http://localhost/page2");

      navHistory._index--;
      expect(navHistory.getCurrentUrl()).toBe("http://localhost/page1");

      navHistory._index--;
      expect(navHistory.getCurrentUrl()).toBe("http://localhost/");
    });
  });
});
