//! The one copy a channel's `publish_copy`, `update_copy` or `find_copy`
//! receives.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::record::RemoteRef;
use super::shape::Shaped;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    Publish,
    Update,
    /// Look up the copy of a send whose outcome is unknown.
    Find,
}

impl Op {
    pub const ALL: [Op; 3] = [Op::Publish, Op::Update, Op::Find];

    pub fn hook(self) -> &'static str {
        match self {
            Op::Publish => "publish_copy",
            Op::Update => "update_copy",
            Op::Find => "find_copy",
        }
    }
}

/// The original the copy points back to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostRef {
    /// Absolute.
    pub url: String,
    pub source_path: String,
    /// Empty for an untitled post.
    pub title: String,
    #[serde(default)]
    pub lang: Option<String>,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// The site a post lives on: `post.url` is built on `url`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiteRef {
    /// Absolute, no trailing slash.
    pub url: String,
    pub name: String,
    /// Absolute URL of the largest square raster of the site's own icon, at
    /// least 256 px; absent when the site has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

/// The smallest raster a channel is offered as the site's icon.
const ICON_MIN: u32 = 256;

/// The site's own icon as a channel can fetch it: the largest square raster of
/// at least [`ICON_MIN`] px the latest build served, and only when the folder
/// has an icon of its own (moss's mark is not the site's).
pub fn site_icon(folder: &std::path::Path, site_url: &str) -> Option<String> {
    crate::build::site_meta::favicon::own_icon_file(folder)?;
    let served = crate::moss_paths::MossPaths::new(folder).current_ptr();
    crate::build::site_meta::favicon::SIZES.iter().rev().filter(|&&size| size >= ICON_MIN).find_map(|&size| {
        let path = crate::build::served_path::ServedPath::for_favicon_raster(size);
        served.join(path.as_str()).is_file().then(|| format!("{}/{}", site_url.trim_end_matches('/'), path.as_str()))
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopyJob {
    pub op: Op,
    pub channel: String,
    /// The same on every send of this post to this channel.
    pub idempotency_key: String,
    pub site: SiteRef,
    pub post: PostRef,
    /// The copy as moss shaped it: `form`, `parts`, and `teaser` or `article`.
    /// Absent for a lookup whose payload no longer fits.
    #[serde(flatten)]
    pub copy: Option<Shaped>,
    /// The copy already on the network, for an update or a lookup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous: Option<Previous>,
    /// The channel's resolved settings, filled by the dispatcher.
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,
}

/// What `copy_account` receives: the channel, with its resolved settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountQuery {
    pub channel: String,
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Previous {
    #[serde(flatten)]
    pub copy: RemoteRef,
    #[serde(default)]
    pub parts: Vec<RemoteRef>,
}
