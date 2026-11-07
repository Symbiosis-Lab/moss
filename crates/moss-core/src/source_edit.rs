//! Pure source-folder editing policy. Hosts supply bytes and directory facts,
//! then perform the reads, coordinated writes, history and trash operations.

pub mod tree;
pub mod save;
pub mod policy;
pub mod conversion;
mod content;
mod types;

pub use tree::{build_tree, build_tree_root, ListingEntry, TreeOptions, TreeNode, DirEntry, EditorFileInfo};
pub use save::{decide_save, SaveDecision};
pub use types::{ParsedFrontmatter, SaveResult, SourceRole};
pub use policy::{free_name, SpecialFileKind};
pub use content::{parse_frontmatter_of, bootstrap_source_path, compute_heading_state_inner,
    describe_source_role_inner, classify_source_read, SourceRead};

#[cfg(test)]
mod content_tests;
#[cfg(test)]
mod tree_tests;
#[cfg(test)]
mod save_tests;

#[cfg(test)]
mod policy_tests;
