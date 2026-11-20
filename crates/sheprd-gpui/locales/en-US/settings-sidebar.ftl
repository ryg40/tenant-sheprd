# Settings > Sidebar: what the page shows beyond the rows search finds,
# whose titles are in settings.ftl. Host names are data.

settings-sidebar-section-layout = Layout
settings-sidebar-section-spacing = Spacing
settings-sidebar-activity-bar-title = Show activity bar
settings-sidebar-activity-bar-hint = The icon column at the window's left edge that switches between workspaces, overview, pull requests, issues, security, automations, ports and usage.
settings-sidebar-automations-title = Show automations
settings-sidebar-ports-count-title = Count servers on Ports
settings-sidebar-ports-count-hint = The number of servers you run on this computer, on the activity bar's Ports icon. On, Sheprd lists this computer's ports every 30 seconds while the Ports section is closed.
settings-sidebar-automations-hint = The activity bar's Automations section. Off, Sheprd neither reads nor starts any host's automation daemon.
settings-sidebar-agents-header-hint = The Agents title row with the sort toggle. Hidden, the order follows Herdr's ui.agent_panel_sort unless a plugin sets the agent view, which keeps the row.

## Choices.

settings-sidebar-select-row = Row
settings-sidebar-select-group = Host group
settings-sidebar-select-group-dim = Group, dim others
settings-sidebar-footer-always = Always
settings-sidebar-footer-auto = Auto
settings-sidebar-footer-never = Never
settings-sidebar-counts-pull-request = Pull request
settings-sidebar-counts-local = Uncommitted
settings-sidebar-placement-panel = Own section
settings-sidebar-placement-nested = Under workspaces

## Host colours.

settings-sidebar-hosts-empty = Connect to a host to give it a colour.
settings-sidebar-host-tint = Tints the host header, its workspaces, worktrees and agents.
settings-sidebar-host-uncoloured = Connected, no colour set.
settings-sidebar-host-offline = Not connected.
# The hex field of a host with no colour.

## Artifacts.

# The artifact server's page that the activity bar's Artifacts button opens.
settings-sidebar-artifacts = Artifacts
# $example is a sample address.
settings-sidebar-artifacts-placeholder = None, or { $example }
settings-sidebar-artifacts-click-note = The address of an artifact server's page, for the Artifacts button of the activity bar. The button also shows without an address when an `artifacts` program is installed (on PATH or in ~/.local/bin, Linux and macOS only). A click asks the program first, starts a stopped local server, and opens the page. This address wins when the program serves another one. A loopback address such as 127.0.0.1 is the computer where Sheprd runs.
settings-sidebar-artifacts-invalid = Use an http or https address.
settings-sidebar-artifacts-apply = Apply artifacts address
