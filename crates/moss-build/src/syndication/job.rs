//! The one copy a channel's `publish_copy`, `update_copy` or `delete_copy`
//! receives.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::record::{CopyRecord, RemoteRef};
use super::shape::{idempotency_key, page_url, Shaped};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    Publish,
    Update,
    Delete,
    /// Look up the copy of a send whose outcome is unknown.
    Find,
}

impl Op {
    pub const ALL: [Op; 4] = [Op::Publish, Op::Update, Op::Delete, Op::Find];

    pub fn hook(self) -> &'static str {
        match self {
            Op::Publish => "publish_copy",
            Op::Update => "update_copy",
            Op::Delete => "delete_copy",
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopyJob {
    pub op: Op,
    pub channel: String,
    /// The same on every send of this post to this channel.
    pub idempotency_key: String,
    pub post: PostRef,
    /// The copy as moss shaped it: `form`, `parts`, and `teaser` or `article`.
    /// Absent for a delete, whose page is gone.
    #[serde(flatten)]
    pub copy: Option<Shaped>,
    /// The copy already on the network, for an update or delete.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous: Option<Previous>,
    /// The channel's resolved settings, filled by the dispatcher.
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,
}

impl CopyJob {
    /// Remove `copy`, the copy `record` holds, whose page is gone.
    pub fn delete(record: &CopyRecord, copy: RemoteRef, site_url: &str) -> Self {
        CopyJob {
            op: Op::Delete,
            channel: record.channel.clone(),
            idempotency_key: idempotency_key(&record.uid, &record.channel),
            post: PostRef {
                url: page_url(site_url, &record.url_path),
                source_path: record.source_path.clone(),
                title: String::new(),
                lang: None,
                date: None,
                tags: Vec::new(),
            },
            copy: None,
            previous: Some(Previous { copy, parts: record.parts.clone() }),
            config: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Previous {
    #[serde(flatten)]
    pub copy: RemoteRef,
    #[serde(default)]
    pub parts: Vec<RemoteRef>,
}
