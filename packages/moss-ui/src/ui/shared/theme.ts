/**
 * The app-shell theme: one key, one owner, every webview a subscriber.
 *
 * Model:
 *   - `localStorage["preview-theme"]` is the moon-toggle override, or absent.
 *   - Resolved theme = override || OS. The pre-paint script in each
 *     webview's HTML page evaluates the same expression before CSS parses;
 *     a desktop app test pins the three copies equal.
 *   - A live OS change clears the override and follows the OS.
 *
 * Cross-webview sync is the `storage` event: the shell, editor and launcher
 * share one origin, so a write in any of them fires `storage` in the other
 * two. No Tauri event, no basis key. This file must stay import-free — the
 * launcher pulls it in and is bundle-size gated.
 *
 * No disposer: all three webviews are process-lifetime.
 */

export type Theme = "light" | "dark";

const KEY = "preview-theme";
const OS_DARK = "(prefers-color-scheme: dark)";

function osTheme(): Theme {
  return window.matchMedia(OS_DARK).matches ? "dark" : "light";
}

/** The theme the person chose over the OS's, or null while following the OS. */
export function themeOverride(): Theme | null {
  return localStorage.getItem(KEY) as Theme | null;
}

/** The override if set, else the OS. Same expression as the pre-paint scripts. */
export function resolveTheme(): Theme {
  return (localStorage.getItem(KEY) as Theme | null) || osTheme();
}

function applyTheme(theme: Theme): void {
  document.documentElement.setAttribute("data-theme", theme);
}

/** The moon toggle. `storage` does not fire in the writing document, so apply here. */
export function setThemeOverride(theme: Theme): void {
  localStorage.setItem(KEY, theme);
  applyTheme(theme);
}

/**
 * Apply the resolved theme now and keep `<html data-theme>` in step with the
 * OS and with the other webviews.
 */
export function followTheme(): void {
  applyTheme(resolveTheme());
  window.matchMedia(OS_DARK).addEventListener("change", (e) => {
    localStorage.removeItem(KEY);
    applyTheme(e.matches ? "dark" : "light");
  });
  window.addEventListener("storage", (e) => {
    if (e.key === KEY) applyTheme(resolveTheme());
  });
}

/** Read the current theme from the attribute `followTheme` maintains. */
export function isDarkTheme(): boolean {
  return document.documentElement.getAttribute("data-theme") === "dark";
}
