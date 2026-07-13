use patchwright_engine::codex::protocol::{
    ClientMethod, ClientRequest, CodexEvent, IncomingMessage, InitializedNotification,
    MAX_LINE_BYTES, ProtocolDecoder, ProtocolError, RequestId,
};
use serde_json::json;

#[test]
fn decodes_validated_responses_notifications_and_unsupported_events() {
    let mut decoder = ProtocolDecoder::default();
    decoder.register_request(RequestId::Number(1)).unwrap();
    let response = decoder
        .decode_line(br#"{"jsonrpc":"2.0","id":1,"result":{"userAgent":"codex_cli_rs/0.144.2"}}"#)
        .unwrap();
    assert!(matches!(response, IncomingMessage::Response(_)));

    let initialized = decoder
        .decode_line(br#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#)
        .unwrap();
    assert_eq!(initialized, IncomingMessage::Event(CodexEvent::Initialized));

    let unsupported = decoder
        .decode_line(br#"{"jsonrpc":"2.0","method":"future/event","params":{"token":"redacted"}}"#)
        .unwrap();
    assert!(matches!(
        unsupported,
        IncomingMessage::Event(CodexEvent::Unsupported { .. })
    ));
    assert!(!format!("{unsupported:?}").contains("redacted"));
}

#[test]
fn encodes_only_the_pinned_client_method_discriminators() {
    let request = ClientRequest::new(
        RequestId::String("request-1".to_owned()),
        ClientMethod::TurnInterrupt,
        json!({"threadId": "thread-1", "turnId": "turn-1"}),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(request).unwrap(),
        json!({
            "jsonrpc": "2.0",
            "id": "request-1",
            "method": "turn/interrupt",
            "params": {"threadId": "thread-1", "turnId": "turn-1"}
        })
    );
    assert_eq!(
        serde_json::to_value(InitializedNotification::default()).unwrap(),
        json!({"jsonrpc": "2.0", "method": "initialized"})
    );
    assert!(
        serde_json::from_value::<ClientRequest>(json!({
            "jsonrpc": "2.0", "id": 1, "method": "future/method", "params": {}
        }))
        .is_err()
    );
}

#[test]
fn rejects_bounds_malformed_ids_and_duplicate_completion() {
    let mut decoder = ProtocolDecoder::default();
    assert!(matches!(
        decoder.decode_line(&vec![b'x'; MAX_LINE_BYTES + 1]),
        Err(ProtocolError::LineTooLarge { .. })
    ));
    assert!(matches!(
        decoder.decode_line(b"{"),
        Err(ProtocolError::MalformedJson(_))
    ));
    assert!(matches!(
        decoder.decode_line(br#"{"jsonrpc":"2.0","id":9,"result":{}}"#),
        Err(ProtocolError::UnexpectedResponseId(_))
    ));
    assert!(matches!(
        decoder.decode_line(
            br#"{"jsonrpc":"2.0","id":10,"method":"item/commandExecution/requestApproval"}"#
        ),
        Err(ProtocolError::MissingField("params"))
    ));

    let completion = json!({
        "jsonrpc": "2.0",
        "method": "turn/completed",
        "params": {"threadId": "thread-1", "turn": {"id": "turn-1", "status": "completed"}}
    })
    .to_string();
    decoder.decode_line(completion.as_bytes()).unwrap();
    assert!(matches!(
        decoder.decode_line(completion.as_bytes()),
        Err(ProtocolError::DuplicateCompletion { .. })
    ));
}

#[test]
fn rejects_missing_required_identity_and_unknown_completion_status() {
    let mut decoder = ProtocolDecoder::default();
    assert!(matches!(
        decoder.decode_line(
            br#"{"jsonrpc":"2.0","method":"item/started","params":{"threadId":"thread-1"}}"#
        ),
        Err(ProtocolError::MissingField("turnId"))
    ));
    assert!(matches!(
        decoder.decode_line(br#"{"jsonrpc":"2.0","method":"turn/completed","params":{"threadId":"thread-1","turn":{"id":"turn-1","status":"future"}}}"#),
        Err(ProtocolError::UnknownTurnStatus(_))
    ));
}
