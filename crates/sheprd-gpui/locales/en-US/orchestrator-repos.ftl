# The Repositories control above the Pull requests, Issues, Tasks, and
# Kanban sections, its popover, and `sheprd orchestrator repos`.
# Repository names and hosts are shown as they are.

# The control while no selection is saved, so every repository shows.
orchestrator-repos-all = Repositories: all { $count }
# The control while a selection shows some of the listed repositories.
orchestrator-repos-some = Repositories: { $shown } of { $count }
# The badge on the control: repositories listed since the selection was saved.
orchestrator-repos-new-count = { $count } new
# Beside the control when the selection could not be saved.
orchestrator-repos-unsaved = Repository selection not saved
# Beside the control when a change was refused: the saved selection already
# holds the most repository keys it can keep. All clears it.
orchestrator-repos-full = Selection full: nothing changed. Choose All to start again.
# The popover's filter field.
orchestrator-repos-filter = Filter repositories
# The popover's buttons.
orchestrator-repos-select-all = All
orchestrator-repos-select-none = None
# The popover when the filter matches no repository.
orchestrator-repos-none-found = No repositories match
# The tag on a repository listed since the selection was saved.
orchestrator-repos-new = new
# `sheprd orchestrator repos`: the first line, then one line per repository.
orchestrator-repos-listed-all = Every repository is active (no selection saved).
orchestrator-repos-listed-some = Only the active repositories show.
orchestrator-repos-state-active = active
orchestrator-repos-state-hidden = hidden
orchestrator-repos-state-new = new
orchestrator-repos-truncated = More repositories are listed than one reply holds.
# `sheprd orchestrator repos set` and its refusals.
orchestrator-repos-set = Repository selection changed.
orchestrator-repos-unknown = No listed repository has that key. Nothing changed.
orchestrator-repos-too-many = Too many or too long repository keys. Nothing changed.
orchestrator-repos-disabled = The orchestrator needs the activity bar.
