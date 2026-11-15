# A Kanban card's face. The accessible name and tooltip list, in words, the
# marks the card draws: $details joins the facts below with semicolons.
kanban-card-label = { $title }; { $source }; { $column }; { $status }; { $details }
kanban-card-draft = Draft
kanban-card-ready = Ready
kanban-card-checks-failed = Checks failing
kanban-card-checks-pending = Checks running
kanban-card-checks-passed = Checks passed
kanban-card-checks-skipped = Checks skipped
# $status is a run status such as "working" or "needs input".
kanban-card-agent = Agent { $status }
kanban-card-assignee = Assigned to { $name }
# $labels is the item's labels, as the tracker names them, joined by commas.
kanban-card-labels = Labels: { $labels }
# $age reads like "3 days ago".
kanban-card-updated = Updated { $age }
# The labels a card has beyond the chips it shows.
kanban-card-more = +{ $count }
