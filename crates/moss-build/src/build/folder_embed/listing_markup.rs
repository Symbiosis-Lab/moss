//! Markup a folder listing writes only when it has entries, which `when:` and
//! drafts can empty on the day of the build.

/// Whether a class, id (`#`) or attribute (`[`) name may be written by a
/// listing. Some prefixes also cover names that pages outside a listing carry,
/// so the caller counts a name as listing-only just when no built page has it.
/// `tests/listing_classes_test.rs` checks the list covers what listings emit.
pub fn is_listing_name(kind: char, name: &str) -> bool {
    const CLASS_PREFIXES: &[&str] = &[
        "moss-card", "moss-folder-", "moss-prefix-link", "moss-event", "moss-when", "moss-credit", "moss-lang-mark",
        "moss-child-section-divider", "moss-embed-more", "moss-calendar-link",
    ];
    const ATTRIBUTE_PREFIXES: &[&str] = &["data-layout", "data-embed", "data-list", "data-cover", "data-when", "datetime"];
    match kind {
        // The list rows' bare `date` and `title` spans and the minimal year group's `minimal`.
        '.' => matches!(name, "date" | "title" | "minimal") || CLASS_PREFIXES.iter().any(|p| name.starts_with(p)),
        '[' => ATTRIBUTE_PREFIXES.iter().any(|p| name.starts_with(p)),
        _ => false,
    }
}
