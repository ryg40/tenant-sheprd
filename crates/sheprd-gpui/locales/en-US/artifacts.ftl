# The Artifacts button of the activity bar and the user's `artifacts`
# program, which runs the local artifact server.

artifacts-start-title = Start the artifact server?
# $program is the program file, as ~/.local/bin/artifacts.
artifacts-start-note = Sheprd runs `{ $program } start`, which starts the local server on this computer, then opens this page. The server keeps running when Sheprd quits.
artifacts-start-yes = Start
artifacts-restart-title = Restart the artifact server?
# $program is the program file, as ~/.local/bin/artifacts.
artifacts-restart-note = The local server runs but does not answer. Sheprd runs `{ $program } off`, then `{ $program } start`, then opens this page. The server keeps running when Sheprd quits.
artifacts-restart-yes = Restart
artifacts-stop-title = Stop the artifact server?
# $program is the program file, as ~/.local/bin/artifacts.
artifacts-stop-note = Sheprd runs `{ $program } off`, which stops each local artifact server of this user. Open pages stop loading.
artifacts-stop-yes = Stop
artifacts-no = Cancel
artifacts-stopped = The artifact server stopped
artifacts-unreachable = The artifact server does not answer. Run `artifacts status` in a terminal for details.
artifacts-unconfigured = The artifacts program is not set up. Run `artifacts setup` in a terminal.
# $address is the host and port the program printed.
artifacts-started-other-address = The artifact server started at { $address }, not at the expected address, so Sheprd opened nothing
# A start ended, but the window did not show the server's page. $reason
# says why: no-workspace (no workspace is open), dialog-open (a menu page or
# dialog holds the window), or unreachable (the window could not go to the
# workspace).
artifacts-started-not-shown =
    { $reason ->
        [no-workspace] The artifact server runs, but no workspace is open to show its page. Open one, then click Artifacts.
        [dialog-open] The artifact server runs, but a dialog is open, so its page did not open. Close the dialog, then click Artifacts.
       *[unreachable] The artifact server runs, but the window could not go to the workspace to show its page.
    }
artifacts-no-address = Neither the artifacts setting nor the artifacts program names a page
# Nothing started, because a program listens on the server's port.
# $port is the port number. $reason says why Sheprd did not free it: still
# (a stop ran, and the port is still in use), unfinished (the stop did not
# finish in time), unidentified (the port list shows nothing that listens
# on it), or unlisted (the port list could not be read on this computer).
artifacts-port-taken =
    { $reason ->
        [still] Port { $port } is still in use after the stop, so the artifact server did not start
        [unfinished] The stop of what listens on port { $port } did not finish, so the artifact server did not start
        [unidentified] Port { $port } is in use, and Sheprd could not identify what listens on it, so the artifact server did not start. Stop it, or choose another port with `artifacts setup --port`.
       *[unlisted] Port { $port } is in use, and Sheprd could not read what listens on it, so the artifact server did not start
    }
