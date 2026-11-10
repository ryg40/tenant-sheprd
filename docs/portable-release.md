# Portable release

A portable release is a sanitized snapshot of this repository in the private
GitHub repository `ryg40/tenant-sheprd`. The development history stays on the
development machine. Only reviewed snapshots leave it.

The tooling is in `scripts/portable/`:

| File | Function |
| --- | --- |
| `publish_check.py` | Gives the publish set: each tracked file of a commit, minus `dev-only-files.txt` |
| `scan.py` | Scans Git objects for host values and secrets, with redacted output |
| `publish_portable.py` | Builds the snapshot commit and the tag, and pushes on request |
| `host-values.regex` | Tracked generic rules: private addresses and secret forms |
| `host-values.allow`, `host-values.allow-lines` | Tracked, reviewed exceptions of the generic rules |
| `dev-only-files.txt` | Tracked list of the files that stay in the private copy |

## Rules

- A snapshot comes from committed `HEAD` of `local-dev`, never from the working
  tree. The command refuses a working tree that is not clean.
- Snapshots chain on the local branch `portable`. Each snapshot commit and tag
  has the neutral identity `tenant-sheprd portable` and no signature. The tag
  is `portable/<yyyymmdd>-<source commit>`.
- The scan needs the local deny list `.local/host-values.deny`. Git does not
  track this file. Without it the scan stops with exit status 2, and no
  snapshot is built. The last line of a scan prints the path of the list that
  it read.
- A tag reaches the push only when it is an annotated tag with the neutral
  identity as tagger and no signature. The command scans each tag that the
  push plan names, also a tag of an earlier run. Another tag with a snapshot
  name on the snapshot commit stops the command.
- A run without `--push` contacts no remote.
- `--push` sends the branch `portable` to `main` and the tags of the snapshot,
  by explicit refspec, in one atomic push, without force. It refuses a remote
  that is not `ryg40/tenant-sheprd` on GitHub by HTTPS or SSH, and a repository
  that `gh` does not report as private.

## Release steps

1. Commit all work on `local-dev`. The snapshot reads the commit only.
2. Run `just portable-check`. It runs the tests of the tooling, prints the
   size of the publish set, and scans the publish set of `HEAD`. The last line
   must start with `scan: clean`.
3. Run `just portable-snapshot`. This is the dry run: it builds the snapshot
   commit and the tag in this repository, scans them, and prints the push plan.
4. Owner gate: show the push plan and the scan line to the owner. Continue only
   after the owner approves this push.
5. For the first push only, add the remote. The HTTPS form uses the credential
   helper of `gh`:

   ```sh
   git remote add tenant-sheprd https://github.com/ryg40/tenant-sheprd.git
   ```

6. Run `just portable-snapshot --push`. The command checks the visibility,
   prints the plan, runs `git push --dry-run`, pushes, and prints the refs of
   the remote.

A finding stops step 2 or step 3 before a ref moves. Each finding line is
`<rule> <path>:<line> <count>`. The output never holds the matched value. A
file name that holds a finding prints as `name:<hash>`, in a finding and in a
refusal.

## Resolve a finding

| Finding | Action |
| --- | --- |
| A value of this host in a fixture, a document or a comment | Replace it with a neutral example such as `example.test`, `user` or `/home/user`, in a separate commit |
| A file with no value for a reader outside | Add its path to `scripts/portable/dev-only-files.txt` |
| A documentation address or a test fixture that a generic rule finds | Add `<rule> <value>` to `host-values.allow`, or the line hash to `host-values.allow-lines` (`python3 scripts/portable/scan.py --hashes publish-set`) |

No allow list drops a finding of the local deny list. A malformed entry of an
allow list stops the scan with exit status 2.

## Add a deny value

Add one literal for each line to `.local/host-values.deny` of the main
checkout. The match ignores the case of ASCII letters only. A linked worktree
reads the list of the main checkout. `SHEPRD_PORTABLE_DENY` names another file.

Keep host values out of tracked files, and out of this document. A new
machine needs its own list before its first scan: the domain of the owner and
each host name under it, account names, the home directory, e-mail addresses,
host names, and the addresses of real hosts.

## Publish set

The publish set is each tracked file except the entries of
`scripts/portable/dev-only-files.txt`. The list excludes itself. An entry that
names no tracked file stops the command, because a mistyped entry would
publish the file that it meant.

`.github/workflows/` is on the list. The workflows would run in the snapshot
repository on each push to `main` and on schedules, and they need secrets and
runners that the repository does not have. `.github/dependabot.yml` is on
the list, because Dependabot would open branches that are no snapshot refs.
`.github/CODEOWNERS` is on the list, because it names accounts of upstream
Sheprd. The owner can take it off the list.

`LICENSE`, `NOTICE` and `crates/sheprd-protocol/NOTICE.md` must stay in the
publish set. They carry the attribution that the Apache License requires.

## How the repository stays private

- The owner creates `ryg40/tenant-sheprd` as a private repository. The tooling
  creates no repository.
- Each `--push` asks `gh repo view ryg40/tenant-sheprd --json isPrivate`. The
  push stops when the answer is not `true`, and when `gh` gives no answer.
- The command has no option that permits a public repository. To publish
  after the sign-off of the owner, change `check_visibility` in
  `scripts/portable/publish_portable.py` in a reviewed commit.
- The command refuses `--force`, `--all`, `--mirror`, `--tags` and the other
  options that send a ref that it does not name.
- Do not push `local-dev` or another development branch to the snapshot
  repository by hand. The command stops when the remote holds a ref that is no
  snapshot ref.

## Limits

- The scan finds the literals of the local deny list, as UTF-8 and UTF-16
  text, and the forms of the generic rules. It does not decode: it does not
  find a value in an encoded form (base64, compressed, URL-encoded, escaped),
  in an image or in an archive. A deny literal matches in each case of its
  ASCII letters only.
- The history scan reads the snapshot commits that no remote-tracking ref of
  the remote contains. This is as current as the last push or fetch.
- The first scan of the full tree takes approximately 15 seconds.
