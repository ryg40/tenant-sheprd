# Why a role from the agent_roles config cannot be offered or taken.
# agent_roles and prompt_file are config keys: keep them as they are.

# $role is the role's name and $agent the agent named, each in quotes.
error-orchestrator-role-agent = agent_roles: { $role } names { $agent }, which is not an agent Herdr knows
# $path is the configured prompt_file, in quotes.
error-orchestrator-role-missing = The role's prompt file { $path } is not on the agent's host
error-orchestrator-role-name = agent_roles: each role needs a name of 1 to { $max } characters
# $role is the role's name, in quotes.
error-orchestrator-role-prompt-file = agent_roles: the prompt_file of { $role } must be an absolute path or start with ~/
error-orchestrator-role-too-many = agent_roles: at most { $max } roles
