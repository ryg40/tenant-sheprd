//! Usage text for every control command.
use crate::cli::control::FAMILIES;

const COMMON: &str = "\
Every command accepts --json, printing one JSON document, and --window ID,
acting in that window (IDs come from `window list`). By default a command
acts in the window showing the caller's workspace (HERDR_WORKSPACE_ID), and
exits 5 when no window shows it; outside Herdr, in the frontmost window.
Exit status: 0 done, 1 refused, 2 invalid arguments, 3 Sheprd not running,
4 nothing yet (no notes, no event), 5 not found, 6 busy (a dialog is open),
7 timed out, 8 unsupported on this platform or by this app version.";

const BROWSER: &str = "\
sheprd browser open URL|FILE [--workspace ID] [--no-focus] [--beside|--tab]
                    [--source]
    Show a web address, or a local HTML or Markdown file, in a browser tab
    of the running Sheprd. The tab joins the caller's own workspace (HERDR_WORKSPACE_ID),
    or the frontmost one outside Herdr. Showing the same page again from the
    same pane reuses and reloads its tab. Bare hosts such as localhost:3000
    are accepted. A file is served with the other files in its folder.
    A file opened from a Herdr pane goes in a split next to that terminal,
    which stays in view and keeps the keyboard; a web address takes a tab of
    the group in use. --beside and --tab choose instead.
    A Markdown file renders as a page that runs no script. The tab follows
    the file as it changes on disk. --source shows a file's source, read-only,
    in place of its page; the tab's switch, or the toggle_source command,
    changes between the two. A build that cannot render pages shows the
    source, and so does a Markdown or HTML file in a hidden folder; the
    answer then says so.
sheprd browser reload
    Reload the pages this pane opened, for example after editing the file.
sheprd browser feedback [--wait SECONDS]
    Print the notes the user sent about this pane's pages and have not been
    delivered yet. With --wait, wait up to SECONDS (at most 600) for them.
    Notes are otherwise typed into this pane as a prompt once it is idle.
sheprd browser list
    List page tabs with their IDs.
sheprd browser close|back|forward|text TAB
    Close a page tab, go back or forward in it, or print its visible text.
sheprd browser navigate TAB URL
    Load another web address in a page tab.
sheprd browser zoom TAB +|-|reset|PERCENT
    Zoom a page tab in or out a step, back to 100%, or to PERCENT (50-300),
    as the font size keys do while it is focused. The tab keeps its zoom.
sheprd browser screenshot TAB PATH
    Save a PNG of the page (macOS).
sheprd browser clear-data
    Ask the user to remove what sites keep in page tabs from one run to the
    next: cookies and site storage (Linux builds with Servo). Sheprd opens
    a dialog; nothing is removed until the user confirms there, and every
    site is signed out then. With page tabs opened in this run, the removal
    of site storage completes when Sheprd is restarted.
sheprd browser skill
    Print the agent skill that explains these commands.
sheprd browser example
    Print an example explainer page built on the kit Sheprd serves to local
    pages at /.herdr/kit.css and /.herdr/kit.js.
Exit status: 0 done, 1 refused, 2 invalid arguments, 3 Sheprd not running,
4 no notes (feedback), 5 no such tab or workspace, 6 busy, 7 timed out,
8 unsupported on this platform.";

fn family(name: &str) -> &'static str {
    match name {
        "status" => {
            "sheprd status\n    Whether Sheprd runs, its version, protocol, windows, and hosts.\n    Exits 3 when it does not run."
        }
        "skill" => "sheprd skill\n    Print the agent skill describing these commands.",
        "app" => {
            "sheprd app focus|hide|quit\n    Bring the app forward, hide it, or quit it. Quitting leaves Herdr\n    and its terminals running."
        }
        "open" => {
            "sheprd open DIR\n    Open the folder as Open Project in the command palette does: focus its\n    workspace, or start one there, and bring the window forward. DIR must\n    be an existing folder. With no Sheprd running, starts it and waits up\n    to 20 seconds for its window to reach the local Herdr (exit 7 past that)."
        }
        "window" => {
            "sheprd window [list]\n    The app's windows, with IDs for --window.\nsheprd window alerts\n    The alerts the window kept, newest first, including those held back\n    by quiet hours or with delivery off.\nsheprd window new [--session NAME]\n    Open another window: of the caller's own Herdr session, of a named\n    local session, or outside Herdr of the frontmost window's host.\nsheprd window focus ID\nsheprd window fullscreen [on|off|toggle]\nsheprd window size [WIDTHxHEIGHT]\n    Print the window's size, or resize it. Windows cannot be moved.\nsheprd window detach|reconnect\n    Detach from, or reconnect to, the window's selected host."
        }
        "ui" => {
            "sheprd ui sidebar [on|off|toggle]\nsheprd ui statusbar [on|off|toggle]\nsheprd ui font-size +|-|reset|POINTS\nsheprd ui panel workspaces|overview|pull-requests|issues|kanban|tasks|security|automations|ports|usage|walkr\nsheprd ui agent-group [NAME|all]\n    List the agents panel's coordinator groups, or show one group's sessions\n    with its coordinator first. Needs [sidebar] coordinator_suffix.\n    For this window and session; settings are changed with `config`."
        }
        "tab" => {
            "sheprd tab [list]\n    The app's own tabs: pages, code, reviews, Devices.\n    Terminal tabs belong to Herdr: use `herdr tab`.\nsheprd tab focus|close ID\n    Show a tab, switching to its workspace, or close it."
        }
        "code" => {
            "sheprd code open FILE[:LINE]\n    Show a local file read-only in a code tab, at LINE.\nsheprd code edit FILE[:LINE]\n    Open it in your terminal editor ($VISUAL, $EDITOR) in a new pane."
        }
        "devices" => {
            "sheprd devices [open]\n    Open the Devices overview tab in the caller's workspace."
        }
        "orchestrator" => {
            "sheprd orchestrator [open] [--repo PATH]\n    Show the Issues section narrowed to the caller's project, or PATH's.\nsheprd orchestrator items|issues|prs|tasks [--repo PATH | --forgejo HOST]\n    List what it last synced, from its local database (no network).\n    --forgejo HOST lists the open issues of every repository the Forgejo\n    token for HOST reads. Runs here; Sheprd need not run. Exits 4 before\n    its first sync.\nsheprd orchestrator board [--cursor CURSOR] [--json] [--window ID]\n    Read the window's prepared read-only Kanban board, without focus or\n    provider I/O. Exit 4 means preparation is pending or a stale cursor\n    requires a restart. Pages contain at most 100 cards and 192 KiB.\n    Forgejo moves need native confirmation. Done history includes up to\n    100 records per repository updated within 30 days; account reads cap\n    the whole account at 200. GitHub shows already loaded settled PRs;\n    its Done history reader arrives later.\nsheprd orchestrator board order LANE [--before LANE] [--json] [--window ID]\n    Move a known canonical lane before another, or to the end. Local\n    preferences save asynchronously; no tracker changes. Exit 5 means\n    an unknown lane; exit 6 means a busy drag or dialog.\nsheprd orchestrator board rank LANE [ITEM...] [--json] [--window ID]\n    Set a lane's order of implementation: canonical item keys from board\n    output, first to build first, at most 100; no ITEM clears it. Board\n    reads give each ranked card not in Done its rank number. Local\n    preferences save asynchronously; no tracker changes. Exit 2: a bad\n    or repeated ITEM; 4: pending; 5: unknown lane or item; 6: busy.\nsheprd orchestrator board move ITEM --lane LANE --to open|in-progress|in-review|done [--json] [--window ID]\n    Open native exact-change confirmation for a Forgejo issue. The lane\n    is its current canonical lane, not a destination repository. Success\n    means the dialog opened, not a tracker change. No --yes or --force.\n    Exit 4: pending projection; 5: unknown key; 6: busy or existing\n    dialog; 8: unsupported card or disabled activity bar.\nsheprd orchestrator repos [list] [--json] [--window ID]\n    List the repositories the window's orchestrator reads, each active\n    or not, and new when it appeared after the selection was saved.\n    At most 512 entries and 192 KiB. No provider I/O.\nsheprd orchestrator repos set KEY... | --all [--json] [--window ID]\n    Show only the repositories KEY names (canonical keys from the list),\n    or every repository with --all, in Pull requests, Issues, Tasks, and\n    Kanban. Saved locally. Exit 2: a malformed or oversized key; 5: a key\n    not listed now, which changes nothing."
        }
        "review" => {
            "sheprd review [open]\n    Open the review of the caller's workspace's uncommitted changes."
        }
        "chat" => {
            "sheprd chat [status]\n    The window's sheprd-acp chat panes on this machine or an SSH host: pane, agent, status,\n    and whether the native chat view or the terminal shows. Exits 4 when\n    there are none.\nsheprd chat show|hide|toggle [--pane ID]\n    Show the native chat view over the pane, or its terminal, for this\n    window. The pane is the caller's own, else the focused one. Exits 5 when\n    it runs no chat the window can show.\nsheprd chat new AGENT [--pane ID]\n    Open a tab running a sheprd-acp chat with AGENT (claude, codex, copilot,\n    gemini, opencode, or pi) in the workspace and folder of the pane, else the\n    caller's own. On an SSH host the tab runs that host's sheprd-acp, from\n    ~/.herdr/bin or PATH. With no Herdr pane to go by, a Herdr 2 tab in the\n    active group splits its pane in use with the chat instead. The chat view\n    attaches once the program starts.\n    Exits 8 when ACP chats are off in Settings, sheprd-acp is missing here,\n    the host cannot be reached, or the agent is not installed there.\nsheprd chat convert [--pane ID]\n    Move the terminal agent in the pane, else the caller's own, else the\n    focused one, into a sheprd-acp chat in the same pane: stops its CLI\n    with Ctrl+C and continues its session, with its messages, in the chat.\n    Claude, Codex, Copilot, and OpenCode, once idle; a draft in its input\n    is lost. Exits 6 while the agent works and 8 for an agent or pane that\n    cannot move."
        }
        "px0" => {
            "sheprd px0 [open]\n    Open or focus the caller's px0 editor tab on its local checkout.\n    Discovers a running workbench or starts installed px0 lazily.\n    px0.url configures an existing loopback HTTP workbench.\n    Success means the tab opened; server startup errors appear there.\n    Embedded pages require macOS or Windows; remote workspaces are unsupported."
        }
        "notes" => {
            "sheprd notes [list]\n    Every worktree note, most recently edited first.\nsheprd notes get|clear\nsheprd notes set TEXT...\n    The note on the caller's checkout (its repository and branch), or the\n    focused workspace's outside a Herdr pane. Notes are one line of at most\n    500 characters. `list` and `get` exit 4 when there is none."
        }
        "worktree" => {
            "sheprd worktree [status]\n    Whether the caller's worktree is muted.\nsheprd worktree mute|unmute\n    Silence its notifications, or let them through again. Mutes stay in\n    this app; nothing reaches the daemon."
        }
        "feedback" => {
            "sheprd feedback [--wait SECONDS]\n    Notes the user sent this pane: page, review, and terminal notes that\n    were not typed into it. Exits 4 when there are none."
        }
        "checkpoint" => {
            "sheprd checkpoint [list]\n    The current directory's checkout's checkpoints, newest first.\nsheprd checkpoint create [LABEL...]\n    Take one now.\nsheprd checkpoint diff CHECKPOINT [CHECKPOINT]\n    What changed between two, or since one in the working tree (untracked\n    files included).\n    These run here, on the current branch; Sheprd need not run.\nsheprd checkpoint restore [CHECKPOINT]\n    Open the caller's checkout's checkpoints dialog in Sheprd with\n    CHECKPOINT chosen; the user confirms the restore there."
        }
        "scripts" => {
            "sheprd scripts [list]\n    The current directory's repository's worktree scripts\n    (.herdr/worktree.toml). Runs here; Sheprd need not run.\nsheprd scripts setup|run\n    Run one in a new tab of the caller's workspace (setup: linked worktrees\n    only). Sheprd asks before running a script it has not seen. The\n    archive script runs only when a worktree is removed in Sheprd."
        }
        "teleport" => {
            "sheprd teleport --to HOST\n    Open the teleport dialog for the caller's linked worktree, reviewing\n    HOST (a host ID or label). The user confirms the move there."
        }
        "fan-out" => {
            "sheprd fan-out PROMPT... [--agent KIND[:COUNT]]...\n    Open the fan-out dialog for the caller's repository with PROMPT and\n    these agents (claude, codex, ...; at most 6 in all) chosen once the\n    host's agents are known. The user starts it there."
        }
        "pr" => {
            "sheprd pr [status]|checks\n    The pull request of the caller's workspace's branch, as Sheprd last\n    looked it up (selected host only; sign in to GitHub first). Exits 4\n    when there is none.\nsheprd pr comments\n    Its review comments (the workspace must be the focused one).\nsheprd pr open\n    Open it in a browser tab.\nsheprd pr quota\n    Last reported GraphQL points and rate-limit retry delays for this\n    window's GitHub accounts. Reads cached state without a network request."
        }
        "host" => {
            "sheprd host [list]\n    The window's hosts and their status. Herdr's `machine` command\n    manages saved hosts.\nsheprd host mute|unmute [HOST]\n    Silence a host's notifications as a whole, or let them through again,\n    by ID or label; absent, the caller's host. Works while it is offline.\nsheprd host select ID\n    Show that host in the window, as choosing it in the sidebar does.\nsheprd host tag ID [TAG...]\n    Replace the tags that group that host in the sidebar and on paired\n    phones; with no tags, clear them."
        }
        "cloud" => {
            "sheprd cloud [list]\n    Saved cloud machines (Sheprd need not run).\nsheprd cloud add\n    Open the settings that add a machine.\nsheprd cloud setup ID\n    Run the setup script from [cloud] in config-gpui.toml again on a saved\n    machine, named by its ID or label, in a new tab once it is shown.\nsheprd cloud images [list]\n    Sandbox images built from projects' .herdr/sandbox/Dockerfile or dev\n    container (Sheprd need not run). Exits 4 when there are none.\nsheprd cloud images build [PROVIDER]\n    Build the caller's repository's images ahead of a dispatch, for\n    PROVIDER (container, daytona) or every provider that holds them.\n    Settings > Cloud devices shows progress, logs, and Reset.\nsheprd cloud create PROVIDER [--count N] [--template T] [--preset P]\n                        [--sandbox] [--no-install] [--no-setup]\n    Ask in Sheprd to create N machines (at most 16). The user confirms\n    there, once for all of them; prints the batch to follow. Each is ready\n    once saved, with Herdr installed and the [cloud] setup script run.\n    --template and --preset are Coder's, by name; --sandbox is Azure's.\nsheprd cloud exec ID [--timeout S] [--cwd DIR] -- COMMAND...\n    Run COMMAND on a saved machine through the app's own connection to\n    it, with no input, for at most S seconds in all (default 120, at most\n    600). Its stdout and stderr come back as they are, each cut to its\n    end; exits 0 when it did, 1 otherwise. --cwd ~/x is under its home.\nsheprd cloud destroy ID... | --batch BATCH\n    Ask in Sheprd to delete saved machines at their providers and forget\n    them (a provider that cannot delete one only forgets it). The user\n    confirms there, once for all of them; prints the batch to follow.\nsheprd cloud wait BATCH [--timeout S]\n    Each machine of the batch once all are settled, or after S seconds\n    (default 60, at most 600), with its ID once made. Exits 0 when all went\n    as asked, 1 when one failed or the user said no, 4 while it is still\n    going.\nsheprd cloud batches\n    The batches Sheprd keeps."
        }
        "ports" => {
            "sheprd ports [list]\n    Ports the caller's workspace's processes listen on, and forwards.\n    Listening ports are known only while show_listening_ports is on.\nsheprd ports forward|unforward PORT\n    Forward a port of the workspace's SSH host to this machine, or stop.\nsheprd ports open PORT\n    Show localhost:PORT in a browser tab, forwarding it first if remote.\nsheprd ports all\n    Every TCP port this computer's user listens on, whoever started it:\n    process, pid, directory, uptime, and the workspace that started it.\n    macOS and Linux only. Exits 4 when nothing listens.\nsheprd ports stop|pause PORT\n    Ask in Sheprd to stop (SIGTERM, then SIGKILL) or pause (SIGSTOP) the\n    processes listening on PORT of this computer. Nothing is signalled\n    until the user confirms there. Exits 6 while a dialog is open.\nsheprd ports resume PORT\n    Resume (SIGCONT) a paused process listening on PORT.\nsheprd ports prune\n    Ask in Sheprd to stop the servers of this computer whose Herdr pane\n    is closed, listing them. Nothing is signalled until the user confirms\n    there. Exits 4 when there are none, 6 while a dialog is open."
        }
        "processes" => {
            "sheprd processes [--pane ID]\n    The processes under a pane (default HERDR_PANE_ID), with CPU and\n    memory. Runs here; Sheprd need not run."
        }
        "usage" => "sheprd usage\n    AI plan usage on the window's selected host.",
        "security" => {
            "sheprd security\n    Private vulnerability reports in triage (titles only) and open\n    Dependabot alerts of the github.com repositories the window's\n    workspaces hold, most severe first, as the Security section last read\n    them (sign in to GitHub first). Exits 4 when there are none.\n    `ui panel security` shows the section.\nsheprd security open REPOSITORY ALERT|GHSA\n    Show alert ALERT (its number) or private report GHSA (its GHSA ID) of\n    REPOSITORY (owner/name) in the Security section, beside its list, as\n    the section last read it."
        }
        "automation" => {
            "sheprd automation approve FILE [--host ID]\n    Open the approval of the automation script FILE in Sheprd: its plan,\n    caps, and content hash, for the user to approve or cancel. Nothing is\n    approved by this command. FILE is the script's path on the host, as\n    `sheprd-automate status` lists it; --host names a host by ID or label\n    (default: the caller's host). Exits 5 when the host does not list the\n    script, 6 while a dialog is open. `ui panel automations` shows the\n    section."
        }
        "system" => "sheprd system\n    The window's selected host's load, when Sheprd samples it.",
        "remote" => {
            "sheprd remote [status]\n    Whether remote control is on and its daemon runs: the addresses it\n    serves, its certificate fingerprint, and paired and connected devices.\nsheprd remote devices\n    The paired devices, with when each was last seen. Exits 4 when there\n    are none.\nsheprd remote pair [--address HTTPS_ORIGIN]\n    Open the pairing dialog in Sheprd. Tailscale is preferred by default;\n    choose an address in the dialog or pass one from remote status.\n    The QR and its token show only there; the user approves the device there.\nsheprd remote revoke DEVICE\n    Ask in Sheprd to revoke a paired device, named by its ID or name;\n    the user confirms there.\nsheprd remote restart\n    Restart the daemon from the binary beside Sheprd, as after a\n    rebuild or update; phones reconnect on their own. Exits 6 while the\n    service is already changing.\n    pair, devices, revoke, and restart exit 1 while remote control is off."
        }
        "mod" => {
            "sheprd mod [list]\n    The mods the window runs, where each draws, and any error. Mods are\n    folders in <config>/herdr/gpui-mods; saving a file reloads its mod.\n    Exits 4 when there are none.\nsheprd mod reload NAME\n    Read a mod's files again now.\nsheprd mod open NAME\n    Show a mod's activity bar app in the window's body. Exits 5 when no\n    running mod of that name has one, 8 while the activity bar is off.\nsheprd mod panel [show|hide|toggle]\n    The panel of mod sections on the window's right: whether it shows and\n    which mods have a section, or show, hide, or toggle it.\nsheprd mod logs NAME [--lines N]\n    The last lines it wrote with herdr.log (default 50, at most 200).\nsheprd mod check DIR\n    Load a mod folder, run its init and view once, and report what it\n    draws. Runs here; Sheprd need not run. Exits 1 with the error and\n    its line when the mod is invalid."
        }
        "files" => {
            "sheprd files [status]\n    The file explorer on the window's right: whether it shows, and the\n    folder it lists, which is the checkout of the window's focused pane\n    (its Git root, else its folder). Only a local checkout is listed.\nsheprd files show|hide|toggle\n    Show, hide, or toggle the explorer (also the `toggle_file_explorer`\n    command).\nsheprd files reveal PATH\n    Show the explorer with the file or folder PATH selected and the\n    folders holding it open. Exits 5 when PATH is not inside the folder\n    the explorer lists, 8 for a remote host. Opens nothing: use\n    `sheprd code open` or `sheprd browser open` to show a file."
        }
        "search" => {
            "sheprd search symbol|file QUERY... [--limit N]\n    Go to Symbol or Go to File over the current directory's checkout,\n    printing paths relative to its root. Runs here; Sheprd need not run."
        }
        "walkr" => {
            "sheprd walkr connect URL|- [--show]\n    Tell the Walkr section of this window the page address of a Walkr\n    server you started in this pane. The section shows the page while\n    the server answers. URL must be http on 127.0.0.1, localhost or\n    [::1] with a port, at most 2,048 bytes; anything else exits 2 and\n    changes nothing. The address holds a secret in its path: pass - to\n    send it as the first line of standard input, which other users of\n    the computer cannot read. Sheprd shows, logs and keeps host and port\n    only. --show also shows the section, as `ui panel walkr` does.\n    A run of another server is replaced and its page closes; the answer\n    says so. Runs in a Herdr pane only.\nsheprd walkr disconnect\n    This pane no longer holds the run. The run ends when no pane holds\n    it. The page stays open for a rating until the user selects Close\n    in the section or it is idle for 2 hours: no command closes it.\n    Exits 0 also when this pane held no run.\nsheprd walkr status\n    Whether Walkr can be reached from this computer, its host and port,\n    the age of the last probe, the source (a connected run or walkr.url),\n    whether the run is live, and for each counted host the count of saved\n    sessions and of those that wait for a pick, and for an SSH host\n    whether a live Walkr server answers there.\n    Waits at most 7 seconds for a fresh probe or saved-session list.\n    A probe older than 30 seconds or a list older than 60 seconds\n    needs a refresh. No --wait option is needed.\n    Exits 0 for every determined state, including unreachable; 4 when\n    no run is connected and walkr.url is unset (source none), and 7\n    when the probe or saved-session list does not answer in time.\nsheprd walkr clear\n    Take every page that waits for a pick out of the Walkr count and\n    waiting list of this window, as Clear in the section does, and print\n    how many left. Only Sheprd forgets them: no host and no session\n    file changes, and they stay in the history. A page that waits later\n    counts again."
        }
        "artifacts" => {
            "sheprd artifacts [status]\n    The state of the artifact server and its page, from the user's\n    artifacts program (on PATH, else ~/.local/bin/artifacts): running,\n    stopped, remote, unreachable, or unconfigured. When artifacts.url names\n    another page, the answer also names the page the Artifacts button\n    opens. Exits 8 when the program is not installed. Sheprd looks for it\n    only on Linux and macOS, so on Windows this always exits 8.\nsheprd artifacts open\n    Open the page in this window when the server answers, as the button\n    does. Starts nothing; exits 1 when the server is stopped.\nsheprd artifacts start\n    Ask in Sheprd to start the local server when the program reports it\n    stopped, also when artifacts.url names another server; the user\n    confirms there, and Sheprd then opens the started server's page.\n    Answers at once. Exits 6 while a dialog is open.\nsheprd artifacts off\n    Ask in Sheprd to stop the local server; the user confirms there.\n    Answers at once. Exits 6 while a dialog is open.\nEach waits at most 20 seconds for the program's status."
        }
        "config" => {
            "sheprd config [path]\n    The settings files.\nsheprd config get KEY\n    The setting's value, as TOML: your override, else the default.\nsheprd config set KEY VALUE\nsheprd config unset KEY\n    Change a setting in config-gpui.local.toml. VALUE is read as TOML\n    (14, true, [\"a\"], { show = true }), else as text. A value or key\n    Sheprd would not load is refused and the file left as it was.\n    A running Sheprd applies the change at once. KEY is dotted, such\n    as terminal.size. Unsetting a key that is not set exits 4.\nsheprd config check\n    Report settings that do not load and unknown keys, and, when Sheprd\n    runs, fonts that are not installed; exits 1 when there are any.\nsheprd config reload\n    Reread Herdr's config and this app's.\nOnly reload needs Sheprd running."
        }
        "theme" => {
            "sheprd theme [list]\n    The themes there are; * marks the selected one.\nsheprd theme set NAME...\n    Choose and save a theme, or light:NAME,dark:NAME to follow the\n    system appearance. Saved even when Sheprd is not running."
        }
        "font" => {
            "sheprd font [list]\n    The installed font families.\nsheprd font set NAME...\nsheprd font reset\n    The font of every face; reset goes back to the default.\nThese need Sheprd running, which knows the installed fonts."
        }
        "language" => {
            "sheprd language [status]\n    The language set, and the locale Sheprd shows: the best match for\n    the setting among the languages there are, else en-US.\nsheprd language list\n    The languages there are, each named in itself, and the en-XA and en-XB\n    pseudo-locales.\nsheprd language set system|TAG\n    Save the language setting: system follows the OS's languages; TAG is\n    a BCP 47 tag such as fr-CA. A running Sheprd switches at once.\nOnly status asks Sheprd; without it, status reports what it would show."
        }
        "keybinds" => {
            "sheprd keybinds [list]\n    The window's shortcuts.\nsheprd keybinds check\n    Herdr's custom commands this client cannot reach; exits 1 if any."
        }
        "caffeine" => {
            "sheprd caffeine [status]\nsheprd caffeine on|off|toggle [--for 90s|30m|2h]\n    Keep the display and the machine awake. With --for (at most 24h),\n    caffeine turns off then, unless it was changed in the meantime."
        }
        "sound" => {
            "sheprd sound [list]\n    The notification sounds.\nsheprd sound play [done|request]\n    Play one now, from the file Herdr's sound settings name or the\n    built-in one."
        }
        "bell" => "sheprd bell\n    Ring the system bell, as a terminal bell does.",
        "events" => {
            "sheprd events [--after CURSOR] [--wait SECONDS] [--kind KIND]... [--follow]\n    What happened since CURSOR, the INSTANCE:SEQ the last call printed:\n    windows and tabs opening and closing, notes, host status, and cloud\n    batches. A cursor\n    from an earlier app run gets everything this run kept. --follow keeps\n    printing until interrupted. Kinds:\n    window_opened, window_closed, window_focused, tab_opened, tab_closed,\n    feedback, note, host, cloud_batch."
        }
        "wait" => {
            "sheprd wait feedback|window|online|tab-closed ID [--timeout SECONDS]\n    Wait (default 60, at most 600 seconds). Exits 4 when time runs out."
        }
        "run" => {
            "sheprd run COMMAND\n    Run an app command by its [keybindings] name, as the command palette\n    does, for example toggle_sidebar. See `commands`."
        }
        "commands" => "sheprd commands\n    The commands `run` accepts, with their shortcuts.",
        "logs" => {
            "sheprd logs [--lines N] [--path]\n    The app's latest log records (default 100, at most 5000), or the log file's path.\n    Runs here; Sheprd need not run. Exits 4 when there is no log yet."
        }
        "update" => {
            "sheprd update [status]\nsheprd update check\n    Ask for the latest release. Never downloads or installs. Local\n    builds report updates as off."
        }
        "diagnostics" => {
            "sheprd diagnostics bundle PATH\n    Write build information, settings diagnostics, your settings\n    overrides, and recent logs to a new file at PATH for a bug report,\n    with home paths shortened and secret-looking values redacted. Read\n    it before sharing. Never replaces an existing file. Runs here."
        }
        "integrations" => {
            "sheprd integrations [list]\n    The agent integrations Herdr offers the window's host. Install them\n    from Sheprd's integrations settings.\nsheprd integrations install-skill\n    Install or update the agent skill for Claude Code and other agents.\n    Runs here; Sheprd need not run."
        }
        _ => "",
    }
}

/// Usage for one family, or the list of families.
pub(super) fn usage(name: Option<&str>) -> String {
    match name {
        Some("browser") => BROWSER.to_owned(),
        Some(name) => format!("{}\n\n{COMMON}", family(name)),
        None => {
            let mut text = String::from("sheprd COMMAND ... [--json] [--window ID]\n\n");
            for (name, summary) in FAMILIES {
                text.push_str(&format!("  {name:<14}{summary}\n"));
            }
            text.push_str("\nRun `sheprd COMMAND --help` for one command's usage.\n\n");
            text.push_str(COMMON);
            text
        }
    }
}
