//! Values shared by editor hosts.

/// What an editor save did. A failure to write is an `Err`; this is the
/// outcome of a save that did not fail.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum SaveResult {
    /// Written. `hash` is the [`crate::page::content_hash`] of the bytes now on disk, the
    /// base the editor's next save must name. `displaced` is the history
    /// version that keeps the bytes this save replaced, when the save was
    /// asked to keep them.
    Saved {
        hash: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        displaced: Option<String>,
    },
    /// Nothing written: the file is no longer at this path, renamed or deleted
    /// outside moss since it was opened. Not a failure — the file event that
    /// follows says which, and the text stays unsaved in the editor until then.
    Gone,
    /// Nothing written: another writer changed the file since the editor last
    /// read it. `disk` is what is there now, so the editor can merge it into
    /// the unsaved text and save again with `disk.hash` as the base.
    Conflict { disk: ParsedFrontmatter },
    /// Nothing written: the save was to replace another writer's text and
    /// moss could not keep that text first, because history is turned off for
    /// the site or the file is in no site.
    NotKept,
}

/// Frontmatter parsing result exposed to the frontend.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ParsedFrontmatter {
    /// [`crate::page::content_hash`] of the file's bytes as read, before any newline
    /// normalisation. Empty while [`still_arriving`](Self::still_arriving).
    pub hash: String,
    /// Parsed frontmatter as a JSON value (object).
    pub frontmatter: serde_json::Value,
    /// The markdown body (everything after the closing `---`).
    pub body: String,
    /// Whether the document had frontmatter delimiters.
    pub has_frontmatter: bool,
    /// serde_yaml error message when a delimited `---...---` block failed to
    /// parse. `None` when it parsed cleanly or there was no block. The frontend
    /// can render an "invalid YAML" banner off this so a malformed block flags an
    /// error instead of silently showing a blank frontmatter form. `body` still
    /// holds the whole document so the author can repair the raw block in CM6.
    #[serde(default)]
    pub frontmatter_error: Option<String>,
    /// The file is real and its bytes are not here yet — a cloud placeholder
    /// the provider has not handed back. Every other field is
    /// empty, and the editor must mount its "still arriving" pane rather than
    /// this document: an empty CM6 buffer over a file with content is the same
    /// lie as the errno toast this replaced.
    ///
    /// Set only by [`super::classify_source_read`], which is the one place in the
    /// editor that reads the cloud's answer. Never true together with content.
    #[serde(default)]
    pub still_arriving: bool,
}

impl ParsedFrontmatter {
    /// The document for a source whose bytes are still in the cloud.
    pub fn still_arriving() -> Self {
        Self {
            hash: String::new(),
            frontmatter: serde_json::Value::Object(serde_json::Map::new()),
            body: String::new(),
            has_frontmatter: false,
            frontmatter_error: None,
            still_arriving: true,
        }
    }
}

/// What a source file becomes on the built site. "Which slot, if any" rather
/// than a boolean, so a future slot file needs no new command.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SourceRole {
    /// Layout slot this file fills INSTEAD of becoming a page (`"footer-left"`).
    pub slot: Option<String>,
    /// Language tree the file lives in (`"zh-hans"`); `None` for the default
    /// tree. Reported for EVERY file, not just slot files: the shell compares
    /// the tree of the slot file being edited against the tree of the page the
    /// preview is showing, and needs the same answer for both sides of that
    /// comparison. One command, one definition of "which language tree" —
    /// asking a second command for the page side would be a place to drift.
    pub language: Option<String>,
    /// Whether the build turns this file's extension into a page at all.
    ///
    /// Reported so the preview follower can tell "not built YET" (keep
    /// retrying on the next build) from "never a page" (stop, and say so).
    /// It answers the extension question only — `slot` above still overrides
    /// it, and `.moss/` inputs are a path fact the frontend already owns.
    ///
    /// This rides on `describe_source_role` rather than becoming a set in
    /// TypeScript for the same reason `slot` does: the authority is
    /// `build::scan::classify::is_page_source`, and a second copy of it is the
    /// Rust↔TS parity trap.
    pub can_be_page: bool,
}

