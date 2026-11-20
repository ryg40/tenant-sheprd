# Walkr

Walkr is a browser review workspace. The tenant-pi `/walkr` skill starts a
Walkr server on the computer of the agent and publishes HTML or Markdown
there for a person to review. An agent in a Herdr pane tells Sheprd the
address of that server with `sheprd walkr connect`; this is a **run**. The
Walkr section shows the page of the run while its server answers. Select
Walkr on the activity bar or run `sheprd ui panel walkr` to show the section.

## When the icon shows

Walkr is the last section of the activity bar whenever the activity bar is on
(`[sidebar] activity_bar`). `sheprd ui panel walkr` then always shows the
section.

### The count

A badge on the icon gives the number of pages that wait for a pick on every
counted host. It shows nothing at 0 and `99+` above 99, as the other counts
of the bar do. It is in the accent color while it holds a page made since
the section was last open.

- **What counts:** each saved Compare session whose `status` is `open`.
  A live run counts through its session: Sheprd's probe asks the run's
  server for `/sdk.js` only, so Sheprd does not know whether an agent waits
  for feedback, and a run is not counted on its own. The live Walkr
  server of an SSH host is not counted either: only its sessions that wait
  for a pick count. See
  [The live Walkr server of an SSH host](#the-live-walkr-server-of-an-ssh-host).
- **Which hosts:** this computer, and each SSH host that the window shows
  (an enabled host whose daemon is connected). A cloud machine, WSL or a
  socket endpoint is not counted.
- **What is read:** `status`, `createdAt` and `title` of each record, and
  `repo.name` of each of the 100 newest waiting ones, so that a click
  still finds a workspace when newer ones are cleared. The bounds are
  those of the list: at most 5,000 entries of the sessions folder, whatever they hold,
  no symlink as a folder or a record, and no record over 16 MiB. A row
  reads the first 8 KiB of its record; `repo.name` comes from the first
  64 KiB of an `open` record, read only up to the `repo` field. A record that is refused or malformed is
  skipped, and the others still count. On an SSH host, a record with a
  text field that has no UTF-8 form (a lone surrogate escape) is skipped
  too. A full list of 5,000 sessions counts all 5,000.
- **How:** each host has a reader thread of its own. This computer is read
  from the file system with the checks of [History](#history). An SSH host
  runs one read-only Python program, sent on standard input to
  `python3 -I -` through the host's own shell and SSH connection, the one
  Automations uses, so no file is written on the host. On the host, it
  opens the real path of the home (a home that is a symlink is read, as on
  this computer), then each folder down to the sessions folder and each
  session folder, relative to the folder handle before it with
  `O_NOFOLLOW`. It opens each record with `O_NOFOLLOW` and `O_NONBLOCK`,
  and checks on the opened handle that it is a regular file within
  16 MiB. It sends only `sessionId`, `status`, `createdAt`, `title` (at
  most 200 characters) and `repo.name` (at most 100), and an end line.
  Sheprd checks each line again: a line it does not know, or a list
  without its end line, fails the read. A host without `python3` fails
  its read, and the section says so; nothing is installed.
- **Time:** a read of this computer has 5 seconds. A read of an SSH host
  has 20 seconds in all: output, on standard output or standard error,
  does not give it more time. The program on the host also ends itself
  after 15 seconds with an alarm signal, wherever it waits, and Sheprd
  shows that as a read that did not end in time.
- **When:** when a window opens, every 60 seconds, when the section shows,
  and when a run connects or ends. A host joins the count at the next of
  these.
- **Failures:** no sessions folder counts 0. A host whose read fails keeps
  its last count, also when its sessions folder cannot be read. The
  section shows the failure on the line of that host, never on the badge.
- **Clear:** a session that the user cleared does not count. See
  [Clear](#clear).

The list in the section starts with one line for each counted host and its
count. The waiting sessions of an SSH host follow its line, with their
title and time. An SSH host whose Walkr server answers also has a
**Live Walkr server** row there.

### A click on the icon

With a count of 0, a click shows the section, as before.

With a count above 0, a click goes to the newest waiting page of all hosts,
by `createdAt`:

1. **The workspace.** For the session of the live run, a workspace of this
   computer that holds a pane of the run. For any other session, a
   workspace on the same host whose checkout has the repository name of
   `repo.name`. The repository name is the folder that holds the
   checkout's Git directory. Of several, the one its host shows wins, else
   the first in sidebar order (the hosts as the sidebar lists them, and a
   repository's linked worktrees after its checkout). A workspace of
   another host never matches.
2. The window goes to that workspace and shows the Walkr section.
3. **The page.** A session of this computer is picked in the list, as a
   click of its row does: the section shows the run's page when the
   session is live, and opens it again after Close or the idle limit.
   With no live run, the section opens the session's page from the Walkr
   server of this computer; see
   [The page of a waiting session](#the-page-of-a-waiting-session). A
   session of an SSH host opens its page in the section the same way,
   through an SSH tunnel.

When no workspace matches, the workspace stays and the section says that
no workspace matched.

### The page of a waiting session

A click on the icon, or on a waiting session in the list, opens that
session's page in the Walkr section. This is a session of an SSH host, or
a session of this computer when no live run is connected. The session of
a live run shows the run's page instead, as before. A session that does
not wait for a pick (picked, or of another status) shows as Sheprd's own
document of its record; see
[What a row and a session show](#what-a-row-and-a-session-show). A
session of this computer keeps that view while its page opens, and when
the page cannot open.

1. The host's reader program reads the record of the Walkr server of that
   home, `~/.human-review/server.json`, with the same no-follow and
   regular-file checks, at most 64 KiB. It needs its `port`, its `token`,
   an integer `pid`, and `protocol` 9; else no request is sent.
   `HUMAN_REVIEW_STATE_DIR` is not read.
2. It asks that server on `127.0.0.1` for `/health` without the token.
   The answer must be Walkr's, with the `pid` and the `protocol` (9) of the
   record; else the token is not sent. Then it asks, with the token, for
   its capabilities and for the path of its private Compare page. These are
   `GET` requests only. The server must serve the Compare folder of that
   home (`~/.local/.walkr/compare`). The token stays on the host: the
   program sends Sheprd only the page address with the session ID as its
   fragment. On this computer, the same program runs through this
   computer's shell.
3. Sheprd accepts the address only when it is `http` on `127.0.0.1`,
   `localhost` or `[::1]`, with a port and no user info, within 2,048
   bytes, on the page path, and its fragment is the session's ID.
4. On this computer, Sheprd shows the page at that address on
   `127.0.0.1`, with no tunnel. On an SSH host, Sheprd opens an SSH tunnel
   from the same port number on this computer
   to that port on the host (`ssh -N -L`, as a listening port of an SSH
   host opens), and shows the page through it. The Walkr server answers
   only a loopback `Host` with its own port, so the local port must be the
   host's port. When that port is busy on this computer, Sheprd opens no
   page, and the section names the busy port.
   The tunnel belongs to the page: it closes with Close, the idle limit,
   another page in the section, or the window.

The address holds the secret of that server run. Sheprd keeps it in memory
only, as the address of a run: no log, no tab, no file, and no message
names more than the port. Sheprd never starts, stops or writes to a Walkr
server. A missing server record, a server that does not answer or that
serves another folder, a host without `python3`, a busy local port, or a
tunnel that does not open gives a line under the session in the section,
and no page. A build that draws no pages says so.

On Windows, the page of a session of this computer does not open:
Windows has no `/bin/sh` for the reader program. The session's row says
so, and the session shows as text. The page of a session of an SSH host
opens on Windows as on other systems.

A run that connects closes the page and shows the run's page. When the
closed page was of this computer, open or still opening, its session is
no longer picked, so the section does not fall back to that session's
text. After a page that failed, the picked session's text stays. A click
of a session that does not wait also closes the page, and shows that
session's document.

No command opens this page. `sheprd walkr status` already counts the
waiting sessions, and an agent on the host has the page's link.

With nothing connected and no `walkr.url`, the section is a normal place,
not an error. The header says **No Walkr run is connected**. The body says
that a run appears when an agent runs the `/walkr` prompt with the Sheprd
link on, and that the saved sessions of this computer are listed there. The
list is on the left; see [History](#history).

### The live Walkr server of an SSH host

`sheprd walkr connect` works only from a pane on this computer, so a Walkr
server that an agent starts on an SSH host cannot connect a run. The
Walkr section shows that server instead, through the same tunnel as the
page of a waiting session.

1. Each list read of an SSH host also reads the host's server record and
   asks `/health` without the token, with the checks of steps 1 and 2 of
   [The page of a waiting session](#the-page-of-a-waiting-session). When
   the record and the answer agree (the same `pid`, `protocol` 9), the
   reader adds one line that says only that a server answers. No request
   of a list carries the token, and the line holds no port or address.
   That check, the read of the record included, has at most 2 seconds in
   all, and it is skipped when the read has less than 1 second left. When
   that time ends, or the record or the answer cannot be read, the host
   has no live server and its session list stays.
2. The host then has a **Live Walkr server** row under its line in the
   section. A click opens the private Compare page of that server, with no
   session in its fragment, through the steps of
   [The page of a waiting session](#the-page-of-a-waiting-session): the
   same token checks, the same address checks (and no fragment), the same
   tunnel on the same port, and the same failure lines.
3. The row is not a waiting page: the badge does not count it, and a click
   on the icon does not go to it.

The Compare page lists the Compare sessions of that server. Not supported:
the review page of a Walkr Core run (`/s/ID`). The Walkr server names that
page to no client that holds its token, so Sheprd cannot find it. Sheprd
does not guess such an address. This computer has no such row: a run on
this computer connects with `sheprd walkr connect`.

### Clear

While a page waits, the list starts with the total and a **Clear** button.
**Clear** takes every waiting session of every counted host out of the
count, as the list shows them then. Each waiting row of an SSH host also
has a button that takes that one session out. `sheprd walkr clear` does
what **Clear** does.

- A cleared session does not count on the badge, does not show in the
  waiting list, and a click on the icon does not go to it. It still shows
  in the history, and its page still opens from there.
- Sheprd alone forgets it. Nothing is written on an SSH host or into a
  session file, so its `status` stays `open` there, and another Sheprd or
  Walkr itself still shows it as waiting.
- A session that waits later counts as before. There is no undo: a cleared
  session counts again only when its record leaves the `open` state and
  waits again.
- The cleared sessions are kept with the window's other preferences of
  that daemon, so they stay cleared after a restart. Sheprd keeps the host
  and the session ID only: `local`, or a hash of the SSH target, never a
  title, a path, or an address.
- A cleared session is forgotten when two complete reads of its host in
  a row do not list it as waiting. One read can miss a record that Walkr
  rewrites at that moment. A failed read, or one that stopped at the limit
  of 5,000 entries, forgets nothing, and does not count as one of the two.
- At most 64 hosts and 20,000 sessions are kept, for all hosts together.
  At the bound, a clear adds no more sessions and reports only those it
  added: the rest still count. A host that a window no longer counts is
  never read, so its cleared sessions stay kept and use part of the bound
  until the same host counts again.
- Two windows of one daemon share the stored set. A save keeps the cleared
  sessions that the other window stored, so a restart loses neither. A
  window does not see the other window's clears until it starts again.
- When the newest waiting session is cleared, a click on the icon goes to
  the next one and to the workspace of its repository. The Walkr icon
  stays on the bar with a count of 0, as
  [When the icon shows](#when-the-icon-shows) says.

## History

The section lists the saved Compare sessions of this computer and shows one
when you pick it. No Walkr server needs to run, and the files are never
created, changed or deleted: the section only reads them.

### Where and how much

- The folder is `~/.local/.walkr/compare/sessions/<session id>/session.json`
  under the home that Sheprd uses. This is Walkr's default root.
  `HUMAN_REVIEW_COMPARE_ROOT` is not read, so a session under another root
  is not listed.
- The list reads at most 5,000 entries of the folder. It keeps the names that
  are session IDs, sorts them newest first, and keeps the newest 100.
- A symlink is refused: as a folder below the home, or as a record.
  Each opened record must be a regular file within the size bound.
  Unix opens each folder from a home directory handle with `openat` and
  `NOFOLLOW`. It opens the record with `NOFOLLOW` and `NONBLOCK` too.
  A folder replaced by a symlink cannot redirect this read.
  Windows checks folders by path and opens the record with
  `FILE_FLAG_OPEN_REPARSE_POINT`. It refuses a record whose opened handle
  has `FILE_ATTRIBUTE_REPARSE_POINT`.
- A folder-swap race remains on Windows: another program can replace a
  checked folder before the record opens. Unix trusts the home path itself.
  On both platforms, another program can move an opened folder or change
  file contents during a read. All resulting content stays bounded and
  appears as text only. The directory listing still checks by path; it
  cannot authorize a record read.
- A row reads at most the first 8 KiB of its record. Walkr writes the
  identity, the times, the status and the title first, so the prefix holds
  them.
- A row is **Unreadable session** when the folder has no record, the record
  is refused, its `schema` is not `walkr-compare-session`, its `version`
  is missing or not 1, or its `sessionId` is not the folder name.
  Its time comes from its name.
- A pick reads one record of at most 16 MiB. A larger record is not read
  past that bound, and the section says that it is too large. A record of
  another `version` than 1 is not shown.
- The `checksum` of a record is not checked. The section shows what the file
  holds.
- Each read has a time limit: 5 seconds for the list, 10 seconds for one
  session. A read that is over is dropped, and the list or the content says
  that it could not be read in time.

The checks of this section apply to the count of this computer too. An
SSH host has its own reader; see [The count](#the-count).

Each window reads on a thread of its own, never on the UI thread. The thread
runs while the section shows or a `walkr status` call asks. The list is read
when the section shows, every 60 seconds while it shows, and when a run
connects or ends. A newer request replaces one that waits. A window keeps
at most one active worker and one cancelled worker that has not ended.
If both remain blocked, requests wait without starting another thread.
The section reports the timeout. Only a successful list refreshes the count
that `walkr status` returns.

### What a row and a session show

A row shows the title, the local time of `createdAt`, the status (**Waiting
for a pick** for `open`, **Picked** for `submitted`) and a **Live** mark.
A long title is cut with an ellipsis. In a narrow section the list is above
the content.

A row is **Live** only when all three are true:

1. The run is live: a pane holds it. An ended run whose page is kept is not
   live.
2. The last probe of the run answered (`reachable` or `ready`).
3. The session ID in the fragment of the run's address is the row's ID.

When the server stops, the mark goes after the next probe. After the last
`disconnect` it goes at once.

A pick of a live row shows the page of the run, reopening it after Close
or the idle limit. A pick of a waiting row of this computer, with no live
run, opens its page; see
[The page of a waiting session](#the-page-of-a-waiting-session). A pick
of another row that does not wait, or of any row while another run is
live, shows the saved session:
the brief, the questions and answers, each
option, the pick, the comments, the note, the outcome and the ratings.

On a build with pages (`embedded: true`), the session shows in a page of
the section, as an HTML document that Sheprd makes from the bounded record.
It reads like Walkr's Compare page: a card for each option, the picked card
marked, and each rating as stars with its number, in the window's theme.
The page has no Walkr server and no other server behind it:

- The record is untrusted text. Sheprd escapes every field, so a title or
  an option that holds markup shows that markup as text.
- The document has no script, no link, no form and no image, and loads
  nothing. Its Content-Security-Policy is `default-src 'none';
  style-src 'unsafe-inline'`.
- The page loads the document through the app's private `herdr-preview:`
  scheme, from memory. The page has no folder, so that scheme serves it
  this one document and nothing else. No socket listens for it, and no file
  is written.
- This page is not the page of a run. A pick never closes a run's page, so
  **Back to live page** and **Back to the page** work as before. A run's
  page takes the content back as before.
- A request of the page for another window opens nothing.
- Leaving the section hides the page; returning shows it again. Another
  pick, a pick of a live row, or **Back** closes it.

Until the page has loaded the document, and when the page fails, the
session shows as plain text in the section, as on a build without pages.
After a failure, the session stays as text until it is picked again.

In either view no link is active, no Markdown or HTML is drawn, and nothing
is opened or run. Text and Markdown options show as plain text, up to
256 KiB each. Markdown options are not rendered: the browser's Markdown
pages keep web images and links, which this page must not load or follow.

The reader drops the content of an HTML option. The section shows its label,
its summary and the line "HTML option, N KB. It is drawn only in the live
Walkr page."

Shown text is cut: a title at 200 characters, each other field at 8,000, and
an option at 256 KiB with a line that says the rest is not shown. Control
characters other than newline and tab are removed. The reader keeps the
first 64 options, clarifications and comments while decoding. It skips the
rest without constructing them and shows each omitted count.

The detail first shows the newest 64 ratings. **Show earlier ratings** reads
64 more each time, until every rating in the bounded record is available.
Each request counts all ratings but retains only the requested newest ones.
The text view keeps its scroll position during the read and adds earlier
ratings after the ratings already shown. In the page, the button is in the
section's header, as the document has no control of its own. The page shows
the current document during the read. Then it loads a new document that
opens at the first added rating.

With a run and no pick, the page shows as before. From a session, **Back to
live page** reopens or returns to the page of a live run.
**Back to the page** returns to the kept page of an ended run.

### No list command

There is no `sheprd walkr sessions` command. `walkr-compare list` already
lists the sessions for an agent. A second command would move untrusted
record text into the context of an agent. `walkr status --json` gives the
count only.

## Run

A run is one Walkr server (host and port) and the set of panes that
connected it. Each window has at most one run.

```sh
printf '%s\n' "$URL" | sheprd walkr connect - --show
sheprd walkr disconnect
```

- `sheprd walkr connect URL [--show]` connects the server at `URL` for the
  calling pane. `URL` may be `-`: the address is then the first line of
  standard input. Other users of a computer can read the arguments of a
  process, so a program passes the address on standard input. `--show` also
  shows the section, as `ui panel walkr` does; with the activity bar off
  (`[sidebar] activity_bar = false`) it exits 8 and connects nothing. Exit 8
  also covers an action that the configuration turns off; see the
  [shared exit codes](cli.md).
- The app accepts only `http` on `127.0.0.1`, `localhost` or `[::1]`, with a
  port, without user info, in at most 2,048 bytes. Any other address exits 2
  and changes nothing. The command runs in a Herdr pane on this computer; a
  caller without `HERDR_PANE_ID` exits 1. A Walkr server on an SSH host
  shows in the section without it; see
  [The live Walkr server of an SSH host](#the-live-walkr-server-of-an-ssh-host).
- A `connect` to the same server from another pane adds that pane. With the
  same address the one page stays. With another address of that server the
  run takes the newer address: the open page closes and a new page loads the
  newer address. Reports of the earlier page no longer count.
  A `connect` to another server replaces the run and closes its page; the
  answer says so.
- A run holds at most 64 panes. A `connect` from one more pane exits 6 and
  changes nothing: no pane loses the run to another pane. A pane that holds
  the run can still connect again.
- `sheprd walkr disconnect` removes the calling pane from the run. A pane
  that leaves its daemon's snapshot counts the same. Only a snapshot of the
  same daemon start that listed the pane before proves that it is gone. It
  exits 0 also when the pane held no run.

The run is **live** while a pane holds it. When the last pane leaves, the run
is **ended**: its page stays open, so the user can still rate. A `connect` to
the server of an ended run makes it live again. An ended run that has no open
page is forgotten at once, with its address.

No command and no control request closes the page. An agent must not close a
page on which the user may still rate. The page closes only when the user
selects **Close**, when it is idle for 2 hours, when another server replaces
the run, or when the window closes.

### The address is a secret

The address of a run holds the secret of the server run in its path. Sheprd
keeps the whole address in memory only, and hands it only to the section's
page. Everything else gets host and port: the header, `walkr status`, the
answer of `connect`, every error text, the log, and `Debug` output. A usage
error of a `walkr` command names no argument, because a misplaced argument
may be the address: `sheprd walkr connect - URL` exits 2 and prints no part
of `URL`. Only a mistyped subcommand of letters is named. The
address is in no tab, so `sheprd browser list` and the saved tab file do not
hold it. A run has no **Open in a tab** and no **Open in the system browser**
button. A request of the page for a new window on the run's own server is
dropped; one for another server opens a tab as before. The probe asks the
origin (scheme, host and port) for `/sdk.js` and sends no part of the path.

### Close and the idle limit

The header of a run has a **Close** button while its page is open.

| Run | After Close, or after 2 idle hours |
| --- | --- |
| Live | The page closes. The run stays, the header keeps **Live**, and **Show page** loads the page again. A new `connect` for the run does too. |
| Ended | The page closes, and the run and its address are forgotten. The section shows `walkr.url` if it is set, else the saved sessions. |

A page is idle when 2 hours pass with no activity. The clock runs whether the
section shows or not. Activity is:

- a `connect` for this run;
- a change of the section from hidden to shown;
- a key, pointer or scroll input that the app sends to the page;
- a press of **Check again**, **Show page** or **Try again**.

On Linux with Servo the app routes each input to the page, so input counts.
On macOS and Windows the system hands input to the native page and the app
does not see it. There only `connect`, a show of the section and the buttons
count. Sheprd adds no script to the Walkr page for this.

The page of `walkr.url` has no Close button. It follows the same 2 idle hours
and loads again when the section is next shown.

Warning: text typed in a page is lost when the page closes.

An open page asks its server again and again, shown or hidden, so the Walkr
server does not stop by itself while the page is open. The probe of a shown
section also asks the server every 30 seconds while a run exists, and Walkr
counts each request as use. So the server stops by itself only when both
end. With no agent that waits, a run whose last pane disconnected, and no
activity, the page closes after 2 idle hours and the run is forgotten; the
server then stops after Walkr's own 45 minutes, 2 hours 45 minutes in all.
A live run keeps its probe while the section shows, even after idle page
closure. Hiding the section ends the periodic probe, but an open page still
makes requests. The lifetime bound does not apply while either continues.

### What the section shows for a run

| Run | Probe | Header | Body |
| --- | --- | --- | --- |
| Live | `checking`, `reachable`, `ready` | **Live · HOST:PORT**, Close | The page |
| Live, page closed | the same | **Live · HOST:PORT**, Show page | One line that the page is closed |
| Ended | `checking`, `reachable`, `ready` | **Run ended · HOST:PORT. The page stays open for a rating.**, Close | The page |
| Live or ended | `unreachable`, `down`, `unexpected`, `network_refused` | **The Walkr run on HOST:PORT does not answer**, Check again, Close | One line; the page is hidden, not closed |

In each state the list of saved sessions is beside the body; see
[History](#history).

While the probe of a run fails, no page load starts by itself. The hidden
page shows again when a probe answers. When the server was stopped, its
secret is gone with it: only Close, the idle limit or a new `connect` ends
that state. A page of a run that left the server keeps the header
**Other page · HOST**.

A build without pages (Linux without Servo) shows the header and the line
"This build shows no page in the window. Open the address the agent
printed." It has no page to keep, so it forgets a run with its last pane.

## `walkr.url` as an override

`walkr.url` names a Walkr server at a fixed address, for example one started
with `HUMAN_REVIEW_PORT`. It is an override, not the normal way in: a
connected run has priority over it. While a run exists, the probe, the header
and the page belong to the run. The rest of this document describes the
setting; the probe and the states are the same for a run.

## Configuration

The default is unset. Add this table to `config-gpui.local.toml`:

```toml
[walkr]
url = "https://walkr.example.test"
probe_path = "/sdk.js"
```

Or use the existing settings command:

```sh
sheprd config set walkr.url https://walkr.example.test
sheprd walkr status --json
```

`url` must be an HTTP or HTTPS address with a host. User info, a query, and a
fragment are refused. A path is allowed but the probe uses only the origin
(the scheme, host, and port). `probe_path` defaults to `/sdk.js`; it must
start with `/`, contain no query (`?`), fragment (`#`), whitespace or control
character, and fit in 256 bytes. A leading `//` stays on the configured host. Invalid values do not load, and `config set` leaves the overrides
file unchanged when it refuses one.

## Probe

A worker sends one `GET` to the configured origin and `probe_path`, or to the
origin of the run and `/sdk.js`. It sends
no cookie or Authorization header, follows no redirect, and does not read
the body. It does not inherit an environment proxy or its credentials. The
whole request has a five-second limit.

A status request keeps the worker active for 60 seconds. It probes at once
when the section opens. While it shows or demand is live, the worker waits
30 seconds after each probe before starting another. A network change appears
within about 30 seconds plus one bounded request and the UI polling delay.
When neither applies, the worker stops after its current request, within the
five-second limit.
Each window has at most one probe in flight. Configuration edits replace
the pending request, not the worker. The latest setting runs after the
current probe finishes. Every edit invalidates older results, including a
change from A to B and back to A.

All Walkr probes share one reusable name-lookup thread in the app. At most
one lookup can be outstanding. A stalled system lookup cannot be cancelled,
but the probe stops waiting within its request limit. Further probes report
`timeout` immediately until that lookup ends. The lookup thread remains
idle between requests, including when no window demands a probe.
The command and diagnostics show only host and port, not an address path,
query, or page text. Servo page-console logging is disabled for the Walkr
section page; other page tabs keep their existing console logging.

A `3xx` response means the probe endpoint answers. Use an endpoint that
needs no credential, such as `/sdk.js`.

## States

| JSON state | Meaning |
| --- | --- |
| `not_configured` | No run is connected and `walkr.url` is unset. The source is `none` and the command exits 4. |
| `checking` | No probe has answered yet. The command waits for a result. |
| `unreachable` | This computer cannot reach the host. `cause` is `dns`, `connect`, `timeout`, or `tls`. |
| `network_refused` | HTTP 403. Walkr refuses this address (its Host rule). |
| `down` | HTTP 5xx. HOST answers with STATUS; `status` holds the HTTP status. |
| `unexpected` | Another status outside 2xx and 3xx. Check `walkr.url` and `walkr.probe_path`; `status` holds the HTTP status. |
| `reachable` | Walkr answers with 2xx or 3xx. The section has no page yet, or its page last loaded on another origin. |
| `ready` | The section's page last loaded on the origin of the run, or on the configured Walkr origin. |

The probe result takes priority over the page state. A failed probe cannot
become `reachable` or `ready` because of a page address.
The page compares the scheme, host, and port, not its path or query.
A different port or scheme also means another origin. An invalid address
or `about:blank` leaves the previous page state unchanged.

## Section

The header shows the state in words and the host without a default port.
For `ready` it shows **Walkr · HOST:PORT**. When the page last loaded on
another origin, it shows **Other page · HOST**: the host of that page only,
never its port, path, or query.
**Check again** asks the worker for a fresh probe. It is disabled while a
probe is pending or running. Before a page opens, the body explains the
state and possible causes. Only the header's state text fades when it changes.

On a page-enabled build, selecting Walkr creates its page after the probe
answers. Restoring a saved section or requesting status alone does not create
a page. The page fills the body below the header. **Reload** reloads it.
Leaving the section hides the page without losing its text.
Returning shows the same page. Changing `walkr.url` closes it; the shown
section creates a page for the new address once its probe answers.
A failed probe after the page of `walkr.url` opens changes the header but
keeps the page. The page of a run hides then; see [Run](#run).

A page-engine failure closes the page and shows the error with **Try again**.
There is no automatic page retry. **Open in a tab** remains available as a
way around a section page fault, with the usual browser controls.

The open button is enabled only when Walkr answers. On page-enabled builds,
**Open in a tab** opens the configured address in the focused workspace and
switches to Workspaces. Without a focused workspace, Sheprd asks you to open
one first. Other builds offer **Open in the system browser**.

`sheprd ui panel workspaces` leaves the section. The icon stays on the bar
with no run, no saved session and no `walkr.url`; see
[When the icon shows](#when-the-icon-shows).
Typing while the section shows does not send text to a hidden terminal.

The section owns its page, so `sheprd browser list` does not list it.
Closing browser tabs does not close it. Other tabs and chat pages cannot
change its state. A page's request for another window opens a
valid web address in a tab of the focused workspace, except an address on the
server of a run.

A click on the header returns keyboard focus from the page to the section.
Menus and dialogs use the same page-hiding rules as browser tabs.

## Command

`sheprd walkr status`, `connect`, `disconnect` and `clear` accept `--json` and
`--window ID`, with the usual window targeting: the window of the caller's
workspace. [Run](#run) describes `connect` and `disconnect`.

`walkr status` shows the state, host and port, the probe age, the source and
the count of saved sessions. JSON contains `connectivity`, `address`,
`checked_seconds_ago`, `embedded`, `source`, `live` and `sessions`.
`sessions` holds one entry for each counted host:
`{"host": "local", "count": N, "waiting": W, "live_server": false}` for
this computer, then one for each SSH host with its SSH target as `host`.
`live_server` is true when the last read of that SSH host found its Walkr
server live; see
[The live Walkr server of an SSH host](#the-live-walkr-server-of-an-ssh-host).
It is always false for `local`, and it does not change `waiting`. `count` is the number of
listed rows (at most 100); for an SSH host it is the number of records read,
at most 100. `waiting` is the number of sessions that wait for a pick; see
[The count](#the-count). An SSH host that has not answered yet is not
listed, and one whose read fails keeps its last counts.
It holds no title or text of a session. A status call reads the list when
the last one is older than 60 seconds, within the same seven seconds. `source` is `run`, `override` (the `walkr.url`
setting), or `none` when there is neither. `live` is true only
for a run that a pane holds: an ended run whose page is kept has `source`
`run` and `live` false. `address` is host and port only.
`embedded` reports build support for pages, not whether Walkr has a page.

`waiting` counts no session that was cleared; see [Clear](#clear).
`walkr clear` clears what **Clear** does and answers the number of
sessions that left the count (JSON: `{"kind": "cleared", "count": N}`).

| Exit | Meaning |
| --- | --- |
| 0 | The app determined a state, including unreachable or down. |
| 4 | Nothing yet: no run is connected and `walkr.url` is unset. The JSON has `source` `none`. |
| 7 | No fresh probe or saved-session list answered in time. |

A result no older than 30 seconds returns at once. An older or missing
result triggers a refresh and a wait of at most seven seconds. Other
failures use the [shared CLI exit codes](cli.md).

## Platforms

The probe and state function share the same code on macOS, Linux, and
Windows. Linux
builds without Servo report `embedded: false`; macOS, Windows, and Linux
builds with Servo report `embedded: true`.

| Platform | Section and open action |
| --- | --- |
| macOS | The section embeds a private WKWebView below the window's drawing, like a chat page. A saved session shows in a second private WKWebView. |
| Linux with Servo | The section paints the Servo page below its header, like a browser tab. A saved session shows in a second Servo page. |
| Linux without Servo | Same states with no page; Open in the system browser for `walkr.url` only. A saved session shows as text. |
| Windows | The section embeds a private WebView2 above the window's drawing. Menus and dialogs hide covered pages. A saved session shows in a second WebView2, which WebView2 spells `http://herdr-preview.localhost/`. Runtime behavior is unverified here. |

macOS: unverified. Windows: unverified. The private page and saved-record
open have platform-specific code. Compilation, tests, and native window
checks on those systems require such a machine or the GitHub CI jobs.

## Browser data and limits

On macOS and Windows the Walkr page is a private (non-persistent) page: it
keeps no cookies, no history and no cache on disk, and it shares none with
the browser tabs. A sign-in on the page is gone when the page closes. Page
tabs are unchanged. On Linux, Servo has one store for every page.

| Platform | Browser data of the Walkr page |
| --- | --- |
| macOS | None on disk: WebKit's non-persistent data store. |
| Linux with Servo | `~/.local/state/herdr/gpui/browser-data`, or `$XDG_STATE_HOME/herdr/gpui/browser-data`, shared with page tabs. |
| Linux without Servo | None; the system browser keeps its own data. |
| Windows | None on disk: WebView2's private mode (InPrivate). It needs WebView2 Runtime 101.0.1210.39 or later. |

The page of a saved session asks for the same private data store on macOS
and Windows. A saved record is the user's own text, so it gets the same
care as a run's page.

On Windows, Sheprd checks the installed WebView2 Runtime before building
the Walkr page or the page of a saved session. An older runtime, an
unreadable version, or an unknown version format refuses the page: the
section shows a run's error, and a saved session stays in the text view.
Sheprd does not fall back to a persistent page.

Not verified on this machine: that no file under the app's data on macOS or
Windows holds the address after the page closes and the app restarts. The
change was reviewed as text only on those systems.

Servo keeps no browsing history or automatic HTTP page cache on disk in
this build. It persists cookies, authentication data, HTTPS site rules,
`localStorage`, and IndexedDB. It creates a `cachestorage` folder and records
Cache API names in site storage, but its request/response list is in memory.
Site data can include addresses or page contents that the site chooses to
save. A cookie without a `Path` attribute gets a path from the page address.
Servo writes that path to `cookie_jar.json`.

Warning: Servo's stored site data is not encrypted.

On Linux with Servo, `sheprd browser clear-data` also clears the data of the
Walkr page after the user confirms the app's dialog. See [site storage](linux-browser-plan.md#what-sites-keep-between-runs)
for running-engine limits and the restart needed to finish clearing some data.

Servo cannot use an input method. Some pages can stop
a tab when their script thread dies, and the tab shows no message when it does.
For `walkr.url`, use **Open in a tab** for the browser controls if the
section page fails; it uses the same engine and does not remove those engine
limits.
