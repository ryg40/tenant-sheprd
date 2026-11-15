#![allow(clippy::unwrap_used)]
use super::*;
use crate::protocol::Request;

#[test]
fn requests_have_a_stable_wire_form() {
    for (text, op) in [
        (r#"{"op":"status"}"#, ArtifactsOp::Status {}),
        (r#"{"op":"start"}"#, ArtifactsOp::Start {}),
        (r#"{"op":"off"}"#, ArtifactsOp::Off {}),
        (r#"{"op":"open"}"#, ArtifactsOp::Open {}),
    ] {
        assert_eq!(serde_json::to_string(&op).unwrap(), text);
        assert_eq!(serde_json::from_str::<ArtifactsOp>(text).unwrap(), op);
    }
    let request: Request =
        serde_json::from_str(r#"{"method":"artifacts","caller":{},"op":{"op":"status"}}"#).unwrap();
    assert!(matches!(request, Request::Artifacts(_)));
    assert_eq!(request.wait_seconds(), WAIT_SECONDS);
}

#[test]
fn unknown_operations_states_and_fields_are_rejected() {
    for invalid in [
        r#"{"op":"restart"}"#,
        r#"{"op":"start","yes":true}"#,
        r#"{"op":"off","force":true}"#,
    ] {
        assert!(
            serde_json::from_str::<ArtifactsOp>(invalid).is_err(),
            "{invalid}"
        );
    }
    assert!(serde_json::from_str::<State>(r#""starting""#).is_err());
    assert!(serde_json::from_str::<Status>(r#"{"state":"running","url":null,"pid":1}"#).is_err());
}

#[test]
fn replies_round_trip() {
    let reply = Reply::Status(Status {
        state: State::Stopped,
        url: Some("http://127.0.0.1:6790/browse".into()),
        opens: None,
    });
    let text = serde_json::to_string(&reply).unwrap();
    assert_eq!(
        text,
        r#"{"kind":"status","state":"stopped","url":"http://127.0.0.1:6790/browse"}"#
    );
    assert_eq!(serde_json::from_str::<Reply>(&text).unwrap(), reply);
    let reply = Reply::Status(Status {
        state: State::Stopped,
        url: Some("http://127.0.0.1:6790/browse".into()),
        opens: Some("https://artifacts.example.com/".into()),
    });
    let text = serde_json::to_string(&reply).unwrap();
    assert_eq!(
        text,
        r#"{"kind":"status","state":"stopped","url":"http://127.0.0.1:6790/browse","opens":"https://artifacts.example.com/"}"#
    );
    assert_eq!(serde_json::from_str::<Reply>(&text).unwrap(), reply);
}
