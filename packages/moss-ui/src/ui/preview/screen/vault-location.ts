// The wait/loading screen's quiet location line: where the folder actually
// is, once the title has already said what it's called.
//
// The folder's NAME comes from Rust (`VaultRoot::name()`, threaded through
// `__MOSS_BUILD_CONFIG__` / `config-ready` / `build-config-changed` — see
// the host location resolver's `describe_vault_location`) and this module never
// re-derives it. What IS this module's job is purely presentational: fitting
// an already-abbreviated parent path into one quiet line, and reading a
// detected cloud-sync root as a friendly location rather than its technical
// container path.

/** Default budget for the display text before middle-truncation kicks in.
 *  Generous enough that an ordinary `~/Documents/Sites` never truncates. */
const DEFAULT_MAX_LENGTH = 40;

const ELLIPSIS = "…";
const BREADCRUMB_SEP = " › ";

/**
 * Shorten `text` to `maxLength` by removing the MIDDLE, not the end.
 *
 * End-truncation (CSS `text-overflow: ellipsis`) throws away the part of a
 * path that says where you ended up (the leaf); start-truncation throws away
 * the part that says where you started (the drive/root). A path's two most
 * informative ends are its first and last segments, so the cut goes in
 * between.
 */
export function middleTruncate(text: string, maxLength: number = DEFAULT_MAX_LENGTH): string {
  if (text.length <= maxLength || maxLength <= ELLIPSIS.length) {
    return text.length <= maxLength ? text : text.slice(0, maxLength);
  }
  const keep = maxLength - ELLIPSIS.length;
  const head = Math.ceil(keep / 2);
  const tail = Math.floor(keep / 2);
  return text.slice(0, head) + ELLIPSIS + text.slice(text.length - tail);
}

/**
 * The location line's text, fitted to one line.
 *
 * A detected cloud provider overrides the raw parent path entirely — inside
 * `~/Library/Mobile Documents/iCloud~md~obsidian/Documents`, the parent is a
 * bundle id and a generic "Documents", neither of which tells the author
 * anything; "iCloud Drive › Obsidian Vault" does. The reveal button beside
 * the line shows exactly where the folder lives.
 */
export function formatVaultLocation(opts: {
  folderName: string;
  /** Home-abbreviated parent path (`~/Documents/Sites`), already computed by
   *  the backend — see `describe_vault_location`. */
  parentDisplay: string;
  /** The detected cloud provider's display name, if any. */
  cloudProviderLabel?: string | null;
  maxLength?: number;
}): string {
  const place = opts.cloudProviderLabel
    ? `${opts.cloudProviderLabel}${BREADCRUMB_SEP}${opts.folderName}`
    : opts.parentDisplay;
  return middleTruncate(place, opts.maxLength ?? DEFAULT_MAX_LENGTH);
}
