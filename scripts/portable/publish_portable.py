#!/usr/bin/env python3
"""Snapshot the publish set of HEAD onto the local `portable` branch, and push it on request.

The development history stays on this machine. This command takes the publish set
(scripts/portable/publish_check.py) from committed HEAD, never from the working tree, chains
it onto the previous snapshot (or starts a new history with `--new-root`), and tags it
`portable/<yyyymmdd>-<source commit>`. It writes no file inside the checkout.

A run without `--push` contacts and changes no remote: it builds the snapshot commit and the
tag in this repository and prints the push plan. With `--push` the command sends the snapshot
branch and the tags of that snapshot by explicit refspec, in one atomic push without force,
after `git push --dry-run`.

Before a ref moves, scripts/portable/scan.py reads the tree of the snapshot commit, the
snapshot commits that the remote does not have, and the tag. Each tag that the push plan names
must be an annotated tag with the neutral identity as tagger, and is scanned again, also when
the tree did not change. The scan needs the local deny list. A push also needs a remote that is the GitHub repository below, and that repository
must be private.

The snapshot commit and the tag carry a neutral identity and no signature, so that the Git
identity and the signing key of the publishing host do not leave with a snapshot.
"""
import argparse
import collections
import datetime
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
if __name__ == "__main__":
    sys.dont_write_bytecode = True
sys.path.insert(0, str(HERE))

import scan  # noqa: E402
from publish_check import git, publish_entries  # noqa: E402

# The one repository that receives snapshots. It stays private until the owner decides otherwise;
# that decision is a change of `check_visibility`, not an option.
REPOSITORY = "ryg40/tenant-sheprd"
REMOTE_URL = f"https://github.com/{REPOSITORY}.git"
DEFAULT_REMOTE = "tenant-sheprd"
IDENTITY = ("tenant-sheprd portable", "portable@tenant-sheprd.invalid")
# A signature would carry the key identity of the host.
NO_SIGNATURE = ("-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false", "-c", "tag.forceSignAnnotated=false")
SNAPSHOT_TAG = re.compile(r"portable/[0-9]{8}-[0-9a-f]{7}")
# A remote name goes into a revision range of the scan, so it holds no blank and no glob character.
REMOTE_NAME = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]*")
# The owner and the name of a GitHub repository in an HTTPS URL, an SSH URL or the short SSH form.
GITHUB_URL = re.compile(r"(?i)(?:https://(?:[^/@]*@)?(?:www\.)?github\.com/|ssh://(?:[^/@]*@)?github\.com(?::[0-9]+)?/"
                        r"|(?:[^/@:]+@)?github\.com:)([^/]+)/([^/]+?)(?:\.git)?/?")


def identity_env():
    """The host environment with the neutral identity as author and committer (the tagger is the committer).

    `TZ=UTC` keeps the time zone offset of the host out of the commit date and the tag date.
    """
    name, email = IDENTITY
    return dict(os.environ, GIT_AUTHOR_NAME=name, GIT_AUTHOR_EMAIL=email,
                GIT_COMMITTER_NAME=name, GIT_COMMITTER_EMAIL=email, TZ="UTC")


# --- refusals -------------------------------------------------------------------


def check_snapshot_branch(root, branch):
    """Reserve snapshot updates for the portable namespace, never a checked-out branch."""
    git("check-ref-format", f"refs/heads/{branch}", root=root)
    if not (branch == "portable" or branch.startswith(("portable-", "portable/"))):
        raise SystemExit("refused: the snapshot branch must be named portable, portable-* or portable/*")
    worktrees = git("worktree", "list", "--porcelain", "-z", root=root).split("\0")
    if f"branch refs/heads/{branch}" in worktrees:
        raise SystemExit(f"refused: the snapshot branch {branch} is checked out in a worktree")


def check_repository(root, source_branch):
    """Refuse a repository state in which a snapshot is not the reviewed source."""
    if git("status", "--porcelain", "--untracked-files=all", root=root):
        raise SystemExit("refused: the working tree is not clean; commit first, the snapshot comes from HEAD only")
    if git("rev-parse", "--is-shallow-repository", root=root) == "true":
        raise SystemExit("refused: the repository is shallow; the snapshot needs the full previous chain")
    current = git("symbolic-ref", "-q", "--short", "HEAD", root=root, check_call=False)
    if current != source_branch:
        raise SystemExit(f"refused: HEAD is {'on the branch ' + current if current else 'detached'}, not on the "
                         f"development branch {source_branch}; a snapshot comes from the reviewed development branch "
                         "(--source-branch names another one)")


def check_parent_chain(root, parent, source_ref, branch):
    """Refuse a previous snapshot that is not a chain of snapshot commits only.

    The chain goes to the remote with the next push. It must share no commit with the source,
    and each of its commits has the snapshot identity as author and committer and one parent
    at most. A branch that someone created from a development commit fails both conditions.
    """
    shared = git("merge-base", parent, source_ref, root=root, check_call=False)
    if shared:
        raise SystemExit(f"refused: the snapshot branch {branch} shares history with the source (commit {shared[:7]}); "
                         "a push would send development commits. Delete the branch after review, or start a new "
                         "history with --new-root")
    expected = "{} <{}>".format(*IDENTITY)
    # The parents are not the last field: the last line of the output loses a trailing tab.
    lines = git("log", "--no-show-signature", "--format=%H%x09%P%x09%an <%ae>%x09%cn <%ce>", parent, root=root)
    for line in lines.splitlines():
        fields = line.split("\t")
        if len(fields) != 4 or fields[2] != expected or fields[3] != expected:
            raise SystemExit(f"refused: the commit {fields[0][:7]} in the chain of {branch} does not carry the snapshot "
                             "identity; it is no snapshot of this command. Start a new history with --new-root")
        if len(fields[1].split()) > 1:
            raise SystemExit(f"refused: the commit {fields[0][:7]} in the chain of {branch} has more than one parent; "
                             "a snapshot chain holds no merge. Start a new history with --new-root")


def remote_urls(root, remote):
    """The fetch and push URLs of `remote`, or None when the remote is not configured."""
    if remote not in git("remote", root=root).split():
        return None
    urls = git("remote", "get-url", "--all", remote, root=root).split()
    urls += git("remote", "get-url", "--push", "--all", remote, root=root).split()
    return list(dict.fromkeys(urls))


def check_remote(root, remote):
    """The URLs of `remote`; refuse a URL that holds userinfo in an HTTP URL. None: no such remote."""
    urls = remote_urls(root, remote)
    if urls is not None and any(re.match(r"(?i)^https?://[^/@]*@", url) for url in urls):
        raise SystemExit(f"refused: a URL of the remote {remote} holds userinfo; use the Git credential helper")
    return urls


def check_push_target(urls):
    """Refuse a push unless each URL of the remote is the snapshot repository on GitHub."""
    for url in urls:
        match = GITHUB_URL.fullmatch(url)
        if match is None or f"{match.group(1)}/{match.group(2)}".lower() != REPOSITORY.lower():
            raise SystemExit(f"refused: a URL of the remote is not the GitHub repository {REPOSITORY}; "
                             f"a snapshot goes to that repository only (git remote add {DEFAULT_REMOTE} {REMOTE_URL})")


def gh_private(repository, which=shutil.which, run=subprocess.run):
    """True or False from `gh repo view --json isPrivate`; None when `gh` is absent or gives no answer."""
    gh = which("gh")
    if gh is None:
        return None
    result = run([gh, "repo", "view", repository, "--json", "isPrivate"], text=True, capture_output=True)
    try:
        private = json.loads(result.stdout)["isPrivate"] if result.returncode == 0 else None
    except (ValueError, KeyError, TypeError):
        return None
    return private if isinstance(private, bool) else None


def check_visibility(private):
    """Refuse a push unless the snapshot repository is known to be private."""
    if private is True:
        return f"visibility: {REPOSITORY} is private (gh)"
    if private is False:
        raise SystemExit(f"refused: the GitHub repository {REPOSITORY} is not private; snapshots stay private until "
                         "the owner signs off (docs/portable-release.md)")
    raise SystemExit(f"refused: the visibility of {REPOSITORY} is not verified (gh is absent, gives no answer, or the "
                     "repository does not exist); nothing is pushed")


# --- snapshot -------------------------------------------------------------------


def write_tree(root, entries):
    """The tree object of exactly `entries`, (mode, object, path) triples, written with a temporary index."""
    with tempfile.TemporaryDirectory(prefix="tenant-sheprd-portable-") as temp:
        env = dict(os.environ, GIT_INDEX_FILE=str(Path(temp) / "index"))
        git("read-tree", "--empty", root=root, env=env)
        records = b"".join(f"{mode} {oid}\t{path}\0".encode("utf-8") for mode, oid, path in entries)
        git("update-index", "-z", "--index-info", root=root, env=env, input=records, text=False)
        return git("write-tree", root=root, env=env)


def scan_snapshot(root, commit, remote, tag_object=None):
    """Scan a snapshot commit, the snapshot commits that `remote` lacks, and the tag; stop on a finding.

    The commits that no remote-tracking ref of `remote` contains are the commits that the next
    push sends, as current as the last fetch or push. Without such a ref it is the whole chain.
    A missing local deny list stops the scan with exit status 2 (scan.load_rules).
    """
    rules = scan.load_rules(root)
    commits = git("rev-list", commit, "--not", f"--remotes={remote}", root=root).split()
    if commit not in commits:
        commits.insert(0, commit)
    findings = collections.Counter()
    count = scan.scan_commits(root, rules, commits, findings)
    if tag_object is not None:
        scan.scan_object(root, rules, tag_object, "tag", findings)
    if findings:
        raise SystemExit("\n".join([*scan.finding_lines(findings), f"check failed: scan of the snapshot {commit[:7]}; no ref is moved",
                                    scan.summary(findings, count, rules)]))
    return f"check ok: {scan.summary(findings, count, rules)}; {len(commits)} snapshot commits"


def snapshot(root, entries, source_ref, branch, *, message, new_root=False, tag=None, verify=None):
    """Write a commit on `branch` whose tree is exactly `entries` (mode, object, path) of `source_ref`.

    Returns (commit, tree, changed). Uses a temporary index; the working tree is untouched.
    With `tag`, the branch and annotated tag refs update in one transaction, or neither updates.
    An existing tag with that name stops the command before an object is written.
    With `new_root` the commit has no parent, also when its tree equals the previous snapshot.
    Without `new_root`, a previous snapshot that is not a chain of snapshot commits stops the command.
    `verify(commit, tag object or None)` runs before a ref moves; it stops the command to refuse.
    """
    check_snapshot_branch(root, branch)
    if tag is not None:
        git("check-ref-format", f"refs/tags/{tag}", root=root)
    tree = write_tree(root, entries)
    parent = git("rev-parse", "--verify", "--quiet", f"refs/heads/{branch}^{{commit}}", root=root, check_call=False)
    if parent and not new_root:
        check_parent_chain(root, parent, source_ref, branch)
        if git("rev-parse", f"{parent}^{{tree}}", root=root) == tree:
            if verify is not None:
                verify(parent, None)
            return parent, tree, False
    if tag is not None and git("rev-parse", "--verify", "--quiet", f"refs/tags/{tag}", root=root, check_call=False):
        raise SystemExit(f"refused: the tag {tag} exists; a snapshot tag never moves")
    args = ["commit-tree", tree, "-m", message] + (["-p", parent] if parent and not new_root else [])
    commit = git(*NO_SIGNATURE, *args, root=root, env=identity_env())
    # Create objects first, then publish both refs atomically with an expected old branch value.
    tag_object, tag_update = None, ""
    if tag is not None:
        tagger = git("var", "GIT_COMMITTER_IDENT", root=root, env=identity_env())
        tag_object = git("mktag", root=root,
                         input=f"object {commit}\ntype commit\ntag {tag}\ntagger {tagger}\n\n{message}\n")
        tag_update = f"create refs/tags/{tag} {tag_object}\n"
    if verify is not None:
        verify(commit, tag_object)
    branch_update = (f"update refs/heads/{branch} {commit} {parent}\n" if parent
                     else f"create refs/heads/{branch} {commit}\n")
    check_snapshot_branch(root, branch)
    git("update-ref", "--stdin", root=root, input="start\n" + branch_update + tag_update + "prepare\ncommit\n")
    return commit, tree, True


def snapshot_tags(root, commit, rules):
    """(name, tag object) of each tag `portable/<yyyymmdd>-<source sha>` that points at `commit`, sorted.

    A tag with another name on the same commit is not a snapshot tag, and the push does not send it.
    A tag with a snapshot name goes to the remote, also a tag that a person or an earlier run made.
    So each one must be an annotated tag with the neutral identity as tagger and no signature, and
    its object is scanned with the current rules. Another tag stops the command.
    """
    expected = "{} <{}>".format(*IDENTITY)
    tags, findings = [], collections.Counter()
    for name in sorted(git("tag", "--points-at", commit, "--list", "portable/*", root=root).split()):
        if SNAPSHOT_TAG.fullmatch(name) is None:
            continue
        kind, _, rest = git("for-each-ref", "--format=%(objecttype)%09%(objectname)%09%(taggername) %(taggeremail)",
                            f"refs/tags/{name}", root=root).partition("\t")
        oid, _, tagger = rest.partition("\t")
        if kind != "tag" or tagger != expected or b"-----BEGIN" in git("cat-file", "tag", oid, root=root, text=False):
            raise SystemExit(f"refused: the tag {name} is not an annotated, unsigned tag with the snapshot identity; "
                             "it is no tag of this command, and nothing is pushed. Delete it after review")
        scan.scan_object(root, rules, oid, f"tag:{name}", findings)
        tags.append((name, oid))
    if findings:
        raise SystemExit("\n".join([*scan.finding_lines(findings),
                                    f"check failed: scan of the tags of the snapshot {commit[:7]}; nothing is pushed"]))
    return tags


def identities(root, commit, tags):
    """One line for the author and the committer of `commit` and for the tagger of each tag."""
    lines = [f"author     {git('log', '-1', '--format=%an <%ae>', commit, root=root)}",
             f"committer  {git('log', '-1', '--format=%cn <%ce>', commit, root=root)}"]
    for tag in tags:
        tagger = git("for-each-ref", "--format=%(taggername) %(taggeremail)", f"refs/tags/{tag}", root=root)
        lines.append(f"tagger     {tagger}  ({tag})")
    return lines


# --- push -----------------------------------------------------------------------


def push_refspecs(branch, remote_branch, tags=()):
    """The explicit refspecs of the push: the snapshot branch and each tag of this snapshot, no other ref."""
    return [f"refs/heads/{branch}:refs/heads/{remote_branch}", *(f"{oid}:refs/tags/{tag}" for tag, oid in tags)]


def push_command(remote, branch, remote_branch, tags=(), *, dry_run=False):
    """Arguments of the push: explicit refspecs only, no option that follows tags, no force.

    `tags` holds (name, tag object) of the `portable/*` tags that point at the snapshot commit
    (snapshot_tags). The source of a tag refspec is the object that the scan read, not the name.
    After a build without a push the next run makes no new tag, and the tag of the earlier build
    is in this list.
    `push.followTags` of the configuration is switched off for this one command.
    `--atomic`: the remote updates the branch and each tag together, or no ref.
    """
    command = ["-c", "push.followTags=false", "push", "--atomic", "--no-follow-tags"]
    if dry_run:
        command.append("--dry-run")
    return command + [remote, *push_refspecs(branch, remote_branch, tags)]


def remote_refs(root, remote):
    """{ref: object} of the remote, from `git ls-remote`. This contacts the remote and changes nothing."""
    refs = {}
    for line in git("ls-remote", remote, root=root).splitlines():
        value, _, ref = line.partition("\t")
        refs[ref] = value
    return refs


def foreign_refs(refs, remote_branch):
    """The refs of the remote that are not the snapshot branch, a snapshot tag or a `refs/pull/*` ref."""
    return sorted(ref for ref in refs
                  if ref not in ("HEAD", f"refs/heads/{remote_branch}")
                  and not ref.startswith(("refs/tags/portable/", "refs/pull/")))


def push_plan(remote, urls, branch, remote_branch, tags, commit, old):
    """The lines of the push plan: remote, URL, each ref, the old and the new commit."""
    lines = [f"push plan: remote {remote}" + (f" ({', '.join(urls)})" if urls else f" (not configured; {REMOTE_URL})"),
             f"  refs/heads/{branch} -> refs/heads/{remote_branch}  {old} -> {commit}"]
    lines += [f"  refs/tags/{tag} -> refs/tags/{tag}  new" for tag in tags]
    return lines


def main(argv=None, *, root=ROOT, private=gh_private):
    argv = sys.argv[1:] if argv is None else list(argv)
    # The parser knows the six options below only and takes no abbreviation. So argparse itself stops with
    # exit status 2 on --force, -f, --force-with-lease, --all, --mirror, --tags, --follow-tags, --prune and
    # --delete: no option of this command sends a ref that it does not name, or forces a ref.
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter,
                                     allow_abbrev=False)
    parser.add_argument("--remote", default=DEFAULT_REMOTE,
                        help=f"git remote that receives the snapshot (default {DEFAULT_REMOTE})")
    parser.add_argument("--remote-branch", default="main", help="branch name on the remote (default main)")
    parser.add_argument("--branch", default="portable", help="local snapshot branch: portable, portable-* or portable/*")
    parser.add_argument("--source-branch", default="local-dev",
                        help="development branch that HEAD must be on (default local-dev)")
    parser.add_argument("--push", action="store_true",
                        help="push the snapshot branch and its tags after the plan and a push dry run")
    parser.add_argument("--new-root", action="store_true",
                        help="start a new history: the snapshot commit has no parent; not with --push")
    args = parser.parse_args(argv)
    if args.push and args.new_root:
        raise SystemExit("refused: --new-root with --push; the push of a new history is a separate, manual step")
    if REMOTE_NAME.fullmatch(args.remote) is None:
        raise SystemExit("refused: the remote name holds a character other than a letter, a digit, '.', '_' or '-'")
    git("check-ref-format", f"refs/heads/{args.remote_branch}", root=root)
    check_repository(root, args.source_branch)
    check_snapshot_branch(root, args.branch)
    urls = check_remote(root, args.remote)
    if args.push:
        # The conditions of a push that need no snapshot, before a Git object is written.
        if urls is None:
            raise SystemExit(f"refused: the remote {args.remote} is not configured "
                             f"(git remote add {args.remote} {REMOTE_URL})")
        check_push_target(urls)
        print(check_visibility(private(REPOSITORY)))
    # Fail closed before a Git object is written: no local deny list, no snapshot.
    rules = scan.load_rules(root)
    head = git("rev-parse", "HEAD", root=root)
    short = head[:7]
    entries = publish_entries(root, head, label=lambda path: scan.path_label(rules, path))
    print(f"check ok: publish set ({len(entries)} files)")

    def verify(commit, tag_object):
        print(scan_snapshot(root, commit, args.remote, tag_object), flush=True)

    date = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%d")
    message = (f"Portable snapshot of {short} ({date})\n\n"
               f"Exactly the publish set of scripts/portable/publish_check.py at\n"
               f"source commit {head}.\n")
    previous = git("rev-parse", "--verify", "--quiet", f"refs/heads/{args.branch}^{{commit}}", root=root,
                   check_call=False)
    tag = f"portable/{date.replace('-', '')}-{short}"
    commit, tree, changed = snapshot(root, entries, head, args.branch, message=message, new_root=args.new_root,
                                     tag=tag, verify=verify)
    if args.new_root and previous:
        print(f"new root: {args.branch} was {previous}; earlier local tags and other refs stay unchanged")
    if changed:
        print(f"portable commit {commit[:7]} (tree {tree[:7]}), tag {tag}")
    else:
        print(f"no change: the branch {args.branch} already holds tree {tree[:7]} (commit {commit[:7]})")
    tags = snapshot_tags(root, commit, rules)
    names = [name for name, _ in tags]
    print("\n".join(identities(root, commit, names)))

    if not args.push:
        tracking = git("rev-parse", "--verify", "--quiet", f"refs/remotes/{args.remote}/{args.remote_branch}",
                       root=root, check_call=False)
        old = f"{tracking[:7]} (last fetch)" if tracking else "not read"
        print("\n".join(push_plan(args.remote, urls, args.branch, args.remote_branch, names, commit[:7], old)))
        print("no push: the remote is not contacted and not changed; --push sends this plan")
        return 0

    before = remote_refs(root, args.remote)
    foreign = foreign_refs(before, args.remote_branch)
    old = before.get(f"refs/heads/{args.remote_branch}", "")
    print("\n".join(push_plan(args.remote, urls, args.branch, args.remote_branch,
                              [name for name in names if f"refs/tags/{name}" not in before], commit[:7],
                              old[:7] if old else "absent")))
    if foreign:
        raise SystemExit("refused: the remote holds refs that are not snapshot refs; nothing is pushed or deleted:\n"
                         + "\n".join(f"  {ref}" for ref in foreign))
    # A refused push stops the command: with named refs the remote can accept one ref and refuse another.
    git(*push_command(args.remote, args.branch, args.remote_branch, tags, dry_run=True), root=root)
    print("push dry run ok")
    git(*push_command(args.remote, args.branch, args.remote_branch, tags), root=root)
    print(f"pushed {args.branch} to {args.remote}/{args.remote_branch}" + "".join(f", tag {name}" for name in names))
    after = remote_refs(root, args.remote)
    print("remote refs after the push:")
    print("\n".join(f"  {value}  {ref}" for ref, value in sorted(after.items())))
    if after.get(f"refs/heads/{args.remote_branch}") != commit:
        raise SystemExit(f"the remote branch {args.remote_branch} does not hold {commit}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
