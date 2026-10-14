use crate::services::mercure::take_events;

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
