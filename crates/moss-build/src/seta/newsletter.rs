//! A site's newsletter provider, as seta keeps it: connecting a pasted key,
//! the connection's state, the sender, disconnecting. The key passes through
//! to seta once per call and is never stored or logged here.

use super::client::{MossSetaClient, SetaError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct ServiceList {
    pub id: String,
    pub name: String,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Sender {
    pub email: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
}

/// An address the service sends from, or a domain any address at which it sends from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(untagged)]
pub enum SenderSource {
    Domain { domain: String },
    Address(Sender),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct ReplacedKey {
    pub account: String,
    #[serde(rename = "keyId")]
    pub key_id: String,
}

/// Why a call did not go through. `Unreachable` is moss's server, not the service.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum NewsletterRefusal {
    Auth,
    Scopes,
    WebhooksFull,
    Sender,
    /// The kept key predates sending; Reconnect makes one that may send.
    CannotSend,
    Unknown,
    Unreachable,
}

/// A finished connect, as seta sends it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Connected {
    pub account: String,
    pub list: ServiceList,
    pub key_left: Option<String>,
    pub replaced_key_left: Option<ReplacedKey>,
    pub senders: Vec<SenderSource>,
    pub sender: Option<Sender>,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConnectAnswer {
    Lists { account: String, lists: Vec<ServiceList> },
    Connected(Connected),
    Refused { refused: NewsletterRefusal },
}

/// seta's answer as sent: one object, told apart by its keys.
#[derive(Deserialize)]
#[serde(untagged)]
enum SetaConnect {
    Connected(Connected),
    Lists { account: String, lists: Vec<ServiceList> },
}

impl From<SetaConnect> for ConnectAnswer {
    fn from(answer: SetaConnect) -> Self {
        match answer {
            SetaConnect::Connected(connected) => ConnectAnswer::Connected(connected),
            SetaConnect::Lists { account, lists } => ConnectAnswer::Lists { account, lists },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(untagged)]
pub enum ListPick {
    New { new: String },
    Id(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SenderAnswer {
    Saved { sender: Sender },
    Refused { refused: NewsletterRefusal },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    Connecting,
    Connected,
    /// A state this moss does not know yet; it asks for a key.
    #[serde(other)]
    Other,
}

/// Why a connected service is not taking subscribers; they wait in seta.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Pause {
    Key,
    List,
    Service,
    /// A cause this moss does not know yet; it says the service is not answering.
    #[serde(other)]
    Other,
}

/// What seta's own sending refused.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum FromMossAbout {
    Sender,
    /// A cause this moss does not know yet; it shows that moss is sending.
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct FromMoss {
    pub about: Option<FromMossAbout>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub state: ConnectionState,
    pub paused_for: Option<Pause>,
    pub account: Option<String>,
    pub list: Option<String>,
    pub can_send: bool,
    pub sender: Option<Sender>,
    pub senders: Vec<SenderSource>,
    pub from_moss: Option<FromMoss>,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct SetaNewsletter {
    pub connection: Option<Connection>,
    /// Subscriber changes not yet at the service.
    pub waiting: u32,
}

#[derive(Deserialize)]
struct SetaStatus {
    connection: Option<Connection>,
    counts: Counts,
}

#[derive(Deserialize)]
struct Counts {
    waiting: Option<u32>,
}

/// The refusal a failed call means, or the error to show as it is.
fn refusal(err: SetaError) -> Result<NewsletterRefusal, String> {
    if let SetaError::Api { status, message } = &err {
        let named = serde_json::from_str::<serde_json::Value>(message).ok()
            .and_then(|body| serde_json::from_value::<NewsletterRefusal>(body.get("refused")?.clone()).ok());
        if let Some(refused) = named {
            return Ok(refused);
        }
        if matches!(status, 502..=504) {
            return Ok(NewsletterRefusal::Unreachable);
        }
    }
    match err {
        SetaError::Http(_) | SetaError::Challenge { .. } => Ok(NewsletterRefusal::Unreachable),
        other => Err(other.to_string()),
    }
}

fn path(site_id: &str, route: &str) -> String {
    format!("/api/sites/{site_id}/newsletter{route}")
}

impl MossSetaClient {
    pub async fn newsletter_status(&self, site_id: &str) -> Result<SetaNewsletter, SetaError> {
        let s: SetaStatus = self.get_json(&path(site_id, "/status")).await?;
        Ok(SetaNewsletter { connection: s.connection, waiting: s.counts.waiting.unwrap_or(0) })
    }

    /// Without `list`, seta answers with the account's lists and connects nothing.
    pub async fn newsletter_connect(&self, site_id: &str, key: &str, list: Option<ListPick>) -> Result<ConnectAnswer, String> {
        let mut body = serde_json::json!({ "key": key });
        if let Some(list) = list {
            body["list"] = serde_json::json!(list);
        }
        match self.post_json::<_, SetaConnect>(&path(site_id, "/connect"), &body).await {
            Ok(answer) => Ok(answer.into()),
            Err(e) => Ok(ConnectAnswer::Refused { refused: refusal(e)? }),
        }
    }

    pub async fn newsletter_pick_sender(&self, site_id: &str, sender: Sender) -> Result<SenderAnswer, String> {
        #[derive(Deserialize)]
        struct Saved {
            sender: Sender,
        }
        match self.post_json::<_, Saved>(&path(site_id, "/sender"), &sender).await {
            Ok(saved) => Ok(SenderAnswer::Saved { sender: saved.sender }),
            Err(SetaError::Api { status: 409, .. }) => Ok(SenderAnswer::Refused { refused: NewsletterRefusal::CannotSend }),
            Err(e) => Ok(SenderAnswer::Refused { refused: refusal(e)? }),
        }
    }

    pub async fn newsletter_disconnect(&self, site_id: &str) -> Result<(), SetaError> {
        self.delete_json::<serde_json::Value>(&path(site_id, "/connection")).await.map(|_| ())
    }
}

#[cfg(test)]
#[path = "newsletter_tests.rs"]
mod tests;
