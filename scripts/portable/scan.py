#!/usr/bin/env python3
"""Scan Git objects for host values and secrets before a portable snapshot leaves this machine.

Usage: scan.py [--report FILE] [--hashes] <mode>
  publish-set [<commit>]  the files of the publish set of <commit> (default HEAD) and their names
  tree <commit>           each file of <commit>, the file names, and the commit object itself
  history <rev>...        `tree` for each commit of `git rev-list <rev>...`
  staged                  the staged content and its file names

The scan reads Git objects only, never the working tree.

Rules:
  host-values.regex        tracked generic rules: private addresses and secret forms
  host-values.allow        tracked: <rule> <value> pairs that are no finding (documentation examples)
  host-values.allow-lines  tracked: <path><TAB><sha256 of one exact line>; a reviewed line
  .local/host-values.deny  NOT tracked: the literal values of this host, one for each line,
                           case-insensitive. No allow list drops a finding of this list.

The local deny list is required. Without it (absent, or with no pattern) the scan has no rule for
the values of this host, so it stops with exit status 2 and reports nothing as clean.
The list is `.local/host-values.deny` of this checkout, or of the main checkout for a linked
worktree, or the file that SHEPRD_PORTABLE_DENY names.

Output: one line `<rule><TAB><path>:<line><TAB><count>` for each finding. The output never holds
the matched text or a pattern of the local deny list: a local rule is `local-<line of the list>`.
Line 0 is the file name. A file name with a finding prints as `name:<hash>` in each finding of
that file, also in a finding of its content.

A deny literal is searched as UTF-8, UTF-16-LE and UTF-16-BE, in any ASCII letter case. The scan
does not decode: it does not find a value in an encoded form (base64, compressed, URL-encoded,
escaped), in an image or in an archive.

Exit: 0 clean, 1 finding, 2 usage or tool error.
"""
import argparse
import collections
import hashlib
import os
import re
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
if __name__ == "__main__":
    sys.dont_write_bytecode = True
sys.path.insert(0, str(HERE))

from publish_check import git, publish_entries, tree_entries  # noqa: E402

REGEX_FILE = HERE / "host-values.regex"
ALLOW_FILE = HERE / "host-values.allow"
ALLOW_LINES_FILE = HERE / "host-values.allow-lines"
LOCAL_DENY_REL = ".local/host-values.deny"
LOCAL_DENY_VARIABLE = "SHEPRD_PORTABLE_DENY"

Rules = collections.namedtuple("Rules", "local generic allow allow_lines deny")
LINE_HASH = re.compile(r"[0-9a-f]{64}")


def die(message):
    """Stop with exit status 2: a tool error is never a clean scan."""
    print(f"scan: {message}", file=sys.stderr)
    raise SystemExit(2)


def list_lines(path):
    """(line number, text) of each entry of a list file, without comments and empty lines."""
    try:
        text = Path(path).read_text(encoding="utf-8")
    except (OSError, UnicodeError):
        die(f"cannot read the rule file {Path(path).name}")
    return [(number, line.strip()) for number, line in enumerate(text.splitlines(), 1)
            if line.strip() and not line.strip().startswith("#")]


def local_deny_file(root, environ=None):
    """The path of the local deny list, or None when no file exists."""
    environ = os.environ if environ is None else environ
    named = environ.get(LOCAL_DENY_VARIABLE)
    if named:
        return Path(named) if Path(named).is_file() else None
    own = Path(root) / LOCAL_DENY_REL
    if own.is_file():
        return own
    # A linked worktree has no .local/ directory: use the list of the main checkout.
    common = subprocess.run(["git", "rev-parse", "--path-format=absolute", "--git-common-dir"], cwd=root,
                            text=True, capture_output=True)
    main = Path(common.stdout.strip()) if common.returncode == 0 and common.stdout.strip() else None
    if main is not None and main.name == ".git" and (main.parent / LOCAL_DENY_REL).is_file():
        return main.parent / LOCAL_DENY_REL
    return None


def load_rules(root, environ=None):
    """The rules of a scan; stop when the local deny list is absent or holds no pattern."""
    deny = local_deny_file(root, environ)
    if deny is None:
        die(f"no local deny list ({LOCAL_DENY_REL} or {LOCAL_DENY_VARIABLE}); without it no rule knows the "
            "values of this host, so nothing is scanned (docs/portable-release.md)")
    local = []
    for number, literal in list_lines(deny):
        low = literal.lower()
        # The UTF-16 forms find the value in the wide text of a binary file, in each byte order.
        local.append((f"local-{number}", tuple(low.encode(form) for form in ("utf-8", "utf-16-le", "utf-16-be"))))
    if not local:
        die("the local deny list holds no pattern; nothing is scanned")
    generic = []
    for number, line in list_lines(REGEX_FILE):
        name, _, pattern = line.partition(" ")
        try:
            generic.append((name, re.compile(pattern.strip().encode("utf-8"))))
        except re.error:
            die(f"{REGEX_FILE.name}:{number}: not a regular expression")
    if not generic:
        die(f"{REGEX_FILE.name} holds no rule")
    # A malformed allow entry allows nothing, so a typing error would look like a rule that works: stop.
    names = {name for name, _ in generic}
    allow, allow_lines = set(), set()
    for number, line in list_lines(ALLOW_FILE):
        entry = tuple(line.split(None, 1))
        if len(entry) != 2 or entry[0] not in names:
            die(f"{ALLOW_FILE.name}:{number}: not `<rule> <value>` with a rule of {REGEX_FILE.name}")
        allow.add(entry)
    for number, line in list_lines(ALLOW_LINES_FILE):
        entry = tuple(line.split("\t"))
        if len(entry) != 2 or LINE_HASH.fullmatch(entry[1]) is None:
            die(f"{ALLOW_LINES_FILE.name}:{number}: not `<path><TAB><sha256>`")
        allow_lines.add(entry)
    return Rules(local, generic, allow, allow_lines, deny)


def line_at(data, offset):
    """(line number, line bytes without the line end) of the byte `offset` of `data`."""
    start = data.rfind(b"\n", 0, offset) + 1
    end = data.find(b"\n", offset)
    return data.count(b"\n", 0, offset) + 1, data[start:len(data) if end == -1 else end]


def scan_bytes(rules, path, data, *, name_only=False):
    """(rule, line number) for each finding in `data`, the content of `path`.

    A local deny pattern is a literal in any letter case. A generic rule gives the value of its
    group 1; the value is dropped when the allow list names it, or when the allowed lines hold
    the path and the hash of the line. With `name_only` the line number is 0.
    """
    found = []
    low = data.lower()
    for rule, forms in rules.local:
        for form in forms:
            offset = low.find(form)
            while offset != -1:
                found.append((rule, 0 if name_only else line_at(data, offset)[0]))
                offset = low.find(form, offset + 1)
    for rule, regex in rules.generic:
        for match in regex.finditer(data):
            group = 1 if regex.groups else 0
            if (rule, match.group(group).decode("utf-8", "replace")) in rules.allow:
                continue
            number, line = line_at(data, match.start(group))
            if (path, hashlib.sha256(line).hexdigest()) in rules.allow_lines:
                continue
            found.append((rule, 0 if name_only else number))
    return found


def path_label(rules, path):
    """`path` as the output may show it: `name:<hash>` when the name itself holds a finding."""
    name = path.encode("utf-8", "replace")
    if scan_bytes(rules, path, name, name_only=True):
        return "name:" + hashlib.sha256(name).hexdigest()[:12]
    return path


def read_blobs(root, oids):
    """Yield (object id, content) for each id of `oids`, with one Git process."""
    process = subprocess.Popen(["git", "cat-file", "--batch"], cwd=root, stdin=subprocess.PIPE, stdout=subprocess.PIPE)
    try:
        for oid in oids:
            process.stdin.write(oid.encode("ascii") + b"\n")
            process.stdin.flush()
            header = process.stdout.readline().split()
            if len(header) != 3:
                die(f"cannot read the object {oid}")
            data = process.stdout.read(int(header[2]))
            process.stdout.read(1)
            yield oid, data
    finally:
        process.stdin.close()
        process.stdout.close()
        process.wait()


def scan_entries(root, rules, entries, findings, seen=None):
    """Add the findings of the files `entries`, (object id, path) pairs, to the counter `findings`."""
    seen = set() if seen is None else seen
    entries = [entry for entry in entries if entry not in seen]
    seen.update(entries)
    paths = collections.defaultdict(list)
    for oid, path in entries:
        # A finding of the content of a file is keyed by the label, so that no output holds a name with a finding.
        label = path_label(rules, path)
        paths[oid].append((path, label))
        for rule, _ in scan_bytes(rules, path, path.encode("utf-8", "replace"), name_only=True):
            findings[(rule, label, 0)] += 1
    for oid, data in read_blobs(root, list(paths)):
        for path, label in paths[oid]:
            for rule, number in scan_bytes(rules, path, data):
                findings[(rule, label, number)] += 1
    return len(entries)


def scan_object(root, rules, oid, label, findings):
    """Add the findings of a commit or tag object: its message and its identities."""
    data = git("cat-file", "-p", oid, root=root, text=False)
    for rule, number in scan_bytes(rules, label, data):
        findings[(rule, label, number)] += 1


def scan_commits(root, rules, commits, findings):
    """Add the findings of each commit of `commits`: the commit object and each file of its tree."""
    seen, count = set(), 0
    for commit in commits:
        scan_object(root, rules, commit, f"commit:{commit[:7]}", findings)
        count += scan_entries(root, rules, [(oid, path) for _, _, oid, path in tree_entries(root, commit)],
                              findings, seen)
    return count


def staged_entries(root):
    """(object id, path) of each staged file that differs from HEAD."""
    names = [name for name in git("diff", "--cached", "--name-only", "-z", "--diff-filter=ACMRT", root=root,
                                  text=False).split(b"\0") if name]
    if not names:
        return []
    listed = git("ls-files", "-s", "-z", "--", *[name.decode("utf-8", "surrogateescape") for name in names],
                 root=root, text=False)
    entries = []
    for record in listed.split(b"\0"):
        if record:
            meta, _, path = record.partition(b"\t")
            entries.append((meta.split()[1].decode("ascii"), path.decode("utf-8", "replace")))
    return entries


def scan(root, mode, arguments=(), environ=None):
    """(findings, number of files) of one scan. `findings` counts (rule, path, line) triples."""
    rules = load_rules(root, environ)
    findings = collections.Counter()
    if mode == "publish-set":
        ref = arguments[0] if arguments else "HEAD"
        entries = publish_entries(root, ref, label=lambda path: path_label(rules, path))
        count = scan_entries(root, rules, [(oid, path) for _, oid, path in entries], findings)
    elif mode == "tree":
        commit = git("rev-parse", "--verify", f"{arguments[0]}^{{commit}}", root=root)
        count = scan_commits(root, rules, [commit], findings)
    elif mode == "history":
        if any(argument.startswith("-") and argument != "--not" and not argument.startswith("--remotes=")
               for argument in arguments):
            die("history takes revisions, --not and --remotes=<remote> only")
        count = scan_commits(root, rules, git("rev-list", *arguments, root=root).split(), findings)
    elif mode == "staged":
        count = scan_entries(root, rules, staged_entries(root), findings)
    else:
        die(f"unknown mode {mode}")
    return findings, count, rules


def summary(findings, count, rules):
    state = "clean" if not findings else (f"{sum(findings.values())} findings in "
                                          f"{len({path for _, path, _ in findings})} files")
    # The path of the list shows which list ran: a file with one pattern also passes the fail-closed check.
    return (f"scan: {state}; {count} files read, {len(rules.local)} local deny patterns, "
            f"{len(rules.generic)} generic rules; deny list {rules.deny}")


def finding_lines(findings):
    """One line `<rule><TAB><path>:<line><TAB><count>` for each finding, sorted by path and line."""
    return [f"{rule}\t{path}:{number}\t{findings[(rule, path, number)]}"
            for rule, path, number in sorted(findings, key=lambda key: (key[1], key[2], key[0]))]


def report(findings, count, rules, title):
    """A Markdown report: rule, file, line and count of each finding, never the matched text."""
    lines = [f"# Scan report: {title}", "", summary(findings, count, rules).removeprefix("scan: "), "",
             "A rule `local-<n>` is line n of the local deny list. Line 0 is the file name.", "",
             "| Rule | File | Line | Count |", "| --- | --- | --- | --- |"]
    lines += [f"| {rule} | {path} | {number} | {findings[(rule, path, number)]} |"
              for rule, path, number in sorted(findings, key=lambda key: (key[1], key[2], key[0]))]
    by_rule = collections.Counter()
    for (rule, _, _), number in findings.items():
        by_rule[rule] += number
    lines += ["", "| Rule | Findings |", "| --- | --- |", *[f"| {rule} | {by_rule[rule]} |" for rule in sorted(by_rule)]]
    return "\n".join(lines) + "\n"


def line_hashes(root, findings, ref):
    """`<path><TAB><sha256>` for each line with a finding: the form of host-values.allow-lines."""
    lines = set()
    for _, path, number in findings:
        if number and not path.startswith(("commit:", "name:", "tag:")):
            data = git("show", f"{ref}:{path}", root=root, text=False)
            line = data.split(b"\n")[number - 1]
            lines.add(f"{path}\t{hashlib.sha256(line).hexdigest()}")
    return sorted(lines)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--report", type=Path, help="also write a Markdown report to this file")
    parser.add_argument("--hashes", action="store_true",
                        help="with publish-set or tree: print the allow-lines form of each line with a finding")
    parser.add_argument("mode", choices=("publish-set", "tree", "history", "staged"))
    parser.add_argument("arguments", nargs="*")
    args = parser.parse_args(argv)
    if (args.mode == "tree" and len(args.arguments) != 1) or (args.mode == "publish-set" and len(args.arguments) > 1) \
            or (args.mode == "staged" and args.arguments) or (args.mode == "history" and not args.arguments):
        parser.error(f"wrong arguments for the mode {args.mode}")
    if args.hashes and args.mode not in ("publish-set", "tree"):
        parser.error("--hashes needs the mode publish-set or tree")
    findings, count, rules = scan(ROOT, args.mode, args.arguments)
    if args.hashes:
        print("\n".join(line_hashes(ROOT, findings, args.arguments[0] if args.arguments else "HEAD")))
    else:
        print("\n".join(finding_lines(findings)), end="\n" if findings else "")
    if args.report is not None:
        args.report.write_text(report(findings, count, rules, " ".join([args.mode, *args.arguments])), encoding="utf-8")
    print(summary(findings, count, rules), file=sys.stderr if args.hashes else sys.stdout)
    return 1 if findings else 0


if __name__ == "__main__":
    sys.exit(main())
