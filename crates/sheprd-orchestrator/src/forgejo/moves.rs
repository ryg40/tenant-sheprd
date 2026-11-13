//! Forgejo/Gitea issue deltas behind one-shot native approval. No public writer.

use std::{
    collections::{HashSet, hash_map::RandomState},
    ffi::OsString,
    hash::BuildHasher,
    sync::{Arc, OnceLock},
};

pub use crate::forge::Scheme;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, de::IgnoredAny};

use super::{LIMIT, MAX_OPEN, MAX_PAGES, PAGE, TIMEOUT, authorization, body, collection_url};
use crate::{
    Provider, Result, SourceKey,
    forge::agent,
    kanban::{
        self,
        facts::{LabelCompleteness, NativeStateProvenance, WorkflowFacts},
        identity::{CardKey, LaneKey, Origin},
        moves::{
            Attempt, Context, Fresh, Identity, Label, LabelId, Labels, MovePlan, Mutation,
            PermissionStep,
        },
    },
};

/// Frozen credential and route. Only the action lifecycle can use its writer.
#[derive(Clone)]
pub struct Session {
    agent: ureq::Agent,
    api: url::Url,
    card: CardKey,
    context: Context,
    identity: Option<Identity>,
    labels: Option<Labels>,
    token: SecretString,
}

impl Session {
    /// No I/O. Resolve credentials on the worker before planning; never switch
    /// credentials after confirmation. `context.api` names the instance root.
    pub fn new(
        card: CardKey,
        mut context: Context,
        scheme: Scheme,
        variable: impl Fn(&str) -> Option<OsString>,
    ) -> Result<Self> {
        let source = &card.item.source;
        let api = api_root(source)?;
        context.route.validate(source)?;
        if scheme != Scheme::Https || context.api != api || LaneKey::try_from(source)? != card.lane
        {
            return Err(kanban::Error::Route.into());
        }
        if card
            .item
            .native_id
            .parse::<u64>()
            .ok()
            .is_none_or(|n| n == 0 || n.to_string() != card.item.native_id)
        {
            return Err(kanban::Error::ReadOnlyMove.into());
        }
        let setting = variable(super::HOST_VARIABLE)
            .and_then(|s| s.into_string().ok())
            .ok_or_else(|| permission(PermissionStep::AccountIdentity, None))?;
        let named = setting.trim();
        let named = if named.contains("://") {
            named.to_owned()
        } else {
            format!("https://{named}")
        };
        let mut named = url::Url::parse(&named).map_err(|_| kanban::Error::Route)?;
        if !named.path().ends_with('/') {
            named.set_path(&format!("{}/", named.path()));
        }
        // A folder-scoped token cannot reach the host root or another instance.
        // A bare host retains the existing checkout reader's host-wide policy.
        if named.scheme() != "https"
            || named.origin() != api.origin()
            || !named.username().is_empty()
            || named.password().is_some()
            || named.query().is_some()
            || named.fragment().is_some()
            || (named.path() != "/" && named.path() != api.path())
        {
            return Err(kanban::Error::Route.into());
        }
        if matches!(context.route.origin, Origin::ForgejoAccount { .. }) && api.path() != "/" {
            return Err(kanban::Error::Route.into());
        }
        let token = super::environment_token(&source.host, variable)
            .ok_or_else(|| permission(PermissionStep::AccountIdentity, None))?;
        authorization(&token)?;
        // A process-keyed fingerprint binds the credential without retaining or
        // exposing its bytes in Context. Reusing the same credential must not
        // invalidate another pending card in this lane.
        static GENERATIONS: OnceLock<RandomState> = OnceLock::new();
        context.account_generation = GENERATIONS
            .get_or_init(RandomState::new)
            .hash_one((context.account_generation, token.expose_secret()));
        Ok(Self {
            agent: agent(TIMEOUT),
            api,
            card,
            context,
            identity: None,
            labels: None,
            token,
        })
    }

    /// Read-only preflight. Repository access, identity, complete labels and
    /// definitions are fetched again by the same session before the first write.
    pub fn read(&self) -> Result<Fresh> {
        let user: Account = self.get(self.endpoint(&["user"])?, PermissionStep::AccountIdentity)?;
        if user.id == 0 || user.login.trim().is_empty() {
            return Err(permission(PermissionStep::AccountIdentity, None));
        }
        let account = format!("{}:{}", user.id, user.login);
        if !self.context.account.is_empty() && self.context.account != account {
            return Err(kanban::Error::StaleMove.into());
        }
        let repository: Repository =
            self.get(self.repository_url()?, PermissionStep::RepositoryRights)?;
        self.validate_repository(&repository)?;
        if matches!(self.context.route.origin, Origin::ForgejoAccount { .. }) {
            let repositories = self.list(
                self.endpoint(&["user", "repos"])?,
                PermissionStep::AccountRepositories,
                |repo: &Repository| repo.id,
            )?;
            if !repositories
                .iter()
                .any(|repo| repo.id == repository.id && repo.full_name == repository.full_name)
            {
                return Err(kanban::Error::Route.into());
            }
        }
        // This is repository capability evidence, not proof of the token's scope.
        // Servers check write:issue on the real request. Never send a no-op probe.
        if repository.archived
            || !repository.has_issues
            || repository
                .permissions
                .as_ref()
                .is_none_or(|p| !p.admin && !p.push)
        {
            return Err(permission(PermissionStep::RepositoryRights, None));
        }
        let mut labels = self.list(
            self.repository_child("labels")?,
            PermissionStep::RepositoryLabels,
            |label: &Definition| label.id,
        )?;
        // Repository labels exclude organization labels. A 404 on the organization
        // endpoint is the API's individual-owner response; other failures refuse.
        match self.list(
            self.endpoint(&["orgs", &repository.owner.login, "labels"])?,
            PermissionStep::OrganizationLabels,
            |label: &Definition| label.id,
        ) {
            Ok(inherited) => labels.extend(inherited),
            Err(crate::Error::Forgejo(super::Error::Status(404))) => {}
            Err(error) => return Err(error),
        }
        let resolved = Labels {
            in_progress: resolve(&labels, self.context.mapping.in_progress_label())?,
            in_review: resolve(&labels, self.context.mapping.in_review_label())?,
        };
        let mut context = self.context.clone();
        context.account = account;
        self.observation(context, resolved, repository.id)
    }

    /// Final exact-item read remains possible after permission was revoked or a
    /// label definition changed. It reports facts, not a new authorization.
    pub(crate) fn observe(&self) -> Result<Fresh> {
        let repository: Repository =
            self.get(self.repository_url()?, PermissionStep::RepositoryRights)?;
        self.validate_repository(&repository)?;
        self.observation(
            self.context.clone(),
            self.labels.clone().ok_or(kanban::Error::MissingApproval)?,
            repository.id,
        )
    }

    fn observation(&self, context: Context, resolved: Labels, repository: u64) -> Result<Fresh> {
        let node: Issue = self.get(self.issue_url()?, PermissionStep::IssueRead)?;
        let expected = self.issue_url()?;
        if node.id == 0
            || node.url != expected.as_str()
            || node.entry.number.to_string() != self.card.item.native_id
        {
            return Err(kanban::Error::Route.into());
        }
        if let Some(named) = &node.repository {
            let parts: Vec<_> = self.card.item.source.repository.split('/').collect();
            let name = parts[parts.len() - 2..].join("/");
            if named.id != repository || !named.full_name.eq_ignore_ascii_case(&name) {
                return Err(kanban::Error::Route.into());
            }
        }
        let identity = Identity::Forgejo {
            repository,
            issue: node.id,
        };
        if node.pull_request.is_some() {
            return Err(kanban::Error::ReadOnlyMove.into());
        }
        // The unpaged issue-label endpoint proves membership even when issue JSON
        // encodes no labels as null. Match IDs as well as names to avoid aliases.
        let held: Vec<Definition> =
            self.get(self.issue_child("labels")?, PermissionStep::IssueRead)?;
        if held.len() > MAX_OPEN {
            return Err(kanban::Error::ReadOnlyMove.into());
        }
        for definition in &held {
            if definition.id == 0 {
                return Err(kanban::Error::UnsafeLabel.into());
            }
            for label in [&resolved.in_progress, &resolved.in_review]
                .into_iter()
                .flatten()
            {
                if (definition.name == label.name || LabelId::Forgejo(definition.id) == label.id)
                    && (definition.name != label.name
                        || LabelId::Forgejo(definition.id) != label.id)
                {
                    return Err(kanban::Error::UnsafeLabel.into());
                }
            }
        }
        let facts = WorkflowFacts {
            labels: Some(LabelCompleteness::Complete),
            native_state: Some(NativeStateProvenance::Tracker),
            settled_at: node.entry.closed_at,
        };
        let mut item = node
            .entry
            .into_item(&self.card.item.source, self.card.item.native_id.clone());
        item.labels = held.into_iter().map(|label| label.name).collect();
        Ok(Fresh {
            context,
            facts,
            identity,
            item,
            labels: resolved,
        })
    }

    /// Freeze the freshly authenticated account before configuring lifecycle admission.
    /// Call on an action worker, never the UI thread.
    pub fn connect(mut self) -> Result<(Self, Fresh)> {
        let fresh = self.read()?;
        self.context = fresh.context.clone();
        self.identity = Some(fresh.identity);
        self.labels = Some(fresh.labels.clone());
        Ok((self, fresh))
    }

    pub(crate) fn context(&self) -> &Context {
        &self.context
    }

    pub(crate) fn card(&self) -> &CardKey {
        &self.card
    }

    pub(crate) fn mutate(&self, plan: &MovePlan, mutation: &Mutation) -> Attempt {
        if plan.card() != &self.card
            || plan.context() != &self.context
            || Some(plan.identity()) != self.identity
            || !plan.mutations().contains(mutation)
        {
            return Attempt::Refused(Arc::new(kanban::Error::Route.into()));
        }
        let result = self.send(mutation);
        match result {
            Ok(()) => Attempt::Applied,
            Err(error) => {
                let refused = matches!(
                    &error,
                    crate::Error::Kanban(
                        kanban::Error::MovePermission { .. }
                            | kanban::Error::Route
                            | kanban::Error::UnsafeLabel
                    ) | crate::Error::Forgejo(
                        super::Error::Status(300..=399 | 400 | 404 | 405 | 409 | 410 | 422 | 429)
                            | super::Error::Token
                    )
                );
                if refused {
                    Attempt::Refused(Arc::new(error))
                } else {
                    Attempt::Unknown(Arc::new(error))
                }
            }
        }
    }

    fn send(&self, mutation: &Mutation) -> Result<()> {
        let auth = authorization(&self.token)?;
        let step = match mutation {
            Mutation::SetState(_) => PermissionStep::StateChange,
            _ => PermissionStep::LabelChange,
        };
        let mut reply = match mutation {
            Mutation::RemoveLabel(LabelId::Forgejo(id)) if *id > 0 => self
                .agent
                .delete(append(self.issue_child("labels")?, &id.to_string())?.as_str())
                .header("Authorization", auth)
                .header("User-Agent", "Sheprd")
                .call(),
            Mutation::AddLabel(LabelId::Forgejo(id)) if *id > 0 => self
                .agent
                .post(self.issue_child("labels")?.as_str())
                .header("Authorization", auth)
                .header("User-Agent", "Sheprd")
                .header("Content-Type", "application/json")
                .send(serde_json::json!({"labels": [id]}).to_string().as_bytes()),
            Mutation::SetState(state) => self
                .agent
                .patch(self.issue_url()?.as_str())
                .header("Authorization", auth)
                .header("User-Agent", "Sheprd")
                .header("Content-Type", "application/json")
                .send(
                    serde_json::json!({"state": state.as_str()})
                        .to_string()
                        .as_bytes(),
                ),
            _ => return Err(kanban::Error::UnsafeLabel.into()),
        }
        .map_err(|source| self.network(source))?;
        let status = reply.status().as_u16();
        if matches!(status, 200 | 201 | 204) {
            body(&mut reply, LIMIT)?;
            Ok(())
        } else {
            Err(response_error(status, step))
        }
    }

    fn validate_repository(&self, repository: &Repository) -> Result<()> {
        let source = &self.card.item.source;
        let parts: Vec<_> = source.repository.split('/').collect();
        let name = parts[parts.len() - 2..].join("/");
        if repository.id == 0
            || !repository.full_name.eq_ignore_ascii_case(&name)
            || !repository
                .owner
                .login
                .eq_ignore_ascii_case(parts[parts.len() - 2])
        {
            return Err(kanban::Error::Route.into());
        }
        Ok(())
    }

    fn endpoint(&self, parts: &[&str]) -> Result<url::Url> {
        let mut url = self.api.clone();
        url.path_segments_mut()
            .map_err(|()| kanban::Error::Route)?
            .pop_if_empty()
            .extend(["api", "v1"])
            .extend(parts);
        Ok(url)
    }

    fn repository_url(&self) -> Result<url::Url> {
        let mut url = self.repository_child("issues")?;
        url.path_segments_mut()
            .map_err(|()| kanban::Error::Route)?
            .pop();
        Ok(url)
    }

    fn repository_child(&self, child: &str) -> Result<url::Url> {
        Ok(collection_url(
            &self.card.item.source,
            Scheme::Https,
            child,
        )?)
    }

    fn issue_url(&self) -> Result<url::Url> {
        append(self.repository_child("issues")?, &self.card.item.native_id)
    }

    fn issue_child(&self, child: &str) -> Result<url::Url> {
        append(self.issue_url()?, child)
    }

    fn network(&self, source: ureq::Error) -> crate::Error {
        super::Error::Network {
            origin: self.api.origin().ascii_serialization(),
            source: Box::new(source),
        }
        .into()
    }

    fn get<T: serde::de::DeserializeOwned>(
        &self,
        url: url::Url,
        step: PermissionStep,
    ) -> Result<T> {
        let (value, _) = self.fetch(url, step)?;
        Ok(value)
    }

    fn fetch<T: serde::de::DeserializeOwned>(
        &self,
        url: url::Url,
        step: PermissionStep,
    ) -> Result<(T, Option<u64>)> {
        let mut reply = self
            .agent
            .get(url.as_str())
            .header("User-Agent", "Sheprd")
            .header("Accept", "application/json")
            .header("Authorization", authorization(&self.token)?)
            .call()
            .map_err(|source| self.network(source))?;
        let status = reply.status().as_u16();
        if status != 200 {
            return Err(response_error(status, step));
        }
        let total = reply
            .headers()
            .get("x-total-count")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok());
        let value =
            serde_json::from_slice(&body(&mut reply, LIMIT)?).map_err(super::Error::Json)?;
        Ok((value, total))
    }

    fn list<T: serde::de::DeserializeOwned>(
        &self,
        url: url::Url,
        step: PermissionStep,
        id: impl Fn(&T) -> u64,
    ) -> Result<Vec<T>> {
        let mut result = Vec::new();
        let mut seen = HashSet::new();
        let mut expected_total = None;
        for page in 1..=MAX_PAGES {
            let mut page_url = url.clone();
            page_url
                .query_pairs_mut()
                .append_pair("limit", &PAGE.to_string())
                .append_pair("page", &page.to_string());
            let (nodes, total): (Vec<T>, _) = self.fetch(page_url, step)?;
            if page > 1 && total != expected_total {
                return Err(kanban::Error::ReadOnlyMove.into());
            }
            expected_total = total;
            let empty = nodes.is_empty();
            if nodes
                .iter()
                .any(|node| id(node) == 0 || !seen.insert(id(node)))
            {
                return Err(kanban::Error::ReadOnlyMove.into());
            }
            result.extend(nodes);
            if result.len() > MAX_OPEN || total.is_some_and(|total| result.len() as u64 > total) {
                return Err(kanban::Error::ReadOnlyMove.into());
            }
            // Read to an empty page without a total: administrators can lower page size.
            if total.is_some_and(|total| result.len() as u64 >= total) || (empty && total.is_none())
            {
                return Ok(result);
            }
            if empty {
                break;
            }
        }
        Err(kanban::Error::ReadOnlyMove.into())
    }

    #[cfg(test)]
    pub(crate) fn with_agent(mut self, agent: ureq::Agent) -> Self {
        self.agent = agent;
        self
    }
}

/// Exact instance root, including folders before owner/repository.
pub fn api_root(source: &SourceKey) -> Result<url::Url> {
    LaneKey::try_from(source)?;
    if source.provider != Provider::Forgejo {
        return Err(kanban::Error::Route.into());
    }
    let mut url = collection_url(source, Scheme::Https, "issues")?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(kanban::Error::Route.into());
    }
    let parts: Vec<_> = source.repository.split('/').collect();
    let mut segments = url.path_segments_mut().map_err(|()| kanban::Error::Route)?;
    segments.clear().extend(&parts[..parts.len() - 2]).push("");
    drop(segments);
    Ok(url)
}

fn append(mut url: url::Url, segment: &str) -> Result<url::Url> {
    url.path_segments_mut()
        .map_err(|()| kanban::Error::Route)?
        .pop_if_empty()
        .push(segment);
    Ok(url)
}

fn permission(step: PermissionStep, status: Option<u16>) -> crate::Error {
    kanban::Error::MovePermission { step, status }.into()
}

fn response_error(status: u16, step: PermissionStep) -> crate::Error {
    if matches!(status, 401 | 403) {
        permission(step, Some(status))
    } else {
        super::Error::Status(status).into()
    }
}

fn resolve(definitions: &[Definition], name: &str) -> Result<Option<Label>> {
    let found: Vec<_> = definitions
        .iter()
        .filter(|label| label.name == name)
        .collect();
    let ids: HashSet<_> = found.iter().map(|label| label.id).collect();
    if ids.len() > 1
        || found
            .iter()
            .any(|label| label.id == 0 || label.exclusive != Some(false))
    {
        return Err(kanban::Error::UnsafeLabel.into());
    }
    Ok(found.first().map(|label| Label {
        exclusive: false,
        id: LabelId::Forgejo(label.id),
        name: label.name.clone(),
    }))
}

#[derive(Deserialize)]
struct Account {
    id: u64,
    login: String,
}

#[derive(Deserialize)]
struct Repository {
    #[serde(default)]
    id: u64,
    full_name: String,
    owner: Owner,
    permissions: Option<Permissions>,
    archived: bool,
    has_issues: bool,
}

#[derive(Deserialize)]
struct Owner {
    login: String,
}

#[derive(Deserialize)]
struct Permissions {
    admin: bool,
    push: bool,
}

#[derive(Deserialize)]
struct Issue {
    #[serde(default)]
    id: u64,
    repository: Option<IssueRepository>,
    #[serde(flatten)]
    entry: super::issue::Entry,
    url: String,
    pull_request: Option<IgnoredAny>,
}

#[derive(Deserialize)]
struct IssueRepository {
    #[serde(default)]
    id: u64,
    #[serde(default)]
    full_name: String,
}

#[derive(Deserialize)]
struct Definition {
    id: u64,
    name: String,
    exclusive: Option<bool>,
}
