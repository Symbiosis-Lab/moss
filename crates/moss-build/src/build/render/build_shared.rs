//! What every page of one build reads identically, built once by the caller.
//!
//! Anything here used to be built by the page renderer itself, once per page,
//! so re-rendering N pages paid for N copies of the same answer. The media
//! index is the expensive one: it walks every image and video in the vault,
//! and on an image-heavy site rebuilding it per page was most of the cost of
//! re-rendering every page after a nav edit.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use crate::build::emit::scripts::ScriptAssets;
use crate::build::media::dimensions::MediaDimensionLookup;
use crate::build::types::ParsedDocument;
use crate::types::content::ProjectStructure;

/// `'d` is the build's documents, which the cached reading orders point into.
pub struct BuildShared<'d> {
    /// The build's one script snapshot, so every page's `<script src>` names
    /// a file that same snapshot emitted.
    pub scripts: ScriptAssets,
    /// Dimensions, colors and LQIP for the vault's media, read by the cards,
    /// covers and embeds a page body resolves. Registry-less: page bodies
    /// have never folded in the build's encoded variants.
    pub media_lookup: MediaDimensionLookup,
    /// Each folder's prev/next reading order.
    pub sequences: SequenceChains<'d>,
}

impl BuildShared<'_> {
    /// Takes the build's script snapshot and indexes `project`'s images and
    /// videos under the build's slug overrides.
    pub fn new(
        scripts: ScriptAssets,
        project: &ProjectStructure,
        dir_overrides: &HashMap<String, String>,
    ) -> Self {
        Self {
            scripts,
            media_lookup: MediaDimensionLookup::new(
                &project.image_files,
                &project.video_files,
                dir_overrides,
                None,
            ),
            sequences: SequenceChains::default(),
        }
    }
}

/// Each folder's ordered prev/next chain, computed by the first of its pages
/// to ask and read by the rest. Ordering a folder sorts all of its children,
/// so doing it for every page made a folder of n pages cost n² log n.
#[derive(Default)]
pub struct SequenceChains<'d> {
    by_parent: Mutex<HashMap<String, Arc<OnceLock<Arc<[&'d ParsedDocument]>>>>>,
}

impl<'d> SequenceChains<'d> {
    /// The chain of the folder whose index is `parent_index`, ordered by
    /// `order` if no page has asked for it yet.
    pub fn chain(
        &self,
        parent_index: &str,
        order: impl FnOnce() -> Vec<&'d ParsedDocument>,
    ) -> Arc<[&'d ParsedDocument]> {
        // Take the folder's cell under the lock and order outside it, so pages
        // of other folders never wait on this one's sort.
        let cell = Arc::clone(
            self.by_parent
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .entry(parent_index.to_string())
                .or_default(),
        );
        Arc::clone(cell.get_or_init(|| order().into()))
    }
}
