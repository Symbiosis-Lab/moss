//! A value paired with its origin. Lets downstream consumers decide
//! whether to honor the value as author intent (explicit) or override
//! it with an inferred default (auto-detected).
//!
//! Generic over `T` so any wrapped value participates: in v4 used for
//! `children_group` and `children_style` on `ParsedDocument`; future
//! fields with the same shape (e.g. cascaded layout fields) can adopt
//! it.

use serde::{Deserialize, Serialize};

/// Origin of a resolved value — used to gate whether a downstream
/// override should fire (auto-detected values lose to inference;
/// explicit author intent wins).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum ResolvedOrigin {
    /// Declared in the document's own frontmatter.
    Frontmatter,
    /// Inherited from an ancestor folder's `cascade:` block.
    Cascade,
    /// Auto-detected by a default-derivation rule.
    Auto,
}

/// A value plus the rule that produced it.
///
/// Use [`Resolved::is_explicit`] to branch: explicit author intent
/// (frontmatter or cascade) survives downstream overrides; auto-detected
/// defaults can be replaced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Resolved<T> where T: Clone {
    pub value: T,
    pub origin: ResolvedOrigin,
}

impl<T: Clone> Resolved<T> {
    pub fn frontmatter(value: T) -> Self {
        Self { value, origin: ResolvedOrigin::Frontmatter }
    }
    pub fn cascade(value: T) -> Self {
        Self { value, origin: ResolvedOrigin::Cascade }
    }
    pub fn auto(value: T) -> Self {
        Self { value, origin: ResolvedOrigin::Auto }
    }
    /// True iff origin is [`ResolvedOrigin::Frontmatter`] or
    /// [`ResolvedOrigin::Cascade`] — i.e. some author (current doc or
    /// ancestor) explicitly set this value, as opposed to the build
    /// pipeline auto-deriving it.
    pub fn is_explicit(&self) -> bool {
        matches!(self.origin, ResolvedOrigin::Frontmatter | ResolvedOrigin::Cascade)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_explicit_classifies_origins() {
        assert!(Resolved::frontmatter("x".to_string()).is_explicit());
        assert!(Resolved::cascade("x".to_string()).is_explicit());
        assert!(!Resolved::auto("x".to_string()).is_explicit());
    }

    #[test]
    fn roundtrips_through_serde() {
        let r = Resolved::frontmatter("year".to_string());
        let json = serde_json::to_string(&r).unwrap();
        let back: Resolved<String> = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }
}
