# The dispatch dialog's summary of what will start, and its start button.
# $agent is an agent's product name, such as Claude or Pi; $role a role's
# name from the user's config; $file the role's prompt file on the host;
# $host the host the agent starts on; $model a model name or
# orchestrator-launch-model-default; $branch the new branch.
orchestrator-launch-summary-role = Starts { $agent } with the role "{ $role }" ({ $file }) on { $host }, model { $model }, branch { $branch }.
orchestrator-launch-summary-default = Starts { $agent } with no role: the agent's own default, on { $host }, model { $model }, branch { $branch }.
orchestrator-launch-summary-no-agent = Nothing starts until you pick an agent.
# Values from the folded rows, said only when they are not the default.
orchestrator-launch-summary-profile = Uses the prompt profile "{ $name }".
# A review's profile marked `review: replace`.
orchestrator-launch-summary-profile-replaces = Uses the prompt profile "{ $name }", which replaces the built-in review prompt.
# $text is the first line of the extra instructions, cut with an ellipsis.
orchestrator-launch-summary-extra = Adds the extra instructions "{ $text }".
# The model the summary names when the model field is empty.
orchestrator-launch-model-default = default
orchestrator-launch-start = Start { $agent }
orchestrator-launch-start-role = Start { $agent } as { $role }
orchestrator-launch-start-pick = Start
orchestrator-launch-review = Review with { $agent }
orchestrator-launch-review-role = Review with { $agent } as { $role }
# Under "Agent default" in the role choice.
orchestrator-launch-role-default-note = The agent's own system prompt
# Why a role cannot be chosen for the chosen agent.
orchestrator-launch-role-no-agent = Pick an agent first
orchestrator-launch-role-not-for = Not set up for { $agent }
orchestrator-launch-role-no-option = { $agent } takes no role
# The folded section: prompt profile, model, extra instructions, host.
orchestrator-launch-more = MORE OPTIONS
