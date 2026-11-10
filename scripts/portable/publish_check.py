#!/usr/bin/env python3
"""The publish set of a portable snapshot: the tracked files of a commit minus the dev-only list.

The set is read from a commit, never from the working tree. `scripts/portable/dev-only-files.txt`
of that commit names the files that stay in the private copy. The list excludes itself.

Usage: publish_check.py [--ref <commit>] [--list]

The command needs the local deny list of scripts/portable/scan.py: a refusal names a path, and
a path that holds a finding of the scan prints as `name:<hash>`.
"""
import argparse
import posixpath
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DEV_ONLY_REL = "scripts/portable/dev-only-files.txt"
REGULAR_MODES = ("100644", "100755")
SYMLINK_MODE = "120000"


def git(*args, root=ROOT, env=None, check_call=True, input=None, text=True):
    """The output of one Git command; stop with its error text when it fails."""
    result = subprocess.run(["git", *args], cwd=root, env=env, text=text, capture_output=True, input=input)
    if check_call and result.returncode != 0:
        error = result.stderr if text else result.stderr.decode("utf-8", "replace")
        raise SystemExit(f"git {' '.join(args)}: {error.strip()}")
    return result.stdout.strip() if text else result.stdout


def tree_entries(root, ref):
    """(mode, kind, object, path) for each file of the commit `ref`."""
    entries = []
    for record in git("ls-tree", "-r", "-z", "--full-tree", ref, root=root, text=False).split(b"\0"):
        if not record:
            continue
        meta, _, path = record.partition(b"\t")
        mode, kind, oid = meta.decode("ascii").split()
        try:
            entries.append((mode, kind, oid, path.decode("utf-8")))
        except UnicodeError:
            raise SystemExit(f"refused: a path of {ref} is not UTF-8") from None
    return entries


def dev_only(root, ref):
    """The entries of the dev-only list of `ref`: exact paths, or directory prefixes that end in `/`.

    The list must exist as a regular file: without it the publish set would be each tracked file.
    """
    listed = git("ls-tree", ref, "--", DEV_ONLY_REL, root=root)
    if not listed.startswith(tuple(f"{mode} blob " for mode in REGULAR_MODES)):
        raise SystemExit(f"refused: {DEV_ONLY_REL} is not a regular file of {ref}; no publish set without the list")
    entries = []
    for line in git("show", f"{ref}:{DEV_ONLY_REL}", root=root).splitlines():
        entry = line.strip()
        if not entry or entry.startswith("#"):
            continue
        parts = entry.removesuffix("/").split("/")
        if (any(c.isspace() or ord(c) < 32 for c in entry) or "#" in entry or "\\" in entry
                or any(part in ("", ".", "..") for part in parts)):
            raise SystemExit(f"refused: {DEV_ONLY_REL} holds an entry that is not a repository path")
        entries.append(entry)
    return entries


def is_excluded(path, entries):
    """True for an exact path or a path below a directory prefix, never a sibling with a similar name."""
    return any(path.startswith(entry) if entry.endswith("/") else path == entry for entry in entries)


def publish_entries(root, ref, label=str):
    """(mode, object, path) of each file of the publish set of `ref`, sorted by path.

    `label(path)` is the text of a path in a refusal. A caller with scan rules passes
    `scan.path_label`, so that a refusal never prints a file name with a finding.

    Stops on a dev-only entry that names no tracked file (a stale or mistyped entry would publish
    the file that it meant), on a submodule, and on a symlink that does not point at a file of
    the publish set.
    """
    entries = tree_entries(root, ref)
    paths = [path for _, _, _, path in entries]
    excludes = dev_only(root, ref)
    for entry in excludes:
        if not entry.endswith("/") and any(path.startswith(entry + "/") for path in paths):
            raise SystemExit(f"refused: the dev-only entry {label(entry)} is a directory; write it with a final /")
        if not any(is_excluded(path, (entry,)) for path in paths):
            raise SystemExit(f"refused: the dev-only entry {label(entry)} names no tracked file of {ref}")
    kept = [(mode, kind, oid, path) for mode, kind, oid, path in entries
            if not is_excluded(path, (*excludes, DEV_ONLY_REL))]
    names = {path for _, _, _, path in kept}
    for mode, kind, oid, path in kept:
        if kind != "blob" or mode not in (*REGULAR_MODES, SYMLINK_MODE):
            raise SystemExit(f"refused: {label(path)} is not a file or a symlink in {ref}")
        if mode == SYMLINK_MODE:
            target = git("cat-file", "blob", oid, root=root, text=False).decode("utf-8", "replace")
            resolved = posixpath.normpath(posixpath.join(posixpath.dirname(path), target))
            if target.startswith("/") or resolved not in names:
                raise SystemExit(f"refused: the symlink {label(path)} does not point at a file of the publish set")
    return sorted(((mode, oid, path) for mode, _, oid, path in kept), key=lambda entry: entry[2])


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--ref", default="HEAD", help="commit to read (default HEAD)")
    parser.add_argument("--list", action="store_true", help="print each path of the publish set")
    args = parser.parse_args(argv)
    import scan  # Here, not at the top: scan.py imports this module.
    rules = scan.load_rules(ROOT)
    tracked = tree_entries(ROOT, args.ref)
    published = publish_entries(ROOT, args.ref, label=lambda path: scan.path_label(rules, path))
    if args.list:
        print("\n".join(path for _, _, path in published))
    print(f"publish set valid: {len(published)} of {len(tracked)} tracked files, "
          f"{len(tracked) - len(published)} dev-only (not a release approval)", file=sys.stderr if args.list else sys.stdout)
    return 0


if __name__ == "__main__":
    sys.dont_write_bytecode = True
    sys.exit(main())
