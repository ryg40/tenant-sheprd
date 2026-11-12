# Walkr

Walkr is a browser review workspace. The tenant-pi `/walkr` skill starts a
Walkr server on the computer of the agent and publishes HTML or Markdown
there for a person to review. An agent in a Herdr pane tells Sheprd the
address of that server with `sheprd walkr connect`; this is a **run**. The
Walkr section shows the page of the run while its server answers. Select
Walkr on the activity bar or run `sheprd ui panel walkr` to show the section.

## When the icon shows

The Walkr icon is last on the activity bar while one of these is true:

- A run exists: live, or ended with its page kept.
- This computer has a saved Walkr Compare session.
- `walkr.url` is set.

A saved session is a directory under
`~/.local/.walkr/compare/sessions` whose name is a session ID, for example
`20261101T120000Z-0123456789ab`. The home is the one Sheprd uses for its own
files: `HOME`, or `USERPROFILE` on Windows. A worker thread checks the folder
when a window opens, each time the section is shown, and after a run
connects, ends or is dropped. The check reads at most 5,000 entries of the
folder and opens no file. A symlink does not count, as the folder or as an
entry. The check goes by path: a program of the same user that swaps the
folder for a symlink while it runs can show the icon, and nothing more.
Another root that Walkr can use is not read.

The list and the detail do not trust this check. They verify each opened
record; see [History](#history) for the platform checks and remaining races.

When none of the three is true, the icon leaves the bar. A window that showed
the section shows Workspaces, and `sheprd ui panel walkr` exits 8.

With nothing connected and no `walkr.url`, the section is a normal place,
not an error. The header says **No Walkr run is connected**. The body says
that a run appears when an agent runs the `/walkr` prompt with the Sheprd
link on, and that the saved sessions of this computer are listed there. The
list is on the left; see [History](#history).

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
  appears as text only. The icon and directory listing still check by path;
  neither can authorize a record read.
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
or the idle limit. A pick of another row shows the session as plain text:
the brief, the questions and answers, each
option, the pick, the comments, the note, the outcome and the ratings.
No link is active, no Markdown or HTML is drawn, and nothing is opened or
run. Text and Markdown options show as plain text, up to 256 KiB each.

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
The view keeps its scroll position during the read and adds earlier ratings
after the ratings already shown.

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
  caller without `HERDR_PANE_ID` exits 1.
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
| Ended | The page closes, and the run and its address are forgotten. The section shows `walkr.url` if it is set, else the saved sessions; with neither, the icon leaves the bar. |

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

`sheprd ui panel workspaces` leaves the section. With no run and no saved
session, removing `walkr.url` hides the icon and shows Workspaces, including
when a saved window selected Walkr. With none of the three conditions of
[When the icon shows](#when-the-icon-shows), `ui panel walkr` exits 8
without changing the window.
Typing while the section shows does not send text to a hidden terminal.

The section owns its page, so `sheprd browser list` does not list it.
Closing browser tabs does not close it. Other tabs and chat pages cannot
change its state. A page's request for another window opens a
valid web address in a tab of the focused workspace, except an address on the
server of a run.

A click on the header returns keyboard focus from the page to the section.
Menus and dialogs use the same page-hiding rules as browser tabs.

## Command

`sheprd walkr status`, `connect` and `disconnect` accept `--json` and
`--window ID`, with the usual window targeting: the window of the caller's
workspace. [Run](#run) describes `connect` and `disconnect`.

`walkr status` shows the state, host and port, the probe age, the source and
the count of saved sessions. JSON contains `connectivity`, `address`,
`checked_seconds_ago`, `embedded`, `source`, `live` and `sessions`.
`sessions` holds one entry for each host, `{"host": "local", "count": N}`
for this computer, where `count` is the number of listed rows (at most 100).
It holds no title or text of a session. A status call reads the list when
the last one is older than 60 seconds, within the same seven seconds. `source` is `run`, `override` (the `walkr.url`
setting), or `none` when there is neither. `live` is true only
for a run that a pane holds: an ended run whose page is kept has `source`
`run` and `live` false. `address` is host and port only.
`embedded` reports build support for pages, not whether Walkr has a page.

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
| macOS | The section embeds a private WKWebView below the window's drawing, like a chat page. |
| Linux with Servo | The section paints the Servo page below its header, like a browser tab. |
| Linux without Servo | Same states with no page; Open in the system browser for `walkr.url` only. |
| Windows | The section embeds a private WebView2 above the window's drawing. Menus and dialogs hide covered pages. Runtime behavior is unverified here. |

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

On Windows, Sheprd checks the installed WebView2 Runtime before building
the Walkr page. An older runtime, an unreadable version, or an unknown
version format refuses the page. Sheprd does not fall back to a persistent
page.

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

Servo cannot paste into the page or use an input method. Some pages can stop
a tab when their script thread dies, and the tab shows no message when it does.
For `walkr.url`, use **Open in a tab** for the browser controls if the
section page fails; it uses the same engine and does not remove those engine
limits.
