use super::*;

fn api(status: u16) -> SetaError {
    SetaError::Api {
        status,
        message: "x".into(),
    }
}

/// The whole table in one place: a wrong answer here either abandons a
/// recoverable publish or re-sends a request the server will never accept.
#[test]
fn classify_groups_statuses_by_what_the_client_should_do() {
    // "Not now" — the request is fine, the peer wants time.
    for status in [429u16, 503] {
        assert_eq!(
            classify(&api(status)),
            FailureClass::Backpressure,
            "{status} is the server asking for time, not a fault"
        );
    }
    // Genuine origin faults: retry, but count them.
    for status in [500u16, 502, 504] {
        assert_eq!(
            classify(&api(status)),
            FailureClass::Transient,
            "{status} is an origin fault"
        );
    }
    // A 524 is retryable but NOT forgiven: the chunk loop answers it by sending
    // less (`upload_policy::escalate_down`), and forgiving it here would buy
    // three more full-size replays first — a large live site's failure, restaged.
    assert_eq!(
        classify(&api(524)),
        FailureClass::Transient,
        "524 must not buy extra full-size replays before escalation"
    );
    // The server's final answer, including a 413 quota rejection.
    for status in [400u16, 401, 403, 404, 409, 413] {
        assert_eq!(
            classify(&api(status)),
            FailureClass::Fatal,
            "{status} is final"
        );
    }
    for local in [
        SetaError::Parse("bad json".into()),
        SetaError::Io("open failed".into()),
        SetaError::Identity("no key".into()),
        SetaError::NotAllowlisted {
            apply_url: String::new(),
        },
    ] {
        assert_eq!(
            classify(&local),
            FailureClass::Fatal,
            "a local defect / invite gate is not a network condition: {local}"
        );
    }
}

/// A challenge is unsolvable by a native client on EVERY status — most
/// importantly 503 and 429, which the status table above calls Backpressure.
/// Backpressure is the *cheapest* class to be in (it is forgiven rather than
/// counted), so a challenge leaking into it would retry a permanent block
/// harder than before rather than less.
#[test]
fn a_challenge_is_fatal_even_on_a_backpressure_status() {
    for status in [403u16, 429, 503] {
        assert_eq!(
            classify(&SetaError::Challenge { status }),
            FailureClass::Fatal,
            "a {status} challenge must never be retried"
        );
    }
}

/// `is_transient` is the boolean facade the rest of the seta module still
/// calls. It must stay exactly "not Fatal", or the three-way table and the
/// two-way one drift apart and each caller gets a different answer.
#[test]
fn is_transient_is_exactly_not_fatal() {
    for status in [400u16, 403, 413, 429, 500, 502, 503, 504, 524] {
        let e = api(status);
        assert_eq!(
            e.is_transient(),
            classify(&e) != FailureClass::Fatal,
            "is_transient disagrees with classify on {status}"
        );
    }
}
