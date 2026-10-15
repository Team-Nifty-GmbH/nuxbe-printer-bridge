use crate::services::mercure::{subscriber_token, take_events};

#[test]
fn take_events_keeps_an_incomplete_event_for_the_next_chunk() {
    let mut buffer =
        b":heartbeat\n\nid: 1\ndata: {\"event\":\"PrintJobCreated\"}\n\ndata: {\"ev".to_vec();

    assert_eq!(
        take_events(&mut buffer),
        vec!["{\"event\":\"PrintJobCreated\"}"]
    );
    assert_eq!(buffer, b"data: {\"ev");

    buffer.extend_from_slice(b"ent\":\"x\"}\n\n");

    assert_eq!(take_events(&mut buffer), vec!["{\"event\":\"x\"}"]);
    assert!(buffer.is_empty());
}

#[test]
fn take_events_joins_multi_line_data() {
    let mut buffer = b"data:a\ndata: b\n\n".to_vec();

    assert_eq!(take_events(&mut buffer), vec!["a\nb"]);
}

#[test]
fn subscriber_token_is_found_under_any_mercure_cookie_name() {
    for cookie in [
        "mercureAuthorization=abc; path=/.well-known/mercure; httponly",
        "__Secure-mercure_access_token=abc; Max-Age=300; path=/.well-known/mercure; secure",
    ] {
        assert_eq!(
            subscriber_token(["laravel_session=xyz; path=/", cookie]),
            Some("abc".to_owned())
        );
    }

    assert_eq!(subscriber_token(["laravel_session=xyz; path=/"]), None);
}
