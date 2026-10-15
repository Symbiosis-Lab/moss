/**
 * The `listing` hook: draw a folder's listing in a style the plugin
 * contributes (`contributes.listing_styles`).
 */

/**
 * One page, as a listing plugin sees it. The listing page itself, every page
 * it lists and every subfolder's home page have this shape.
 *
 * @category Listing styles
 */
export interface PageRecord {
  title: string;
  /** The page's address in the built site, as moss names it (`board/index.html`). */
  url_path: string;
  /** Where the page is, written so it resolves from the listing page. */
  url: string;
  lang: string;
  translations: { lang: string; url_path: string }[];
  /**
   * The page's frontmatter as moss resolved it: values inherited from a
   * parent folder are applied and `title` is filled in when the page set none.
   */
  frontmatter: Record<string, unknown>;
  /**
   * The page's declared fields (`[fields.*]` in `.moss/config.toml`) that its own
   * frontmatter carries and that parse, each as its typed value. A date is
   * `{ value, precision, qualifier }`; a period is `{ start, end, open, text }` with
   * `start` and `end` in that date shape and `open` true only for an interval with
   * no end; a choice is `{ value, label }` with the label in this page's language;
   * a count is `{ number, noun, text }`; a page link is `{ target, alias? }`, with `alias` the text after `|` in `[[target|alias]]` when there is one. A value that does not parse is left out
   * here and stays in `frontmatter`. Values a folder's `cascade:` supplies are not
   * typed here.
   */
  fields: Record<string, unknown>;
  /**
   * For each declared field whose value does not fit, the reason it does not, as
   * the build warning states it. Empty when every value fits.
   */
  field_errors: Record<string, string>;
  /**
   * The rendered body. Relative links and image sources are rewritten to
   * resolve from the listing page, where the HTML is drawn. Empty for a
   * subfolder that has no home page.
   */
  html: string;
  /**
   * The folder the page sits in, relative to the listed folder: empty for the
   * listing page and for its direct children.
   */
  folder: string;
}

/**
 * The argument of the `listing` export.
 *
 * `model` is the version of this shape. Fields may be added within a
 * version; renaming or removing one bumps it.
 *
 * @category Listing styles
 */
export interface ListingContext {
  model: 1;
  /** The style id the page asked for. */
  style: string;
  page: PageRecord;
  /** Exactly the pages moss's own listing of the folder would show, in the order it shows them. */
  pages: PageRecord[];
  /** The listed folder's subfolders that have a home page, in the listing's own order (its sort applied to the home pages). */
  folders: PageRecord[];
  /** `terms[namespace][name]` is where that term's page is, written like a record's `url` so it resolves from the listing page, for every name `pages` use. */
  terms: Record<string, Record<string, string>>;
  /** Every declared field, by name. Choice labels are in the listing page's language. */
  declarations: Record<string, FieldDeclaration>;
}

/**
 * One `[fields.<name>]` declaration. `choices` is present only for a choice, in
 * declared order.
 *
 * @category Listing styles
 */
export interface FieldDeclaration {
  /** The declared type: `text`, `number`, `date`, `url`, `choice`, `period`, `count`, `page` or `pages`. */
  type: string;
  choices?: { value: string; label: string }[];
  /** Where the field is placed on the page, or `null`. */
  place: string | null;
}

/**
 * What the `listing` export returns. A malformed result, a throw or a timeout
 * leaves moss's own listing in place with one build warning.
 *
 * @category Listing styles
 */
export interface ListingResult {
  /** Replaces the listing where moss would have drawn it. */
  html: string;
  /** Tags added to the listing page's `<head>`. A script is emitted with `defer`, so it runs after the page, listing included, is parsed. */
  head?: ({ stylesheet: string } | { script: string })[];
  /**
   * Files written into the build output inside the listing page's folder.
   * A path that is absolute, contains `..`, starts with `_moss/` or collides
   * with a page is refused with a warning.
   */
  files?: ({ path: string; text: string } | { path: string; base64: string })[];
  /**
   * Build warnings, prefixed with the plugin's name (`--strict` fails on them).
   * `{ text, page }` names a record's `url_path`; the warning then names that page's source file.
   */
  warnings?: (string | { text: string; page?: string })[];
  /** Overrides the style's declared `content_width` for this page. */
  content_width?: "wide" | "full";
}
