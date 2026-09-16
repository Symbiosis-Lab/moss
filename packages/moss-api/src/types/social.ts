/** One comment in the .moss/data/social/*.json shared standard.
 *  See moss/docs/reference/social-data-standard.md.
 *  @category Social */
export interface SocialComment {
  id: string;
  source: string;
  content: string;
  createdAt: string;
  author: { displayName: string; url?: string };
  replyToId?: string;
  /**
   * Moderation state. Absent or "active" = visible; anything else is filtered
   * at read time. Well-known values: "active" | "removed" | "archived" |
   * "banned" | "collapsed". The Rust side treats this as an open string so
   * future states do not require a client update.
   */
  state?: string;
}

/**
 * The social data for a single article — currently its comments.
 * @category Social
 */
export interface SocialArticleData {
  comments: SocialComment[];
}

/**
 * A `.moss/data/social/*.json` file: the schema version plus every article's
 * social data, keyed by article path.
 * @category Social
 */
export interface SocialDataFile {
  schemaVersion: string;
  articles: Record<string, SocialArticleData>;
}
