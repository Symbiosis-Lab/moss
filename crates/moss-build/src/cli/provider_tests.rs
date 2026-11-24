use super::*;
use crate::identity::Identity;

const KEY: &str = "mk_123456789";

/// A folder whose `[channels.email]` holds a reply-to and `line`, with a
/// newsletter provider plugin named `mailer` installed.
fn folder(line: &str) -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join(".moss")).unwrap();
    std::fs::write(tmp.path().join(".moss/config.toml"), format!("[channels.email]\nreply_to = \"me@example.com\"\n{line}")).unwrap();
    let plugin = crate::plugins::plugin_dir(tmp.path().to_str().unwrap(), "mailer");
    std::fs::create_dir_all(&plugin).unwrap();
    let manifest = serde_json::json!({"name":"mailer","version":"1.0.0","contributes":{"provider":{"part":"newsletter","addresses":[],
        "credentials":[{"kind":"key","env":"MAILER_KEY","url":"https://mailer.example/keys","prefix":"mk_","length":12,"permission":"Admin"}],
        "newsletter":{"steps":["Paste it here"]}}}});
    std::fs::write(plugin.join("manifest.json"), manifest.to_string()).unwrap();
    tmp
}

fn path(tmp: &tempfile::TempDir) -> String {
    tmp.path().to_string_lossy().into_owned()
}

fn config(tmp: &tempfile::TempDir) -> String {
    std::fs::read_to_string(tmp.path().join(".moss/config.toml")).unwrap()
}

/// A seta answering `method route` once with `status` and `body`, and the
/// mock to assert it was or was not reached.
async fn seta(method: &str, route: &str, status: usize, body: &str) -> (mockito::ServerGuard, mockito::Mock, MossSetaClient) {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock(method, format!("/api/sites/site-1/newsletter{route}").as_str())
        .with_status(status)
        .with_header("content-type", "application/json")
        .with_body(body)
        .create_async()
        .await;
    let client = MossSetaClient::with_identity_and_url(&Identity::generate().unwrap(), &server.url());
    (server, mock, client)
}

const CONNECTED: &str = r#"{"account":"news@example.com","list":{"id":"l1","name":"Readers","count":2},"keyLeft":null,"replacedKeyLeft":null,"senders":[],"sender":null}"#;

fn connect(provider: Option<&str>, list: Option<ListPick>) -> Connect {
    Connect { provider: provider.map(str::to_string), list }
}

#[tokio::test]
async fn the_provider_is_written_only_after_the_service_takes_the_key() {
    let tmp = folder("");
    let (_s, refused_mock, client) = seta("POST", "/connect", 422, r#"{"refused":"auth"}"#).await;
    let refused = connect_flow(&client, "site-1", &path(&tmp), connect(Some("mailer"), Some(ListPick::Id("l1".into()))), |_| Ok(KEY.into()), true).await;
    refused_mock.assert_async().await;
    assert_eq!(refused.code, 3, "{}", refused.out);
    assert_eq!(refused.out.trim(), r#"{"kind":"refused","refused":"auth"}"#);
    assert!(!config(&tmp).contains("provider"), "{}", config(&tmp));

    let (_s, ok_mock, client) = seta("POST", "/connect", 200, CONNECTED).await;
    let done = connect_flow(&client, "site-1", &path(&tmp), connect(Some("mailer"), Some(ListPick::Id("l1".into()))), |_| Ok(KEY.into()), true).await;
    ok_mock.assert_async().await;
    assert_eq!(done.code, 0, "{}", done.out);
    let written = config(&tmp);
    assert!(written.contains("provider = \"mailer\"") && written.contains("reply_to"), "{written}");
}

#[tokio::test]
async fn a_key_of_the_wrong_shape_is_refused_before_any_request() {
    let tmp = folder("provider = \"mailer\"\n");
    let (_s, mock, client) = seta("POST", "/connect", 200, CONNECTED).await;
    let out = connect_flow(&client, "site-1", &path(&tmp), connect(None, None), |_| Ok("SG.notmailer".into()), false).await;
    assert_eq!(out.code, 1);
    assert!(out.err.contains("not a mailer key"), "{}", out.err);
    mock.expect(0).assert_async().await;
}

#[tokio::test]
async fn the_key_reader_is_asked_with_the_part_s_credential() {
    let tmp = folder("provider = \"mailer\"\n");
    let (_s, _mock, client) = seta("POST", "/connect", 200, CONNECTED).await;
    let mut asked = None;
    let out = connect_flow(&client, "site-1", &path(&tmp), connect(None, Some(ListPick::Id("l1".into()))), |c| {
        asked = c.env.clone();
        Err("no key".into())
    }, false).await;
    assert_eq!((out.code, asked.as_deref()), (1, Some("MAILER_KEY")));
}

#[tokio::test]
async fn a_second_provider_is_refused_naming_both() {
    let tmp = folder("provider = \"mailer\"\n");
    let (_s, mock, client) = seta("POST", "/connect", 200, CONNECTED).await;
    let out = connect_flow(&client, "site-1", &path(&tmp), connect(Some("other"), None), |_| Ok(KEY.into()), false).await;
    assert_eq!(out.code, 1);
    assert!(out.err.contains("mailer") && out.err.contains("other"), "{}", out.err);
    mock.expect(0).assert_async().await;
}

#[tokio::test]
async fn without_a_list_the_lists_are_printed_and_nothing_is_written() {
    let tmp = folder("");
    let (_s, _mock, client) = seta("POST", "/connect", 200, r#"{"account":"news@example.com","lists":[{"id":"l1","name":"Readers","count":2}]}"#).await;
    let out = connect_flow(&client, "site-1", &path(&tmp), connect(Some("mailer"), None), |_| Ok(KEY.into()), true).await;
    assert_eq!(out.code, 2, "{}", out.out);
    let v: serde_json::Value = serde_json::from_str(&out.out).unwrap();
    assert_eq!((v["kind"].as_str(), v["lists"][0]["id"].as_str()), (Some("lists"), Some("l1")));
    assert!(!config(&tmp).contains("provider"));
}

#[tokio::test]
async fn status_is_one_object_keyed_by_part() {
    let tmp = folder("provider = \"mailer\"\n");
    let body = r#"{"connection":{"state":"connected","pausedFor":null,"fromMoss":null,"sender":null,"senders":[],
        "canSend":true,"account":"news@example.com","list":"Readers"},"counts":{"waiting":0}}"#;
    let (_s, _mock, client) = seta("GET", "/status", 200, body).await;
    let out = status_flow(&client, "site-1", &path(&tmp), true).await;
    assert_eq!(out.code, 0, "{}", out.out);
    let v: serde_json::Value = serde_json::from_str(&out.out).unwrap();
    let n = &v["newsletter"];
    assert_eq!((n["provider"].as_str(), n["installed"].as_bool(), n["connected"].as_bool()), (Some("mailer"), Some(true), Some(true)));
}

#[tokio::test]
async fn status_of_a_provider_not_connected_exits_3() {
    let tmp = folder("provider = \"mailer\"\n");
    let (_s, _mock, client) = seta("GET", "/status", 200, r#"{"connection":null,"counts":{"waiting":4}}"#).await;
    let out = status_flow(&client, "site-1", &path(&tmp), false).await;
    assert_eq!(out.code, 3);
    assert!(out.out.contains("mailer (not connected)"), "{}", out.out);
}

#[tokio::test]
async fn disconnect_removes_the_line_only_after_the_service_forgets() {
    let tmp = folder("provider = \"mailer\"\n");
    let (_s, _mock, client) = seta("DELETE", "/connection", 502, "<html></html>").await;
    assert_eq!(disconnect_flow(&client, "site-1", &path(&tmp), false).await.code, 3);
    assert!(config(&tmp).contains("provider = \"mailer\""));
    let (_s, _mock, client) = seta("DELETE", "/connection", 200, r#"{"removed":true}"#).await;
    assert_eq!(disconnect_flow(&client, "site-1", &path(&tmp), false).await.code, 0);
    let left = config(&tmp);
    assert!(!left.contains("provider") && left.contains("reply_to"), "{left}");
}

#[test]
fn a_part_that_is_not_one_names_the_parts() {
    assert!(part("comments").unwrap_err().contains("newsletter"));
    assert!(parse_connect(&["--list".into(), "a".into(), "--new-list".into(), "b".into()]).is_err());
}

#[tokio::test]
async fn a_refusal_in_text_says_what_to_do() {
    let tmp = folder("provider = \"mailer\"\n");
    let (_s, _mock, client) = seta("POST", "/connect", 422, r#"{"refused":"auth"}"#).await;
    let out = connect_flow(&client, "site-1", &path(&tmp), connect(None, Some(ListPick::Id("l1".into()))), |_| Ok(KEY.into()), false).await;
    assert_eq!(out.out.trim(), "mailer did not accept the key: make a new one at https://mailer.example/keys");
}
