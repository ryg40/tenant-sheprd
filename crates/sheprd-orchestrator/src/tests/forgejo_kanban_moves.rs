//! Fake wire peers exercise only scoped move safety, never a real tracker.
use super::*;
use crate::{
    forge::Scheme,
    forgejo::moves::{Session, api_root},
    kanban::{
        self, BoardColumn,
        identity::{CardKey, Origin, Route},
        moves::{ConfirmedMove, Context, MoveGate, MovePlan, Outcome, PermissionStep, Proposal},
    },
};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};
use ureq::unversioned::{
    resolver::{ResolvedSocketAddrs, Resolver},
    transport::{Buffers, ConnectionDetails, Connector, LazyBuffers, NextTimeout, Transport},
};

pub(crate) const HOST: &str = "forgejo.example.test";
const TOKEN: &str = "dummy-move-token";

#[derive(Debug)]
pub(crate) struct Peer {
    pub(crate) account_repositories: Vec<String>,
    pub(crate) exclusive: bool,
    pub(crate) missing: bool,
    pub(crate) ambiguous: bool,
    pub(crate) read_only: bool,
    pub(crate) push: bool,
    pub(crate) denied_read: Option<(String, u16)>,
    pub(crate) deny: bool,
    pub(crate) redirect: bool,
    pub(crate) fail_state: bool,
    pub(crate) lose_state: bool,
    pub(crate) issue_route: Option<String>,
    pub(crate) repository_id: Option<u64>,
    pub(crate) issue_id: Option<u64>,
    pub(crate) issue_repository: Option<Value>,
    pub(crate) labels: Vec<Value>,
    pub(crate) state: String,
    pub(crate) mutations: Vec<(String, String, Value)>,
    pub(crate) destinations: Vec<String>,
}

impl Default for Peer {
    fn default() -> Self {
        Self {
            account_repositories: vec!["team/app".into()],
            exclusive: false,
            missing: false,
            ambiguous: false,
            read_only: false,
            push: true,
            denied_read: None,
            deny: false,
            redirect: false,
            fail_state: false,
            lose_state: false,
            issue_route: None,
            repository_id: Some(11),
            issue_id: Some(71),
            issue_repository: Some(json!({"id": 11, "full_name": "team/app"})),
            labels: vec![
                label(20, "status: in-review", false),
                label(90, "unrelated", false),
            ],
            state: "closed".into(),
            mutations: Vec::new(),
            destinations: Vec::new(),
        }
    }
}

fn label(id: u64, name: &str, exclusive: bool) -> Value {
    json!({"id": id, "name": name, "exclusive": exclusive})
}

fn repository(name: &str, writable: bool, id: Option<u64>) -> Value {
    let mut value = json!({"full_name": name, "owner": {"login": "team"},
        "archived": false, "has_issues": true,
        "permissions": {"admin": false, "push": writable, "pull": true}});
    if let Some(id) = id {
        value["id"] = json!(id);
    }
    value
}

impl Peer {
    fn answer(&mut self, uri: &str, request: &[u8]) -> Vec<u8> {
        self.destinations.push(uri.to_owned());
        assert!(
            uri.starts_with(&format!("https://{HOST}/")),
            "credential reached another authority"
        );
        let request = String::from_utf8_lossy(request);
        assert!(
            request
                .to_ascii_lowercase()
                .contains(&format!("authorization: token {TOKEN}"))
        );
        let (headers, bytes) = request.split_once("\r\n\r\n").unwrap();
        let method = headers.split_whitespace().next().unwrap();
        let url = url::Url::parse(uri).unwrap();
        let path = url.path();
        let empty = json!(null);
        let mut status = 200;
        let value = if self.redirect {
            status = 302;
            empty
        } else if self.read_only && path.ends_with("/user") {
            status = 403;
            empty
        } else if let Some((_, denied)) = self
            .denied_read
            .as_ref()
            .filter(|(route, _)| method == "GET" && path.ends_with(route))
        {
            status = *denied;
            json!({"message": TOKEN})
        } else if method != "GET" {
            let body: Value = if bytes.is_empty() {
                empty
            } else {
                serde_json::from_str(bytes).unwrap()
            };
            self.mutations
                .push((method.into(), path.into(), body.clone()));
            if self.deny {
                status = 403;
            } else if method == "DELETE" {
                let id = path.rsplit('/').next().unwrap().parse::<u64>().unwrap();
                self.labels.retain(|label| label["id"].as_u64() != Some(id));
            } else if method == "POST" {
                for id in body["labels"].as_array().unwrap() {
                    assert_eq!(id, &json!(10));
                    assert!(
                        !self.labels.iter().any(|held| held["id"] == *id),
                        "delta replayed"
                    );
                    self.labels.push(label(10, "status: in-progress", false));
                }
            } else if self.lose_state {
                return Vec::new();
            } else if self.fail_state {
                status = 403;
            } else {
                self.state = body["state"].as_str().unwrap().into();
            }
            json!({})
        } else if path.ends_with("/user") {
            json!({"id": 7, "login": "dummy-account"})
        } else if path.ends_with("/user/repos") {
            if url.query_pairs().any(|(k, v)| k == "page" && v != "1") {
                json!([])
            } else {
                json!(
                    self.account_repositories
                        .iter()
                        .map(|name| repository(name, true, self.repository_id))
                        .collect::<Vec<_>>()
                )
            }
        } else if path.ends_with("/orgs/team/labels") {
            if url.query_pairs().any(|(k, v)| k == "page" && v != "1") || self.missing {
                json!([])
            } else {
                json!([label(10, "status: in-progress", self.exclusive)])
            }
        } else if path.ends_with("/issues/7/labels") {
            json!(self.labels)
        } else if path.ends_with("/issues/7") {
            let mut value = json!({"number": 7, "url": self.issue_route.as_deref().unwrap_or(uri),
                "title": "Issue", "state": self.state, "labels": self.labels,
                "repository": self.issue_repository,
                "html_url": format!("https://{HOST}/team/app/issues/7"),
                "created_at": "2026-11-01T00:00:00Z", "updated_at": "2026-11-02T00:00:00Z"});
            if let Some(id) = self.issue_id {
                value["id"] = json!(id);
            }
            value
        } else if path.ends_with("/labels") {
            if url.query_pairs().any(|(k, v)| k == "page" && v != "1") {
                json!([])
            } else if self.ambiguous {
                json!([
                    label(20, "status: in-review", false),
                    label(33, "status: in-progress", false)
                ])
            } else {
                json!([label(20, "status: in-review", false)])
            }
        } else if path.ends_with("/repos/team/app") {
            repository("team/app", self.push, self.repository_id)
        } else {
            panic!("unexpected destination: {path}");
        };
        let body = value.to_string();
        format!("HTTP/1.1 {status} Reply\r\nContent-Length: {}\r\nConnection: close\r\nLocation: https://github.example.test/stolen\r\n\r\n{body}", body.len()).into_bytes()
    }
}

#[derive(Clone, Debug)]
struct Wire(Arc<Mutex<Peer>>);

impl Resolver for Wire {
    fn resolve(
        &self,
        _: &ureq::http::Uri,
        _: &ureq::config::Config,
        _: NextTimeout,
    ) -> std::result::Result<ResolvedSocketAddrs, ureq::Error> {
        let mut addresses = self.empty();
        addresses.push("127.0.0.1:1".parse().unwrap());
        Ok(addresses)
    }
}

impl Connector<()> for Wire {
    type Out = Connection;
    fn connect(
        &self,
        details: &ConnectionDetails,
        _: Option<()>,
    ) -> std::result::Result<Option<Self::Out>, ureq::Error> {
        Ok(Some(Connection {
            peer: self.0.clone(),
            uri: details.uri.to_string(),
            request: Vec::new(),
            reply: None,
            buffers: LazyBuffers::new(8192, 8192),
        }))
    }
}

#[derive(Debug)]
struct Connection {
    peer: Arc<Mutex<Peer>>,
    uri: String,
    request: Vec<u8>,
    reply: Option<Vec<u8>>,
    buffers: LazyBuffers,
}

impl Transport for Connection {
    fn buffers(&mut self) -> &mut dyn Buffers {
        &mut self.buffers
    }
    fn transmit_output(
        &mut self,
        amount: usize,
        _: NextTimeout,
    ) -> std::result::Result<(), ureq::Error> {
        self.request
            .extend_from_slice(&self.buffers.output()[..amount]);
        Ok(())
    }
    fn await_input(&mut self, _: NextTimeout) -> std::result::Result<bool, ureq::Error> {
        let mut reply = self
            .reply
            .take()
            .unwrap_or_else(|| self.peer.lock().unwrap().answer(&self.uri, &self.request));
        let buffer = self.buffers.input_append_buf();
        let len = buffer.len().min(reply.len());
        buffer[..len].copy_from_slice(&reply[..len]);
        self.buffers.input_appended(len);
        reply.drain(..len);
        self.reply = Some(reply);
        Ok(len > 0)
    }
    fn is_open(&mut self) -> bool {
        false
    }
    fn is_tls(&self) -> bool {
        true
    }
}

pub(crate) fn fixture(prefix: &str, account: bool) -> (Proposal, Context) {
    let source = SourceKey {
        provider: Provider::Forgejo,
        host: HOST.into(),
        repository: format!("{prefix}team/app"),
    };
    let card = CardKey::try_from(&ItemKey {
        source: source.clone(),
        native_id: "7".into(),
    })
    .unwrap();
    let context = Context {
        account: String::new(),
        account_generation: 1,
        api: api_root(&source).unwrap(),
        board_generation: 1,
        mapping: Default::default(),
        mapping_generation: 1,
        route: Route {
            worker: uuid::Uuid::new_v4(),
            origin: if account {
                Origin::ForgejoAccount { host: HOST.into() }
            } else {
                Origin::Checkout(source)
            },
        },
        source_generation: uuid::Uuid::new_v4(),
    };
    (
        Proposal {
            board_generation: 1,
            card: card.clone(),
            destination: BoardColumn::InProgress,
            destination_lane: card.lane,
        },
        context,
    )
}

pub(crate) fn session(proposal: &Proposal, context: Context, peer: Arc<Mutex<Peer>>) -> Session {
    let wire = Wire(peer);
    let config = ureq::Agent::config_builder()
        .proxy(None)
        .max_redirects(0)
        .http_status_as_error(false)
        .build();
    let agent = ureq::Agent::with_parts(config, wire.clone(), wire);
    Session::new(
        proposal.card.clone(),
        context,
        Scheme::Https,
        |key| match key {
            "FORGEJO_HOST" => Some(HOST.into()),
            "FORGEJO_TOKEN" => Some(TOKEN.into()),
            _ => None,
        },
    )
    .unwrap()
    .with_agent(agent)
}

pub(crate) fn execute(proposal: Proposal, session: Session) -> kanban::moves::Completion {
    let (session, fresh) = session.connect().unwrap();
    let plan = Arc::new(MovePlan::prepare(proposal, fresh).unwrap().unwrap());
    let gate = Arc::new(MoveGate::new(1));
    let confirmed = ConfirmedMove::from_dialog(plan.clone(), gate.acquire(plan.card()).unwrap());
    let mut revalidate = true;
    confirmed.perform_for_test(
        || {
            if std::mem::take(&mut revalidate) {
                session.read()
            } else {
                session.observe()
            }
        },
        |plan, mutation| session.mutate(plan, mutation),
    )
}

#[test]
fn partial_label_state_move_preserves_unrelated_labels_and_never_replays() {
    let peer = Arc::new(Mutex::new(Peer {
        fail_state: true,
        ..Peer::default()
    }));
    let (proposal, context) = fixture("instance/", false);
    let result = execute(proposal.clone(), session(&proposal, context, peer.clone()));
    assert!(
        matches!(result.outcome, Outcome::Partial { ref error, .. } if matches!(**error, Error::Kanban(kanban::Error::MovePermission { step: PermissionStep::StateChange, status: Some(403) })))
    );
    let observation = result.observation.unwrap();
    assert_eq!(observation.item.state, "closed");
    assert_eq!(
        observation
            .item
            .labels
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>(),
        HashSet::from(["unrelated", "status: in-progress"])
    );
    let held = peer.lock().unwrap();
    assert_eq!(
        held.mutations,
        [
            (
                "DELETE".into(),
                "/instance/api/v1/repos/team/app/issues/7/labels/20".into(),
                json!(null)
            ),
            (
                "POST".into(),
                "/instance/api/v1/repos/team/app/issues/7/labels".into(),
                json!({"labels": [10]})
            ),
            (
                "PATCH".into(),
                "/instance/api/v1/repos/team/app/issues/7".into(),
                json!({"state": "open"})
            ),
        ]
    );
}

#[test]
fn unsafe_routes_labels_and_known_read_only_access_refuse_without_credential_escape() {
    let (proposal, context) = fixture("", false);
    for (api, host, token, scheme) in [
        (
            "https://github.example.test/",
            HOST,
            Some(TOKEN),
            Scheme::Https,
        ),
        (
            "https://forgejo.example.test/wrong/",
            HOST,
            Some(TOKEN),
            Scheme::Https,
        ),
        (
            "https://forgejo.example.test/",
            "https://forgejo.example.test/wrong",
            Some(TOKEN),
            Scheme::Https,
        ),
        (
            "https://forgejo.example.test/",
            HOST,
            Some(TOKEN),
            Scheme::Http,
        ),
        (
            "https://forgejo.example.test/",
            "github.example.test",
            Some(TOKEN),
            Scheme::Https,
        ),
        ("https://forgejo.example.test/", HOST, None, Scheme::Https),
    ] {
        let mut changed = context.clone();
        changed.api = api.parse().unwrap();
        let result = Session::new(proposal.card.clone(), changed, scheme, |key| match key {
            "FORGEJO_HOST" => Some(host.into()),
            "FORGEJO_TOKEN" => token.map(Into::into),
            _ => None,
        });
        assert!(result.is_err());
    }
    for refusal in [
        "exclusive",
        "ambiguous",
        "missing",
        "read-only",
        "redirect",
        "wrong-issue",
    ] {
        let mut fake = Peer::default();
        match refusal {
            "exclusive" => fake.exclusive = true,
            "ambiguous" => fake.ambiguous = true,
            "missing" => fake.missing = true,
            "read-only" => fake.read_only = true,
            "redirect" => fake.redirect = true,
            _ => {
                fake.issue_route =
                    Some(format!("https://{HOST}/api/v1/repos/team/another/issues/7"))
            }
        }
        let peer = Arc::new(Mutex::new(fake));
        let result = session(&proposal, context.clone(), peer.clone())
            .connect()
            .and_then(|(_, fresh)| MovePlan::prepare(proposal.clone(), fresh).map_err(Into::into));
        assert!(result.is_err(), "{refusal}");
        if refusal == "read-only" {
            assert!(matches!(
                result,
                Err(Error::Kanban(kanban::Error::MovePermission {
                    step: PermissionStep::AccountIdentity,
                    status: Some(403)
                }))
            ));
        }
        assert!(peer.lock().unwrap().mutations.is_empty());
    }
    // Stable-looking routes and label IDs must not authorize a replacement row.
    for (repository_id, issue_id, named) in [
        (
            Some(12),
            Some(71),
            Some(json!({"id": 12, "full_name": "team/app"})),
        ),
        (Some(11), Some(72), None),
        (None, Some(71), None),
        (Some(11), None, None),
        (
            Some(11),
            Some(71),
            Some(json!({"id": 12, "full_name": "team/app"})),
        ),
        (Some(11), Some(71), Some(json!({"full_name": "team/app"}))),
        (
            Some(11),
            Some(71),
            Some(json!({"id": 11, "full_name": "team/another"})),
        ),
    ] {
        let peer = Arc::new(Mutex::new(Peer::default()));
        let (connected, fresh) = session(&proposal, context.clone(), peer.clone())
            .connect()
            .unwrap();
        let plan = Arc::new(MovePlan::prepare(proposal.clone(), fresh).unwrap().unwrap());
        let gate = Arc::new(MoveGate::new(1));
        let confirmed =
            ConfirmedMove::from_dialog(plan.clone(), gate.acquire(plan.card()).unwrap());
        {
            let mut fake = peer.lock().unwrap();
            fake.repository_id = repository_id;
            fake.issue_id = issue_id;
            fake.issue_repository = named;
        }
        let completion = confirmed.perform_for_test(
            || connected.read(),
            |plan, mutation| connected.mutate(plan, mutation),
        );
        assert!(matches!(completion.outcome, Outcome::Refused(_)));
        assert!(!completion.attempted);
        assert!(peer.lock().unwrap().mutations.is_empty());
        assert!(
            connected
                .observe()
                .and_then(|fresh| plan.observation(fresh).map_err(Into::into))
                .is_err()
        );
    }
    for (route, step) in [
        ("/user", PermissionStep::AccountIdentity),
        ("/user/repos", PermissionStep::AccountRepositories),
        ("/repos/team/app", PermissionStep::RepositoryRights),
        ("/orgs/team/labels", PermissionStep::OrganizationLabels),
    ] {
        let (proposal, context) = fixture("", true);
        let peer = Arc::new(Mutex::new(Peer {
            denied_read: Some((route.into(), 403)),
            ..Peer::default()
        }));
        let error = session(&proposal, context, peer.clone())
            .connect()
            .err()
            .unwrap();
        assert!(
            matches!(error, Error::Kanban(kanban::Error::MovePermission { step: actual, status: Some(403) }) if actual == step)
        );
        assert!(!format!("{error} {error:?}").contains(TOKEN));
        assert!(peer.lock().unwrap().mutations.is_empty());
    }
    let peer = Arc::new(Mutex::new(Peer {
        push: false,
        ..Peer::default()
    }));
    let error = session(&proposal, context.clone(), peer.clone())
        .connect()
        .err()
        .unwrap();
    assert!(matches!(
        error,
        Error::Kanban(kanban::Error::MovePermission {
            step: PermissionStep::RepositoryRights,
            status: None
        })
    ));
    assert!(peer.lock().unwrap().mutations.is_empty());
    let peer = Arc::new(Mutex::new(Peer {
        deny: true,
        ..Peer::default()
    }));
    let result = execute(proposal.clone(), session(&proposal, context, peer.clone()));
    assert!(
        matches!(result.outcome, Outcome::Refused(ref error) if matches!(**error, Error::Kanban(kanban::Error::MovePermission { step: PermissionStep::LabelChange, status: Some(403) })))
    );
    assert_eq!(
        result
            .observation
            .unwrap()
            .item
            .labels
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>(),
        HashSet::from(["status: in-review", "unrelated"])
    );
    assert_eq!(peer.lock().unwrap().state, "closed");
}
