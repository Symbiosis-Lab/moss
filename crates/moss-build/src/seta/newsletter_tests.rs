use super::*;
use crate::identity::Identity;

const BASE: &str = "/api/sites/site-1/newsletter";

/// Runs `call` against a seta that answers `method BASE+route` with `status` and `body`.
async fn against<T, F, Fut>(method: &str, route: &str, status: usize, body: &str, call: F) -> T
where
    F: FnOnce(MossSetaClient) -> Fut,
    Fut: std::future::Future<Output = T>,
{
    sent(method, route, mockito::Matcher::Any, status, body, call).await
}

/// As [`against`], answering only a request whose body is `request`.
async fn sent<T, F, Fut>(method: &str, route: &str, request: mockito::Matcher, status: usize, body: &str, call: F) -> T
where
    F: FnOnce(MossSetaClient) -> Fut,
    Fut: std::future::Future<Output = T>,
{
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock(method, format!("{BASE}{route}").as_str())
        .match_body(request)
        .with_status(status)
        .with_header("content-type", "application/json")
        .with_body(body)
        .create_async()
        .await;
    let client = MossSetaClient::with_identity_and_url(&Identity::generate().expect("generate test identity"), &server.url());
    let out = call(client).await;
    mock.assert_async().await;
    out
}

/// A first Connect: seta reads an absent list as "answer the lists", and refuses a null one.
async fn connect_answer(status: usize, body: &str) -> Result<ConnectAnswer, String> {
    let request = mockito::Matcher::Json(serde_json::json!({ "key": "SG.key" }));
    sent("POST", "/connect", request, status, body, |c| async move { c.newsletter_connect("site-1", "SG.key", None).await }).await
}

#[tokio::test]
async fn each_refusal_seta_names_is_its_own_answer() {
    for (status, body, refused) in [
        (422, r#"{"refused":"auth"}"#, NewsletterRefusal::Auth),
        (422, r#"{"refused":"scopes"}"#, NewsletterRefusal::Scopes),
        (422, r#"{"refused":"webhooks_full"}"#, NewsletterRefusal::WebhooksFull),
        (503, r#"{"refused":"unknown"}"#, NewsletterRefusal::Unknown),
    ] {
        assert_eq!(connect_answer(status, body).await, Ok(ConnectAnswer::Refused { refused }), "{body}");
    }
}

#[tokio::test]
async fn a_gateway_page_is_moss_s_server_not_answering() {
    let answer = connect_answer(502, "<html><title>502: Bad gateway</title></html>").await;
    assert_eq!(answer, Ok(ConnectAnswer::Refused { refused: NewsletterRefusal::Unreachable }));
}

#[tokio::test]
async fn a_request_seta_cannot_read_is_an_error_not_a_refusal() {
    let answer = connect_answer(400, r#"{"error":"Provide key, and list as an id or {new: name}"}"#).await;
    assert!(answer.unwrap_err().contains("Provide key"));
}

#[tokio::test]
async fn a_key_without_a_list_answers_the_account_s_lists() {
    let answer = connect_answer(200, r#"{"account":"news@example.com","lists":[{"id":"l1","name":"Readers","count":12}]}"#).await;
    assert_eq!(
        answer,
        Ok(ConnectAnswer::Lists {
            account: "news@example.com".into(),
            lists: vec![ServiceList { id: "l1".into(), name: "Readers".into(), count: 12 }],
        })
    );
}

#[tokio::test]
async fn a_picked_list_goes_as_its_id() {
    let request = mockito::Matcher::Json(serde_json::json!({ "key": "SG.key", "list": "l1" }));
    let answer = sent("POST", "/connect", request, 422, r#"{"refused":"webhooks_full"}"#, |c| async move {
        c.newsletter_connect("site-1", "SG.key", Some(ListPick::Id("l1".into()))).await
    })
    .await;
    assert_eq!(answer, Ok(ConnectAnswer::Refused { refused: NewsletterRefusal::WebhooksFull }));
}

#[tokio::test]
async fn a_finished_connect_carries_the_key_left_and_the_senders() {
    let body = r#"{"account":"news@example.com","list":{"id":"l1","name":"Readers","count":12},
        "keyLeft":"moss","replacedKeyLeft":{"account":"news@example.com","keyId":"k9"},
        "senders":[{"email":"a@example.com","name":"A"},{"domain":"example.com"}]}"#;
    let request = mockito::Matcher::Json(serde_json::json!({ "key": "SG.key", "list": { "new": "Readers" } }));
    let answer = sent("POST", "/connect", request, 200, body, |c| async move {
        c.newsletter_connect("site-1", "SG.key", Some(ListPick::New { new: "Readers".into() })).await
    })
    .await;
    let Ok(ConnectAnswer::Connected(Connected { key_left, replaced_key_left, senders, sender, .. })) = answer else {
        panic!("not connected: {answer:?}");
    };
    assert_eq!(key_left.as_deref(), Some("moss"));
    assert_eq!(replaced_key_left, Some(ReplacedKey { account: "news@example.com".into(), key_id: "k9".into() }));
    assert_eq!(
        senders,
        vec![
            SenderSource::Address(Sender { email: "a@example.com".into(), name: "A".into(), reply_to: None }),
            SenderSource::Domain { domain: "example.com".into() },
        ]
    );
    assert_eq!(sender, None);
}

#[tokio::test]
async fn a_sender_is_saved_or_refused() {
    let pick = || Sender { email: "a@example.com".into(), name: "A".into(), reply_to: None };
    let request = || mockito::Matcher::Json(serde_json::json!({ "email": "a@example.com", "name": "A" }));
    let saved = sent("POST", "/sender", request(), 200, r#"{"sender":{"email":"a@example.com","name":"A"}}"#, |c| async move {
        c.newsletter_pick_sender("site-1", pick()).await
    })
    .await;
    assert_eq!(saved, Ok(SenderAnswer::Saved { sender: pick() }));
    let refused = sent("POST", "/sender", request(), 422, r#"{"refused":"sender"}"#, |c| async move { c.newsletter_pick_sender("site-1", pick()).await }).await;
    assert_eq!(refused, Ok(SenderAnswer::Refused { refused: NewsletterRefusal::Sender }));
    let unsendable = against("POST", "/sender", 409, r#"{"error":"Not connected with a key that may send; Reconnect"}"#, |c| async move {
        c.newsletter_pick_sender("site-1", pick()).await
    })
    .await;
    assert_eq!(unsendable, Ok(SenderAnswer::Refused { refused: NewsletterRefusal::CannotSend }));
}

#[tokio::test]
async fn status_reads_the_connection_and_the_waiting_count() {
    // seta's answer as sent: `waiting` beside `refused` is a flag; the count is in `counts`.
    let body = r#"{"destination":null,"refused":null,"waiting":true,
        "connection":{"state":"connected","pausedFor":"service","fromMoss":{"since":1,"about":"sender"},"sender":null,"senders":[],
          "canSend":true,"account":"news@example.com","list":"Readers"},
        "counts":{"waiting":3,"failed":0},"credentialReady":true,"credentialSavedAt":null,"deliveries":[],"groupSuppressions":[]}"#;
    let seta = against("GET", "/status", 200, body, |c| async move { c.newsletter_status("site-1").await }).await.unwrap();
    assert_eq!(seta.waiting, 3);
    let connection = seta.connection.expect("connected");
    assert_eq!((connection.state, connection.paused_for, connection.can_send), (ConnectionState::Connected, Some(Pause::Service), true));
    assert_eq!(connection.from_moss, Some(FromMoss { about: Some(FromMossAbout::Sender) }));
}

#[tokio::test]
async fn a_status_value_this_moss_does_not_know_still_reads() {
    let body = r#"{"waiting":false,
        "connection":{"state":"moving","pausedFor":"quota","fromMoss":{"since":1,"about":"bounces"},"sender":null,"senders":[],
          "canSend":true,"account":"news@example.com","list":"Readers"},
        "counts":{"waiting":0,"failed":0}}"#;
    let seta = against("GET", "/status", 200, body, |c| async move { c.newsletter_status("site-1").await }).await.unwrap();
    let connection = seta.connection.expect("connected");
    assert_eq!((connection.state, connection.paused_for), (ConnectionState::Other, Some(Pause::Other)));
    assert_eq!(connection.from_moss, Some(FromMoss { about: Some(FromMossAbout::Other) }));
}
