# The Artifacts button of the activity bar and the user's `artifacts`
# program, which runs the local artifact server.

artifacts-start-title = Start the artifact server?
# $program is the program file, as ~/.local/bin/artifacts.
artifacts-start-note = Sheprd runs `{ $program } start`, which starts the local server on this computer, then opens this page. The server keeps running when Sheprd quits.
artifacts-start-yes = Start
artifacts-stop-title = Stop the artifact server?
# $program is the program file, as ~/.local/bin/artifacts.
artifacts-stop-note = Sheprd runs `{ $program } off`, which stops each local artifact server of this user. Open pages stop loading.
artifacts-stop-yes = Stop
artifacts-no = Cancel
artifacts-stopped = The artifact server stopped
artifacts-unreachable = The artifact server does not answer. Run `artifacts status` in a terminal for details.
artifacts-unconfigured = The artifacts program is not set up. Run `artifacts setup` in a terminal.
artifacts-ask-busy = Close the open dialog first, then click Artifacts again
# $address is the host and port the program printed.
artifacts-started-elsewhere = The artifact server started at { $address }, not at the address asked about, so Sheprd opened nothing
artifacts-no-address = Neither the artifacts setting nor the artifacts program names a page
