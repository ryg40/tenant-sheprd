# Orchestrator

The orchestrator (`crates/sheprd-gpui/src/orchestrator/`) lists a repository's
issues, pull requests, and the agent runs dispatched for them in the activity
bar's Pull requests, Issues, and Tasks sections, and dispatches new ones through
the Herdr CLI. User-facing behavior is in the crate README's "Orchestrator"
section; this page is for working on it.

## State and restart

The orchestrator keeps its own per-repository SQLite database, in the app's
state directory: `<state dir>/repositories/<uuid v5 of the canonical git
common dir>/state.sqlite3`, private to the user. Repositories on SSH hosts
are keyed by `ssh://<destination><git common dir>` and live on this machine.
Its schema began as agent-launcher's (`PRAGMA user_version`, currently 3:
version 2 added each open pull request's last read review and checks, and
version 3 each bead's assignee and acceptance criteria):

- The first time a repository opens, agent-launcher's database at
  `<data_local_dir>/agent-launcher/repositories/<same uuid>/state.sqlite3`
  is copied in, if there is one, so its items and runs come along. Nothing
  is written back to it afterwards.
- Runs keep their `runs.owner` (`agent-launcher` or `herdr-gpui`); only
  Sheprd's own runs are stopped, messaged, or removed. The persisted owner token
  stays `herdr-gpui` across the rename.
- A source's issues are replaced only with a complete listing. A capped
  listing, or one made while the schema is newer than this build knows, is
  shown and not written. A newer schema opens read-only, with a banner.

A restart shows what the last run found before any network or git call:
the repository each checkout held comes from the app's cache database
(`<state dir>/cache/cache.sqlite3`, `crate::cache`), then the items, runs,
and checks from the repository's database. Detection and the first sync
follow at once and replace it, and syncing then runs on its timer. A
checkout that no longer holds that repository drops what was shown.

`store.rs` implements the schema; its tests open a version-0 database as
agent-launcher wrote it and check that migration keeps its rows.

## Layout

| Module | Responsibility |
| --- | --- |
| `model.rs`, `location.rs`, `store.rs` | Records in the database's shapes, its path, and SQLite access. |
| `repo.rs` | Finding a checkout's repository, main checkout, Beads, and remote on its host. |
| `github.rs`, `gitlab.rs`, `beads.rs` | The sources: a full listing each. GitHub reads only what changed since the last sync, both listings' first pages in one request, with a full listing again every 30 minutes; a listing larger than 2,000 open items is shown, not stored, and continues from memory the same way, staying capped until a full listing reads it all. GitLab lists its open issues and merge requests in full on every sync. |
| `review.rs` | Each open pull request's review decision, review requests, checks, and merge state, kept in the database, so a restart shows the last ones until the first sync. A GitHub sync reads them again only when an open pull request's head or update time moved, a check is still running, GitHub answered that it has not worked out the merge state yet, five minutes passed, or the user pressed Refresh. |
| `service.rs` | The worker thread per open view: sync, reload, and the action queue. Sources sync every minute while the Pull requests or Issues section is shown and every ten minutes otherwise (`service/schedule.rs`); a source failing again waits twice as long each time, up to 30 minutes unless the provider asks for longer. |
| `actions.rs` | Dispatch, stop, send, remove, delete: each on its own thread. |
| `prompt.rs` | Prompt profiles, agent-launcher's built-in and review prompts. |
| `role.rs` | Agent roles from the app's `[[agent_roles]]` config, and their prompt file on the agent's host. |
| `naming.rs` | agent-launcher's branch names, and runs found from branches. |
| `beads/claim.rs` | Claiming a bead for a dispatch, and giving it back. |
| `view/` | The view entity: the list of every repository (`view/repo.rs` keeps each one's worker, snapshot, and rows; an item's page and actions go to its own), the backend and project pills, filters, the item pane, dialogs. Pure row, merge, and grouping logic is in `view/rows.rs`; what the Tasks section adds is in `view/tasks.rs`. |
| `host.rs` | The window's one view and the repositories it lists, one per Git common directory per host across every connected host's workspaces: the tick that feeds it, and its events. |
| `section.rs` | The Pull requests, Issues, and Tasks sections, and the commands that open them narrowed to a project. |

The view is a GPUI entity that talks to its host only through `Event`s and
setters (`set_look`, `set_live`, `set_github`, `set_hosts`). Tabs hosted it
before the activity bar; tabs saved by those builds are dropped on load. Nothing in it blocks: the worker does every disk, network, and host
script call, and the UI reads the newest `Snapshot` from a coalescing mailbox.

Live agent status comes from each connected host's Herdr snapshot, matched to
runs by host, workspace, and pane; it is never written back to the database.

Dispatch can receive `agent_pane_busy` while the new worktree's shell runs its
startup hooks. It retries only that structured rejection for up to ten seconds,
checking the terminal identity returned by worktree creation before each retry.
An accepted launch, transport failure, or agent-readiness timeout is never
replayed. Waiting happens on the action worker and observes cancellation;
permanent failures retain the normal failed-run recording and worktree cleanup.

## Branches name their item

Dispatch names a branch as agent-launcher does: `agent/<identifier>-<title>-<hash>`,
where `<hash>` is the first eight hex digits of a UUIDv5 (URL namespace) of the
item's canonical key. A read-only review uses `agent/review-<identifier>-<title>-<hash>`.
The hash makes the branch the item's own, so work is found without a database:

- a Herdr workspace on a connected host whose branch ends in an item's hash,
  working when an agent is in it and stopped when none is;
- a branch under `refs/heads/agent/` in the repository, read on each sync, that
  no workspace shows.

Each becomes a run owned by `branch`, shown like any other but never written,
and only when no recorded run already has that branch. It opens its workspace;
it has no Stop, Send, or Remove, since no application owns it.

## Review profiles

`compose_review` keeps the built-in read-only prompt and appends the chosen
profile. `Profile::parse` takes a first line of `review: replace` out of the
template and sets `Review::Replace`, and such a profile is the whole prompt
(plus extra instructions), rendered with the `pr_` variables as well as the
`issue_` ones. The marker is removed at load so that no render path can send
it, including an issue dispatch, where the same profile replaces the built-in
prompt anyway. Other profiles never see the `pr_` variables: they are left as
written, as they were before the marker existed. `{{ pr_title }}` and
`{{ pr_body }}` stay marked as untrusted, and ref and repository names outside
`valid_branch`'s characters are withheld, since Git allows `$(…)` in a branch
name and a profile may put one in a command. The rest of what the built-in
prompt guards is the profile author's to keep.

## Agent roles

A role starts a dispatched agent with a system prompt from a file, for
example a coordinator role that plans, delegates, and reviews. A prompt
profile changes only the first message; a role changes the agent's system
prompt. Sheprd ships no role. Add one to `config-gpui.local.toml`:

```toml
[[agent_roles]]
name = "tenant-pi coordinator"
agents = ["claude", "pi"]
prompt_file = "~/.local/state/tenant-pi/claude-role/coordinator.md"
```

- `name` is the label in the dispatch dialog.
- `agents` lists the agent kinds, by Herdr's names, that can take the role.
- `prompt_file` is a path on the host where the agent runs. A leading `~/`
  is that host's home. It must be absolute or start with `~/`.

An unknown agent, an empty name, or a `prompt_file` that is not absolute
stops the config from loading, as other bad values do.

The dialog's Role row shows "Agent default" and each role whose `agents`
holds the chosen agent. Only Claude (`--append-system-prompt-file <path>`) and
Pi (`--append-system-prompt <path>`) take a role, so the row does not show
for other agents. A change of agent that the role does not fit goes back to
"Agent default". The start is
`herdr agent start ... -- [--model <model>] <role option> <path>`.

Before anything is claimed, recorded, or created, the dispatch checks that
the prompt file is on the agent's host: the local disk, or a script on the
SSH host. A missing file stops the dispatch with an error.

## Tasks

Tasks is a third list on the same view, not a store of its own: Beads remains
the source of truth, and the database only caches what `bd` last listed, as it
does for Issues. `Tab::Tasks` lists the Beads items of `Item`, and only while it
shows are its rows computed. The bar lists the section while a project has a
`.beads` folder.

A task is `ItemKey`: its source and its Beads id. The source's `repository` is
the folder holding the database `bd` opens, so identical ids in two projects stay
apart, while everything that reaches one database is one source and the
existing dedupe of rows, pills, and counts applies. `.beads/redirect` is read in
discovery as `bd` reads it (the first line neither blank nor a comment, relative
to the project), by `repo.rs` on this machine and `discover.sh` on an SSH host;
`RepoInfo::beads_root` keeps the result. A target not named `.beads` has no such
folder, so it stays the project's source and `bd` follows the redirect itself;
projects sharing one are not deduplicated. When a project's source moves, as
adding a redirect moves it, `Store::settle_beads` forgets the old source's rows
and moves this app's runs to the new one once discovery confirms it, never from
cached details, so tasks keep their runs and `orchestrator tasks` reads the same rows. A database this
build cannot write keeps the old rows, so `reload` still lists only the current
source's.

A dispatch to a bead claims it before anything else happens (`beads::claim`):
`bd show` says whether it was held, then `bd update --claim`, which is atomic.
Claiming runs as `bd` by hand does, without `--sandbox`, since a claim has to
reach a shared Dolt remote to coordinate anything.
`bd` marks a refused claim only in its words (`already claimed by NAME`, or
`already claimed: already assigned to "NAME"`), read at that boundary into
`claim::Error`; any other refusal stays `bd`'s own failure. Nothing was started
then, so no run is recorded. A failed launch runs `bd unclaim --if-assignee`, but
only for a claim that dispatch took: a second agent on a task its actor already
holds leaves it. The actor is `bd`'s own default. Agents start on the database's
host: embedded databases are not reachable from another, so the dialog offers
none. `bd` leases a claim for five minutes, which is why the prompt asks for
`bd heartbeat`; a stale lease is only ever recovered by `bd reclaim`, never here.

## GitLab

`gitlab.rs` lists a GitLab project's open issues through the REST API
(`/api/v4/projects/<escaped path>/issues`), on gitlab.com or a self-managed
host, in agent-launcher's GitLab row format: `native_id` is the issue's
project `iid`. Its open merge requests (`.../merge_requests?state=opened`, read
the same way, with the same token) join them as pull requests, keyed `pr/<iid>`
and shown as `!<iid>`, with their source and target branches and draft state.
`recent_issues` reads one bounded page instead, the 50 most recently updated
open issues (`order_by=updated_at&sort=desc`), for the new worktree dialog's
issues tab; it never touches the cache.
A refused merge request listing fails the sync and preserves the cached
items. Merge requests are read-only: the status, conversation, checks, merge,
and `gh` review pages are GitHub's, so a merge request has none of them and
opens on its description. GitLab's account identity is not yet used for the
Mine filter. The branch-aware titlebar card remains GitHub-only. The API is reached over HTTPS, or over HTTP when the remote's
own URL is `http://`; the shared source key names only the host, so the scheme
is read from the remote each time.

A remote whose host the GitLab CLI names as an instance's `ssh_host` is read
from that instance instead: requests go to its `api_host` (the instance's own
name when unset) over its `api_protocol` (HTTPS when unset), and its token is
the instance's. This is how a self-managed GitLab with a separate SSH host,
such as `gitlab-ssh.example.com` beside `gitlab.example.com`, is reached; the
SSH host serves no API. The mapping comes from the `hosts` table of glab's
`config.yml`, found where glab looks (`GLAB_CONFIG_DIR`, the legacy
`~/.config/glab-cli`, else `glab-cli` in the platform's config folder or
`XDG_CONFIG_HOME`), and is read again on every sync. Only the block form glab
writes is read; glab has no command that lists its hosts. A remote on an
instance's own host, or on a host glab does not know, keeps its own host and
the remote's scheme, as before, and the source key always keeps the remote's
host, as agent-launcher keys it. A failed connection names the address it
tried.

A token is sent only over HTTPS: over HTTP
the read is anonymous, and a project that is not public asks for an
`https://` remote. Pages follow a cursor on creation time
(`order_by=created_at`, `created_after`), which edits and closes cannot shift. Each sync is a complete listing of at most 2,000 issues, so it
may replace the cache; a larger project is shown and not written, as GitHub's
is.

Its token is looked up once per view on the worker, and again on Refresh,
when the remote's instance changes, or when GitLab refuses it (once, before
asking to sign in, so a connected account renewed since is picked up):

1. the `GITLAB_TOKEN`, then `GITLAB_ACCESS_TOKEN`, that the checkout's
   `.envrc` sets (`gitlab/envrc.rs`): `direnv exec <checkout> env`, only for a
   checkout on this computer with an `.envrc` there or above it. direnv loads
   only a file the user allowed, so nothing runs that their shell would not.
   direnv runs without the app's own token variables, so only a token the
   `.envrc` sets counts. What it loads is reused for five minutes while every
   `.envrc` is unchanged, and closing the view stops a load under way;
2. else `GITLAB_TOKEN`, then `GITLAB_ACCESS_TOKEN`, from the login shell (or
   the app's own environment), only for the instance `GITLAB_HOST` names
   (gitlab.com when unset), so a gitlab.com token never reaches another host.
   An `.envrc` that sets `GITLAB_HOST` names the instance in step 1 too;
3. else the account connected, or the token pasted, in the account menu for
   the instance (`gitlab/saved.rs`). A connected account's access token is
   renewed shortly before it expires;
4. else `glab config get token --host <instance>`, the GitLab CLI's own sign-in,
   with its environment tokens removed and the shared bounded process policy;
5. else none: public projects still list, and a private one asks for a token.

A missing or refused token (`SignIn`, `Authentication`) shows the source as
"sign in", as a signed-out GitHub does, and the chip opens the account menu's
GitLab page; the title bar's account button shows its attention dot meanwhile.
It still waits 30 minutes before the next try, which Refresh skips. Other
refusals, such as a token without the `read_api` scope, stay failures, and so
does a refused `.envrc` or shell `GITLAB_TOKEN` (`EnvrcRefused`,
`EnvironmentRefused`): a pasted token is only tried after them, so the message
asks for the file or variable to change instead.

The account button at the right of the title bar opens one menu for both
providers, with a GitHub and a GitLab tab in its header. The GitLab tab shows
a card for each GitLab instance the open views of every window read, and every
instance with a saved token, once each by its lowercase name (a remote on an SSH host
of its own counts as the instance glab maps it to, and its token is checked at
that instance's `api_host`), with where its token comes from and whom GitLab
signs it in as (`/api/v4/user`, always over HTTPS; `gitlab/account.rs`), with
the account's picture when it is gitlab.com's or Gravatar's (a self-managed
instance's is not fetched, and its card shows the GitLab mark). The saved
record is read once and each instance is checked on its own, so a slow one
never holds the others' cards. A card checked again keeps what it showed,
picture included, until the new answer lands, and closing the menu stops the
checks. While no instance has a token, the tab shows a
single **Connect** button for the instance this window reads instead. An
instance every remote reads over plain `http://` takes no token, so its card
says so and the page refuses to save one for it. The token form is folded
behind a link and **Add instance**, which starts it at an empty instance for
one no repository reads yet; Escape folds it again. A pasted personal access
token is checked with GitLab before it is saved, stays in its field until the
save succeeds, and saving or forgetting one makes every view sync again.

**Connect** signs in with OAuth's device authorization grant
(`gitlab/oauth.rs`), as GitHub's sign-in does: GitLab issues a code, the page
shows it with **Open GitLab** and **Copy code**, and polls `/oauth/token` on
timers until the user approves it there. The application is public, so only
its ID is sent: gitlab.com uses Sheprd's own, and a self-managed instance
gets Connect once `[gitlab] applications` names one for it. The approval page
must be an HTTPS page on the instance, or nothing opens. The granted pair is
checked with `/api/v4/user`, then kept like a pasted token. GitLab's access
tokens last two hours and its refresh tokens are single-use, so whoever reads
a connected account's token (`saved::token`) renews it, one renewal per
instance at a time, when it is within 10 minutes of expiring, and keeps the
new pair before using it. The saved record is not locked while GitLab answers,
so other instances, saves, and forgets never wait on it, and a pair renewed
after a Disconnect or a new Connect is dropped. A renewed pair the store
refuses is still used, and kept in memory and saved again by the next lookup,
since GitLab has spent the stored one. A renewal that fails keeps the old
token, which GitLab refuses once it expires, so the view asks to connect again
and the card offers **Connect again**. **Disconnect** removes the pair. Closing
the menu stops a connection waiting for approval. A saved record not in the expected form is
reported on the page and replaced by the next save or forget; a store that
cannot be read at all is never overwritten. Saved tokens are one
credential-store record of tab-separated lines (`host<TAB>token`, or
`host<TAB>oauth<TAB>access<TAB>refresh<TAB>expires_at<TAB>application`), at
most 16 instances: the
desktop keyring (`dev.herdr.gpui.gitlab`), or with `[gitlab]
allow_plaintext_credentials = true` the private `0600` `gitlab-credentials`
file beside the GUI config. A headless window never reads or writes them.

Azure DevOps repositories are read with `AZURE_DEVOPS_EXT_PAT` from the
login shell, else the personal access token saved for the repository's
organization, else the Azure CLI's sign-in (`az login`). The account menu's
**Azure DevOps** tab saves a token: type the organization (it starts at one
the open checkouts read) and paste a token with the Code (Read) and Work
Items (Read) scopes, made from the page's link to
`https://dev.azure.com/<organization>/_usersSettings/tokens`. **Check and
save** asks the organization's `_apis/connectionData` whom it signs in as and
`_apis/projects` how many projects it shows before saving, and keeps the
token in its field when the check fails. Each organization's card shows the
account, where its token comes from, and **Forget** for one saved here. A
repository waiting for a token shows a banner in its section that opens the
tab, and marks the title bar's account button. Tokens are one
credential-store record of `organization<TAB>token` lines, at most 32
organizations: the desktop keyring (`dev.herdr.gpui.azure-devops`), or with
`[azure_devops] allow_plaintext_credentials = true` the private `0600`
`azure-devops-credentials` file beside the GUI config. Changing that setting
reaches running workers at once, which sync again with the new store. When
Azure DevOps refuses a token, the worker tries the saved token, then the
Azure CLI's sign-in, and the shell's token last: that one is read once per
run, so it may be the token just refused. A saved token that is still the
refused one goes after them all.

Merge requests are not listed yet. They would need `PullRequest`-like
metadata in the shared schema, which is a contract change for both
applications.

## Forgejo

`forgejo.rs` lists a Forgejo repository's open issues and pull requests
through the REST API under `/api/v1`, read-only. Gitea keeps the same API, so
a Gitea instance reads the same way. Every sync is a complete listing, so it
may replace the cache:

- issues: `GET /api/v1/repos/<owner>/<repo>/issues?state=open&type=issues&sort=oldest&limit=50&page=<n>`,
  in GitHub's row format: `native_id` is the issue's number, shown `#<n>`;
- pull requests: `GET /api/v1/repos/<owner>/<repo>/pulls?state=open&sort=oldest&limit=50&page=<n>`,
  keyed `pr/<n>` and also shown `#<n>`, since Forgejo numbers issues and pull
  requests in one sequence. They keep their head and base branches and
  commits, draft state, diff size, and the fork they come from.

Rows keep the title, body (untrusted text, as every source's), `html_url`,
author, labels, first assignee, comment counts, and creation and update
times. The shared helpers GitLab also uses (the scheme, what a request
carries, the agent, and rate-limit waits) live in `forge.rs`.

**Detection.** `parse_remote_url` reads a remote as Forgejo when its host's
name holds `forgejo` or `gitea`, or is `codeberg.org`. That check follows the
`gitlab` and `github` ones, so their hosts read as before, and comes ahead of
`ghe`, which a Forgejo host's name can hold by chance. A host whose name
reads as GitHub or GitLab (other than github.com and gitlab.com) is also
Forgejo when `FORGEJO_HOST` names it: each sync checks that before glab's
`ssh_host` mapping (`forgejo::recognize`). The source key keeps the remote's
host and path, `forgejo:<host>:<owner>/<repo>`. A path with more parts is an
instance served under a folder, so `https://example.com/git/team/app` is
read at `https://example.com/git/api/v1/repos/team/app/...`.

**Scheme.** HTTPS, or HTTP when the remote's own URL is `http://`, as
GitLab's. An SSH remote's port is SSH's, so
`ssh://git@forgejo.example.com:2222/team/app.git` is read at
`https://forgejo.example.com/api/v1/repos/team/app/...`.

**Token.** `FORGEJO_TOKEN` from the login shell (or the app's own
environment), sent as `Authorization: token <value>` only to the host
`FORGEJO_HOST` names: a bare host, `host:port`, or a URL, compared without
case. A checkout's source sends it to the address its remote gives, so a
`FORGEJO_HOST` with a path, such as `https://git.example.com/forgejo`, serves
an instance in a folder. Without `FORGEJO_HOST` no host gets it, codeberg.org included, and
every other Forgejo is read anonymously, which public repositories allow. A
token is sent only over HTTPS: to an `http://` remote it is withheld and the
read is anonymous. A token needs the `read:issue` and `read:repository`
scopes. The login shell is read once per run, so changing either variable
takes a restart. The header is marked sensitive and requests follow no
redirect, so the token is never logged, shown, or carried to another address.

**Limits.** Pages of 50, Forgejo's default `MAX_RESPONSE_ITEMS`, read until a
short one; a short page whose `X-Total-Count` says more are open reads on,
for an instance whose `MAX_RESPONSE_ITEMS` is lower. At most 2,000 open issues
and as many pull requests: a larger listing is shown and not written, as
GitHub's and GitLab's are, and so is one whose next page adds nothing new
or that reaches 100 pages, as full pages of entries the filter should have
left out would.
Each request times out after 30 seconds, and its answer may be at most 4 MiB.
Pages are numbered, so an item closed while later pages are read can move one
onto a page already read; that listing misses it until the next sync.
`sort=oldest` keeps items opened meanwhile from moving anything.

**Refusals.** A collection answered 404 while the other lists is empty, as
Forgejo answers a repository with its issues or pull requests turned off, or
an outside tracker. Both answered 404 is the repository's refusal: without a
token it asks for `FORGEJO_TOKEN` and `FORGEJO_HOST` (`SignIn`), with one
withheld over HTTP for an `https://` remote (`InsecureRemote`), and with one
sent it says the token cannot see it (`NotFound`). A refused token
(`Authentication`) asks for `FORGEJO_TOKEN` to change. These wait 30 minutes
before the next try, which Refresh skips; a rate limit waits as long as
`Retry-After` asks, from five minutes to a day. Forgejo has no sign-in page
in the account menu, so a failure shows as the source's failure banner, not a
"sign in" chip.

**Account.** With a token for the host `FORGEJO_HOST` names, the Issues
section also lists every open issue of every repository that token reads,
checkout or not (`forgejo/search.rs`):
`GET /api/v1/repos/issues/search?type=issues&state=open&sort=oldest&limit=50&page=<n>`
(Forgejo's default, `latest`, sorts newest first, so an issue opened during
the listing moves every other one a page on),
with the same token, scheme (HTTPS only), page and size limits, and
`X-Total-Count` rule as a repository's listing, at most 100 pages. Each issue
is keyed by its own repository, the reply's `repository.full_name`, so it is
the same row a checkout of that repository lists. The view looks the setting
up once, off the UI thread (`forgejo::account_host`), and adds one project
read by a worker of its own with no checkout (`service/forgejo_account.rs`,
`RemoteScope::ForgejoAccount`), which syncs every five minutes while shown,
as Azure DevOps's remote scopes do, and is not counted against the 32
checkouts a view lists. Its database is keyed by the host
(`Repository::ForgejoAccount`, `repositories/<uuid v5 of
forgejo-account://<host>>/state.sqlite3`) and stores the listing under the
source `forgejo:<host>:<host>`; a capped listing is shown and not written, so
a restart shows the last complete one. A repository some checkout lists is
left to that checkout: its issues show once, as the checkout's rows, with
its actions. The account's other rows name their repository as
`<owner>/<name>` in the project column, and the Projects menu lists each of
those repositories to pick. They have no checkout, so they offer no Start
agent, dispatch, or other action (the snapshot grants no access); their page
opens the issue's web page as any row's does. `sheprd orchestrator issues
--forgejo HOST` lists the stored listing. Without `FORGEJO_HOST`, or a
token for it, nothing is added. The host must be a bare host or `host:port`:
an account on an instance served under a folder is not read, and its project
shows the `HostPath` error.

Left out for now: writing to Forgejo (merge, review, comments), the Mine
filter, the title bar's pull request card, avatars, the new worktree dialog's
issues tab, and a sign-in menu, saved accounts, OAuth, or `.envrc` tokens. A
Forgejo pull request opens on its description, as a GitLab merge request
does, since the status, conversation, checks, merge, and review pages are
GitHub's. The view marks Forgejo with Git's branch icon, as no Forgejo logo
is bundled.

## Azure DevOps

`azure.rs` reads an Azure DevOps repository (`dev.azure.com`, keyed
`organization/project/repository`) with the token `AZURE_DEVOPS_EXT_PAT`
names, else the Azure CLI's sign-in: its active pull requests, the latest
100 completed and 25 abandoned ones, and its project's open work items
(keyed `organization/project`, so two repositories of one project list each
once). The reader's display name, from `connectionData`, makes Mine work.

Beyond the open checkouts, each organization a checkout's remote names
becomes one more project, named after the organization, read by a remote
worker (`service/remote.rs`, `Service::start_remote`) that needs no
checkout: it finds no repository, reads no branches, opens no database, and
takes no action (one asked for is refused with a notice). It lists every
pull request the reader created in the organization
(`_apis/git/pullrequests?searchCriteria.creatorId=<connectionData id>&searchCriteria.status=all`),
in every state, a page of 100 at a time up to 500, each keyed by its own
repository, so a pull request a checkout lists too shows once. It also reads
the organization's catalog of projects and repositories
(`_apis/git/repositories`, at most 2,000, disabled ones left out) when it
starts and on Refresh. A remote worker reads again every five minutes.

The Pull requests search row's Projects menu lists each organization's
catalog; picking a repository starts a remote worker for its pull requests
alone (active and recently settled, no work items), at most 16 at once,
until it is picked again or its pill is closed. These last the session: an
organization stays once known, and nothing about them is saved. The State
menu beside the Open toggle narrows the list to all, open (with drafts, as
the toggle does), draft, merged, or closed pull requests; Azure DevOps'
abandoned ones read as closed.

Mine on Azure DevOps matches the reader by identity: each listing keeps
the keys of the pull requests whose `createdBy.id` is connectionData's
`authenticatedUser.id` (`Snapshot::authored`, in memory only), since two
people can share a display name. Each active pull request's reviewer votes
give its review verdict (`azure::pull_request::review`), shown in the
Review column beside GitHub's and never stored.

A section narrows to some projects through the projects menu in its search
row ("All projects" while none is picked); backends keep their pills.
