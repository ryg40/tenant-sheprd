# The activity bar's Ports section: every TCP port a process of this
# computer's user listens on, with buttons to open, pause, resume, or stop
# that process.

local-ports-title = Ports
local-ports-count = { $count ->
    [one] One port
   *[other] { $count } ports
}
local-ports-refresh = Refresh
# How the rows are ordered: by port number, busiest first, or largest first.
local-ports-sort-port = Port
local-ports-sort-cpu = CPU
local-ports-sort-memory = Memory
# The heading over listeners the operating system and its services run,
# set apart from the user's own servers.
local-ports-system = System
local-ports-unsupported = Listing and signalling this computer's ports needs macOS or Linux.
local-ports-reading = Reading this computer's ports…
local-ports-empty = Nothing on this computer is listening for connections.
# $error is why the list could not be read, in English.
local-ports-failed = Could not read the ports: { $error }
local-ports-pid = pid { $pid }
local-ports-paused = Paused
# $workspace is the name of the Herdr workspace whose terminal started it.
local-ports-workspace = Herdr: { $workspace }
# $project is the last folder of the process's working directory, $path the
# whole directory from the home folder (~/code/app).
local-ports-directory = { $project } · { $path }
local-ports-up-minutes = up { $minutes }m
local-ports-up-hours = up { $hours }h { $minutes }m
local-ports-up-days = up { $days }d { $hours }h
local-ports-open = Open
local-ports-pause = Pause
local-ports-resume = Resume
local-ports-stop = Stop
local-ports-report-stop = { $count ->
    [one] Stopped one process.
   *[other] Stopped { $count } processes.
}
local-ports-report-pause = { $count ->
    [one] Paused one process.
   *[other] Paused { $count } processes.
}
local-ports-report-resume = { $count ->
    [one] Resumed one process.
   *[other] Resumed { $count } processes.
}
local-ports-report-killed = { $count ->
    [one] One ignored the request to quit and was killed.
   *[other] { $count } ignored the request to quit and were killed.
}
local-ports-report-gone = { $count ->
    [one] One had already exited.
   *[other] { $count } had already exited.
}
local-ports-report-failed = { $count ->
    [one] One could not be signalled.
   *[other] { $count } could not be signalled.
}
local-ports-report-unidentified = { $count ->
    [one] One could not be identified as the process listed, so it was left alone.
   *[other] { $count } could not be identified as the processes listed, so they were left alone.
}

# The question before stopping or pausing processes from the Ports section,
# or from `sheprd ports stop|pause`.
local-ports-confirm-stop-title = { $count ->
    [one] Stop this process?
   *[other] Stop these { $count } processes?
}
local-ports-confirm-pause-title = { $count ->
    [one] Pause this process?
   *[other] Pause these { $count } processes?
}
local-ports-confirm-stop-note = Each is asked to quit, and killed if it is still running a few seconds later. Unsaved work in it may be lost.
local-ports-confirm-pause-note = Each keeps its port but stops answering until you resume it.
# $process is the process's name, $address where it listens (*:3000).
local-ports-confirm-row = { $process } (pid { $pid }) on { $address }
local-ports-confirm-cancel = Cancel
local-ports-confirm-stop = Stop
local-ports-confirm-pause = Pause

# Servers whose Herdr pane is closed while they still run ("orphaned").
# A row's tag; its tooltip is listening-ports-orphaned.
local-ports-orphaned = Pane closed
# The button in the Ports section and the row in a workspace's menu that
# ask to stop every such server at once.
local-ports-prune = Stop orphaned servers…
# The question before stopping them.
local-ports-confirm-orphaned-title = { $count ->
    [one] Stop this orphaned server?
   *[other] Stop these { $count } orphaned servers?
}
local-ports-confirm-orphaned-note = The panes that started them are closed. Each is asked to quit, and killed if it is still running a few seconds later. A process whose pid now names another process is skipped.
# A row's second line: $name is what runs (vite), $uptime like "up 5m".
local-ports-confirm-detail = { $name }, { $uptime }
# What a request to stop them says when it does not ask.
local-ports-prune-none = No orphaned servers on this computer
local-ports-prune-busy = Close the open dialog first, then try again
local-ports-prune-timeout = This computer's ports were not read in time; try again
# $error says why.
local-ports-prune-failed = Could not list the orphaned servers: { $error }
