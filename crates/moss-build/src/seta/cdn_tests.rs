//! What the CDN endpoints ask for, and what the desktop makes of the answer.
//!
//! The two things worth pinning here are the ones a server change can break
//! silently: the request the client signs (a wrong path is a 404 the UI would
//! render as "not accelerated"), and the tolerance of the status word — seta
//! ships to production ahead of the desktop, so a status this build has never
//! heard of must not take the whole panel down.

use super::*;
use crate::seta::client::MossSetaClient;
use crate::identity::Identity;

use crate::seta::client::tests::StandIn;

fn client_for(server: &StandIn) -> MossSetaClient {
    MossSetaClient::with_identity_and_url(&Identity::generate().unwrap(), &server.url())
}

const ACTIVE_BODY: &str = r#"{"cdn_status":"active","records":[
    {"type":"CNAME","subdomain":"www","value":"sites.mosspub.com"},
    {"type":"CNAME","subdomain":"_acme-challenge.www","value":"www.example.com.dcv.cloudflare.com"},
    {"type":"A","subdomain":"@","value":"45.76.13.224"}
]}"#;

#[tokio::test]
async fn get_domain_cdn_signs_a_get_of_the_hosts_cdn_path() {
    let server = StandIn::json(200, ACTIVE_BODY).await;
    let cdn = client_for(&server)
        .get_domain_cdn("example.com")
        .await
        .expect("a 200 must parse");

    let req = &server.requests()[0];
    assert_eq!((req.method.as_str(), req.path_and_query.as_str()), ("GET", "/api/domains/example.com/cdn"));
    req.signer();
    assert_eq!(cdn.cdn_status, CdnStatus::Active);
}

#[tokio::test]
async fn the_servers_records_arrive_in_the_shape_the_records_table_renders() {
    // seta says `subdomain`; a DnsRecord says `name`. One shape reaches the
    // UI, so a purchased domain's records and a brought-in domain's records
    // render through the same table.
    let server = StandIn::json(200, ACTIVE_BODY).await;
    let cdn = client_for(&server).get_domain_cdn("example.com").await.unwrap();

    assert_eq!(cdn.records.len(), 3);
    assert_eq!(cdn.records[0].record_type, "CNAME");
    assert_eq!(cdn.records[0].name, "www");
    assert_eq!(cdn.records[0].value, "sites.mosspub.com");
    assert_eq!(cdn.records[2].name, "@");
}

#[tokio::test]
async fn a_status_word_this_build_has_never_heard_of_still_parses() {
    // seta deploys to production ahead of every desktop in the field. A new
    // status must cost the panel one unknown word, not the whole reading.
    let server = StandIn::json(
        200,
        r#"{"cdn_status":"pending_something_new","records":[{"type":"CNAME","subdomain":"www","value":"sites.mosspub.com"}]}"#,
    )
    .await;
    let cdn = client_for(&server).get_domain_cdn("example.com").await.unwrap();

    assert_eq!(cdn.cdn_status, CdnStatus::Unknown);
    assert!(!cdn.cdn_status.is_active(), "unknown is never active");
    assert_eq!(cdn.records.len(), 1, "the records still have to arrive");
}

#[tokio::test]
async fn start_domain_cdn_posts_to_the_same_path_and_returns_the_fresh_records() {
    // The POST answers in the GET's shape, and its records are the fresh
    // DCV values — recreating a hostname mints new ones.
    let server = StandIn::json(
        200,
        r#"{"cdn_status":"pending_dcv","records":[{"type":"CNAME","subdomain":"_acme-challenge.www","value":"fresh.dcv.cloudflare.com"}]}"#,
    )
    .await;
    let cdn = client_for(&server).start_domain_cdn("example.com").await.unwrap();

    let req = &server.requests()[0];
    assert_eq!((req.method.as_str(), req.path_and_query.as_str()), ("POST", "/api/domains/example.com/cdn"));
    assert_eq!(cdn.cdn_status, CdnStatus::PendingDcv);
    assert_eq!(cdn.records[0].value, "fresh.dcv.cloudflare.com");
}

#[tokio::test]
async fn a_refused_activation_is_an_error_not_an_empty_reading() {
    // 409 (moss-purchased domain), 403 (not yours) and 502 (Cloudflare) all
    // arrive this way. Returning a Disabled reading instead would tell the
    // user their domain simply is not accelerated yet — which is a lie.
    let server = StandIn::json(409, r#"{"error":"domain is managed by moss"}"#).await;
    let result = client_for(&server).start_domain_cdn("example.com").await;
    assert!(matches!(result, Err(SetaError::Api { status: 409, .. })), "got {:?}", result);
}

#[test]
fn every_status_the_server_can_send_has_a_word_here() {
    // The list is the server's, not ours: disabled | pending_dcv | validating
    // | active | failed.
    assert_eq!(CdnStatus::from_wire("disabled"), CdnStatus::Disabled);
    assert_eq!(CdnStatus::from_wire("pending_dcv"), CdnStatus::PendingDcv);
    assert_eq!(CdnStatus::from_wire("validating"), CdnStatus::Validating);
    assert_eq!(CdnStatus::from_wire("active"), CdnStatus::Active);
    assert_eq!(CdnStatus::from_wire("failed"), CdnStatus::Failed);
    assert!(CdnStatus::Active.is_active());
    assert!(!CdnStatus::Validating.is_active());
}
