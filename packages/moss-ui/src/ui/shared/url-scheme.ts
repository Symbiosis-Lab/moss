/**
 * Is this a URL moss may hand to the OS, or put in an `<a href>`?
 *
 * http(s) only. Every other scheme reaches an OS handler — `file://` opens a
 * local path in Finder, and any installed app can claim a scheme of its own.
 *
 * `openUrl` applies this itself, so a caller needs it only to *render*
 * differently — a plugin address that moss cannot open is shown as copyable
 * text rather than as a link that would silently do nothing.
 *
 * It lives in `shared/` rather than beside `openUrl` because the toast
 * renderer needs it and may not import `platform/`. A toast action's URL comes
 * from the plugin that raised the toast, and the click handler calls
 * `preventDefault`, so the open goes through `openUrl` and is gated — but the
 * `href` stays live for a middle-click or the context menu's "Open link",
 * which is a route to the OS handler that never passes `openUrl` at all.
 */
export function isHttpUrl(url: string): boolean {
  return url.startsWith('http://') || url.startsWith('https://');
}
