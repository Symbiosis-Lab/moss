//! What every page of one build reads identically, built once by the caller.
//!
//! Anything here used to be built by the page renderer itself, once per page,
//! so re-rendering N pages paid for N copies of the same answer. The media
//! index is the expensive one: it walks every image and video in the vault,
//! and on an image-heavy site rebuilding it per page was most of the cost of
//! re-rendering every page after a nav edit.

use std::collections::HashMap;

use crate::build::emit::scripts::ScriptAssets;
use crate::build::media::dimensions::MediaDimensionLookup;
use crate::types::content::ProjectStructure;

pub struct BuildShared {
    /// The build's one script snapshot, so every page's `<script src>` names
    /// a file that same snapshot emitted.
    pub scripts: ScriptAssets,
    /// Dimensions, colors and LQIP for the vault's media, read by the cards,
    /// covers and embeds a page body resolves. Registry-less: page bodies
    /// have never folded in the build's encoded variants.
    pub media_lookup: MediaDimensionLookup,
}

impl BuildShared {
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
        }
    }
}
