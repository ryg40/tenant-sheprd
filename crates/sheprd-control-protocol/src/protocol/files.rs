//! The file explorer: the panel on the window's right that lists the
//! folders of the focused pane's checkout.
use super::app::Switch;
use serde::{Deserialize, Serialize};

/// How long the window may take to find the checkout before a file is
/// revealed in it.
pub const REVEAL_WAIT_SECONDS: u64 = 5;
/// The longest path a request may name.
pub const MAX_PATH: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum FilesOp {
    /// Shows, hides, or toggles the explorer; `None` only reports it.
    Panel {
        #[serde(default)]
        switch: Option<Switch>,
    },
    /// Shows the explorer with the file or folder at absolute `path`
    /// selected and the folders holding it open. The caller resolved the
    /// path on its side.
    Reveal { path: String },
}

impl FilesOp {
    pub fn wait_seconds(&self) -> u64 {
        match self {
            Self::Panel { .. } => 0,
            Self::Reveal { .. } => REVEAL_WAIT_SECONDS,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Reply {
    Panel {
        /// The user has it open.
        shown: bool,
        /// The folder it lists: the focused pane's checkout, once found.
        root: Option<String>,
        /// The focused pane runs on another host, which is not listed.
        remote: bool,
    },
    Revealed {
        root: String,
        /// The path selected, relative to `root`.
        path: String,
    },
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::protocol::{Call, Caller, Request, Response};

    #[test]
    fn requests_and_replies_round_trip() {
        for op in [
            FilesOp::Panel { switch: None },
            FilesOp::Panel {
                switch: Some(Switch::On),
            },
            FilesOp::Reveal {
                path: "/repo/src/main.rs".into(),
            },
        ] {
            let request = Request::Files(Call {
                caller: Caller::default(),
                op,
            });
            let text = serde_json::to_string(&request).unwrap();
            assert_eq!(serde_json::from_str::<Request>(&text).unwrap(), request);
        }
        let reply = Response::Files {
            reply: Reply::Revealed {
                root: "/repo".into(),
                path: "src/main.rs".into(),
            },
        };
        let text = serde_json::to_string(&reply).unwrap();
        assert_eq!(serde_json::from_str::<Response>(&text).unwrap(), reply);
    }

    #[test]
    fn an_unknown_field_is_refused() {
        let text =
            r#"{"method":"files","caller":{},"op":{"op":"reveal","path":"/a","follow":true}}"#;
        assert!(serde_json::from_str::<Request>(text).is_err());
    }
}
