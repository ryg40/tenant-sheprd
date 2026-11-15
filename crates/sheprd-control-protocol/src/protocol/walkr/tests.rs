use super::*;
use crate::protocol::{Call, Caller, Request};

const ADDRESS: &str =
    "http://127.0.0.1:4100/_walkr/compare/g/INVENTEDSECRET#20261101T120000Z-0123456789ab";
const SECRET_PARTS: [&str; 5] = [
    "INVENTEDSECRET",
    "_walkr",
    "compare",
    "20261101",
    "0123456789ab",
];

#[test]
fn walkr_connectivity_round_trips_with_a_state_tag() -> serde_json::Result<()> {
    for connectivity in [
        Connectivity::NotConfigured,
        Connectivity::Checking,
        Connectivity::Unreachable {
            cause: Unreachable::Dns,
        },
        Connectivity::Unreachable {
            cause: Unreachable::Connect,
        },
        Connectivity::Unreachable {
            cause: Unreachable::Timeout,
        },
        Connectivity::Unreachable {
            cause: Unreachable::Tls,
        },
        Connectivity::NetworkRefused,
        Connectivity::Down { status: 502 },
        Connectivity::Unexpected { status: 404 },
        Connectivity::Reachable,
        Connectivity::Ready,
    ] {
        let value = serde_json::to_value(connectivity)?;
        assert!(value.get("state").is_some_and(serde_json::Value::is_string));
        assert_eq!(serde_json::from_value::<Connectivity>(value)?, connectivity);
    }
    Ok(())
}

#[test]
fn a_removed_or_unknown_state_does_not_decode() {
    for state in ["sign_in_needed", "elsewhere"] {
        let value = serde_json::json!({ "state": state });
        assert!(serde_json::from_value::<Connectivity>(value).is_err());
    }
}

#[test]
fn walkr_requests_refuse_unknown_fields() -> serde_json::Result<()> {
    let valid = r#"{"method":"walkr","caller":{},"op":{"op":"status"}}"#;
    let request: Request = serde_json::from_str(valid)?;
    assert!(matches!(request, Request::Walkr(_)));
    for invalid in [
        r#"{"method":"walkr","caller":{},"op":{"op":"status","extra":1}}"#,
        r#"{"method":"walkr","caller":{},"op":{"op":"status"},"extra":1}"#,
    ] {
        assert!(serde_json::from_str::<Request>(invalid).is_err());
    }
    Ok(())
}

#[test]
fn debug_of_a_run_address_and_its_request_holds_host_and_port_only() {
    let address = RunAddress::new(ADDRESS);
    let request = Request::Walkr(Call {
        caller: Caller::default(),
        op: WalkrOp::Connect {
            address: address.clone(),
            show: true,
        },
    });
    for shown in [
        format!("{address:?}"),
        format!("{request:?}"),
        format!("{request:#?}"),
    ] {
        assert!(shown.contains("127.0.0.1:4100"), "{shown}");
        for hidden in SECRET_PARTS {
            assert!(!shown.contains(hidden), "{shown}");
        }
    }
    // Text of another shape prints nothing of itself.
    for odd in [
        "INVENTEDSECRET",
        "http://user:INVENTEDSECRET@127.0.0.1:4100/x",
        "http://127.0.0.1:4100?INVENTEDSECRET",
        "http://127.0.0.1:4100\\INVENTEDSECRET",
        "http://INVENTED SECRET/",
        "http:/INVENTEDSECRET",
    ] {
        let shown = format!("{:?}", RunAddress::new(odd));
        assert!(!shown.contains("INVENTED"), "{shown}");
    }
}

#[test]
fn connect_and_disconnect_round_trip_and_refuse_unknown_fields() -> serde_json::Result<()> {
    let connect = serde_json::json!({ "op": "connect", "address": ADDRESS, "show": true });
    let op: WalkrOp = serde_json::from_value(connect.clone())?;
    assert_eq!(
        op,
        WalkrOp::Connect {
            address: RunAddress::new(ADDRESS),
            show: true,
        }
    );
    // The page needs the whole address, so the wire carries it.
    assert_eq!(serde_json::to_value(&op)?, connect);
    let disconnect = serde_json::json!({ "op": "disconnect" });
    let op: WalkrOp = serde_json::from_value(disconnect.clone())?;
    assert_eq!(op, WalkrOp::Disconnect {});
    assert_eq!(serde_json::to_value(&op)?, disconnect);
    let clear = serde_json::json!({ "op": "clear" });
    let op: WalkrOp = serde_json::from_value(clear.clone())?;
    assert_eq!(op, WalkrOp::Clear {});
    assert_eq!(serde_json::to_value(&op)?, clear);
    let reply = serde_json::json!({ "kind": "cleared", "count": 3 });
    assert_eq!(
        serde_json::from_value::<Reply>(reply.clone())?,
        Reply::Cleared { count: 3 }
    );
    assert_eq!(serde_json::to_value(Reply::Cleared { count: 3 })?, reply);
    for invalid in [
        serde_json::json!({ "op": "clear", "host": "local" }),
        serde_json::json!({ "op": "disconnect", "close": true }),
        serde_json::json!({ "op": "connect", "address": ADDRESS }),
        serde_json::json!({ "op": "connect", "address": ADDRESS, "show": true, "x": 1 }),
        // No operation closes the page.
        serde_json::json!({ "op": "close" }),
    ] {
        assert!(serde_json::from_value::<WalkrOp>(invalid).is_err());
    }
    Ok(())
}

#[test]
fn status_names_its_source_in_snake_case_and_whether_the_run_is_live() -> serde_json::Result<()> {
    for (source, name) in [
        (Source::None, "none"),
        (Source::Run, "run"),
        (Source::Override, "override"),
    ] {
        let status = Status {
            connectivity: Connectivity::Ready,
            address: "127.0.0.1:4100".into(),
            checked_seconds_ago: Some(1),
            embedded: true,
            source,
            live: false,
            sessions: Vec::new(),
        };
        let value = serde_json::to_value(&status)?;
        assert_eq!(value["source"], name);
        assert_eq!(value["live"], false);
        assert_eq!(serde_json::from_value::<Status>(value)?, status);
    }
    let reply = Reply::Connected {
        address: "127.0.0.1:4100".into(),
        replaced: true,
    };
    let value = serde_json::to_value(&reply)?;
    assert_eq!(value["kind"], "connected");
    assert_eq!(serde_json::from_value::<Reply>(value)?, reply);
    Ok(())
}
