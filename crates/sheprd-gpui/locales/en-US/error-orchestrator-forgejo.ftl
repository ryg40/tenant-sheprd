# Why a Forgejo (or Gitea) repository's issues or pull requests could not be read.
# FORGEJO_TOKEN and FORGEJO_HOST are environment variables, and read:issue and
# read:repository token scopes: keep them as they are.

error-orchestrator-forgejo-address = Forgejo's address for { $repository } is not valid
error-orchestrator-forgejo-authentication = Forgejo refused FORGEJO_TOKEN; change it, or unset it to read public repositories, and restart Sheprd
error-orchestrator-forgejo-forbidden = The token cannot read { $repository }; it needs the read:issue and read:repository scopes
# $setting is the FORGEJO_HOST value, such as "https://git.example.com/forgejo".
error-orchestrator-forgejo-host-path = FORGEJO_HOST { $setting } names a path; the list of every repository's issues reads only a Forgejo at the root of its host, so set FORGEJO_HOST to the host alone and restart Sheprd
error-orchestrator-forgejo-insecure-remote = Forgejo shows { $repository } only when signed in, and a token is never sent over plain HTTP: use an https:// remote
error-orchestrator-forgejo-json = Forgejo's answer is not the expected JSON: { $error }
# $origin is the scheme and host tried, such as https://codeberg.org.
error-orchestrator-forgejo-network = Forgejo could not be reached at { $origin }: { $error }
# An issue the account listing of $host shows has no checkout to act in.
error-orchestrator-forgejo-no-checkout = Open a checkout of the repository as a workspace to act on its issues from { $host }
error-orchestrator-forgejo-not-found = Forgejo shows no repository { $repository } to this token
error-orchestrator-forgejo-rate-limit = Forgejo rate limit reached; it is read again later
error-orchestrator-forgejo-read = Forgejo's answer could not be read: { $error }
# $host is the instance's host name, such as codeberg.org, which FORGEJO_HOST must name.
error-orchestrator-forgejo-sign-in = Forgejo shows { $repository } only when signed in: set FORGEJO_TOKEN in your shell, with FORGEJO_HOST naming { $host }, and restart Sheprd
error-orchestrator-forgejo-size = Forgejo's answer exceeded the size limit
error-orchestrator-forgejo-status = Forgejo request failed (HTTP { $status })
error-orchestrator-forgejo-token = FORGEJO_TOKEN is not one an HTTP header can carry
