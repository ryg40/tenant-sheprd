"""Tests of the portable snapshot tooling, in temporary repositories without network.

Each test covers a behavior whose failure leaks data or skips a refusal.
Run: python3 -m unittest discover -s scripts/portable/tests
"""
import contextlib
import io
import os
import subprocess
import sys
import tempfile
import types
import unittest
from pathlib import Path
from unittest import mock

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import publish_check  # noqa: E402
import publish_portable  # noqa: E402
import scan  # noqa: E402

# A synthetic host value: the local deny list of a test holds it, no real list does.
DENY_VALUE = "canary-host.portable-test.invalid"


def run_git(root, *args):
    return subprocess.run(["git", *args], cwd=root, text=True, capture_output=True, check=True).stdout.strip()


class PortableCase(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(prefix="portable-test-")
        self.addCleanup(temp.cleanup)
        self.temp = Path(temp.name)
        self.deny = self.temp / "host-values.deny"
        self.deny.write_text(f"# test list\n{DENY_VALUE}\n", encoding="utf-8")
        # No configuration, identity or deny list of the host reaches a test repository.
        environ = {name: value for name, value in os.environ.items() if not name.startswith("GIT_")}
        environ.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_SYSTEM=os.devnull, GIT_AUTHOR_NAME="Dev",
                       GIT_AUTHOR_EMAIL="dev@source.invalid", GIT_COMMITTER_NAME="Dev",
                       GIT_COMMITTER_EMAIL="dev@source.invalid", **{scan.LOCAL_DENY_VARIABLE: str(self.deny)})
        patch = mock.patch.dict(os.environ, environ, clear=True)
        patch.start()
        self.addCleanup(patch.stop)
        self.root = self.temp / "source"
        self.root.mkdir()
        run_git(self.root, "init", "-q", "-b", "local-dev")
        self.write("README.md", "public text\n")
        self.write("notes/private.md", "stays in the private copy\n")
        self.write(publish_check.DEV_ONLY_REL, "notes/\n")
        self.commit("first")

    def write(self, name, text):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    def commit(self, subject):
        run_git(self.root, "add", "-A")
        run_git(self.root, "commit", "-q", "-m", subject)

    def publish(self, *argv, private=lambda repository: None):
        """Run the snapshot command; return (exit value or SystemExit, the text that it printed)."""
        output = io.StringIO()
        with contextlib.redirect_stdout(output), contextlib.redirect_stderr(output):
            try:
                result = publish_portable.main(list(argv), root=self.root, private=private)
            except SystemExit as stop:
                result = stop
                output.write(str(stop.code))
        return result, output.getvalue()

    def bare_remote(self, name="remote.git"):
        """A local bare repository as the snapshot remote. Only the check of the push target is replaced."""
        remote = self.temp / name
        run_git(self.temp, "init", "-q", "--bare", str(remote))
        if publish_portable.DEFAULT_REMOTE in run_git(self.root, "remote").split():
            run_git(self.root, "remote", "remove", publish_portable.DEFAULT_REMOTE)
        run_git(self.root, "remote", "add", publish_portable.DEFAULT_REMOTE, str(remote))
        patch = mock.patch.object(publish_portable, "check_push_target")
        patch.start()
        self.addCleanup(patch.stop)
        return remote

    def tree_paths(self, commit):
        return set(run_git(self.root, "ls-tree", "-r", "--name-only", commit).splitlines())


class SnapshotContentTests(PortableCase):
    def test_dev_only_file_never_reaches_the_snapshot(self):
        result, text = self.publish()
        self.assertEqual(result, 0, text)
        paths = self.tree_paths("portable")
        self.assertIn("README.md", paths)
        self.assertFalse({"notes/private.md", publish_check.DEV_ONLY_REL} & paths)

    def test_working_tree_changes_never_reach_the_snapshot(self):
        head = run_git(self.root, "rev-parse", "HEAD")
        self.write("untracked.txt", "not committed\n")
        self.write("README.md", "modified and not committed\n")
        self.write("staged.txt", "staged and not committed\n")
        run_git(self.root, "add", "staged.txt")
        # The snapshot function reads the commit only.
        commit, _, _ = publish_portable.snapshot(self.root, publish_check.publish_entries(self.root, head), head,
                                                 "portable", message="snapshot")
        self.assertFalse({"untracked.txt", "staged.txt"} & self.tree_paths(commit))
        self.assertEqual(run_git(self.root, "show", f"{commit}:README.md"), "public text")

    def test_stale_dev_only_entry_and_missing_list_stop(self):
        self.write(publish_check.DEV_ONLY_REL, "notes/\nmistyped/\n")
        self.commit("stale entry")
        with self.assertRaises(SystemExit):
            publish_check.publish_entries(self.root, "HEAD")
        run_git(self.root, "rm", "-q", publish_check.DEV_ONLY_REL)
        self.commit("no list")
        with self.assertRaises(SystemExit):
            publish_check.publish_entries(self.root, "HEAD")

    def test_snapshot_carries_no_identity_of_the_source(self):
        self.publish()
        identities = run_git(self.root, "log", "--format=%an %ae %cn %ce", "portable")
        self.assertNotIn("source.invalid", identities)
        shared = subprocess.run(["git", "merge-base", "portable", "local-dev"], cwd=self.root, capture_output=True)
        self.assertEqual((shared.returncode, shared.stdout), (1, b""))

    def test_signing_configuration_of_the_host_gives_no_signature_and_no_host_identity(self):
        config = self.temp / "host.gitconfig"
        config.write_text("[user]\n\tname = Host Owner\n\temail = owner@host.invalid\n\tsigningkey = HOSTKEY\n"
                          "[commit]\n\tgpgsign = true\n[tag]\n\tgpgsign = true\n\tforceSignAnnotated = true\n"
                          "[gpg]\n\tprogram = false\n", encoding="utf-8")
        for name in [name for name in os.environ if name.startswith(("GIT_AUTHOR_", "GIT_COMMITTER_"))]:
            del os.environ[name]
        os.environ["GIT_CONFIG_GLOBAL"] = str(config)
        result, text = self.publish()
        self.assertEqual(result, 0, text)
        tag = run_git(self.root, "for-each-ref", "--format=%(objectname)", "refs/tags")
        for kind, oid in (("commit", "portable"), ("tag", tag)):
            with self.subTest(kind=kind):
                raw = run_git(self.root, "cat-file", kind, oid)
                self.assertIn(publish_portable.IDENTITY[1], raw)
                for private in ("gpgsig", "-----BEGIN", "host.invalid", "Host Owner", "HOSTKEY"):
                    self.assertNotIn(private, raw)


class ScanTests(PortableCase):
    def test_scan_fails_closed_without_a_local_deny_list(self):
        for state in ("absent", "empty"):
            with self.subTest(state=state):
                if state == "absent":
                    self.deny.unlink()
                else:
                    self.deny.write_text("# no pattern\n", encoding="utf-8")
                with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as stop:
                    scan.scan(self.root, "tree", ["HEAD"])
                self.assertEqual(stop.exception.code, 2)
                result, _ = self.publish()
                self.assertIsInstance(result, SystemExit)
                self.assertEqual(run_git(self.root, "for-each-ref", "refs/heads/portable", "refs/tags"), "")

    def test_deny_value_in_the_snapshot_tree_stops_and_stays_out_of_the_output(self):
        cases = {"content": ("docs/setup.md", f"connect to {DENY_VALUE.upper()} first\n"),
                 "file name": (f"docs/{DENY_VALUE}.md", "text\n"),
                 "file name and content": (f"docs/{DENY_VALUE}.txt", f"host {DENY_VALUE}\n")}
        for case, (name, text) in cases.items():
            with self.subTest(case=case):
                self.write(name, text)
                self.commit(f"add {case}")
                result, output = self.publish()
                self.assertIsInstance(result, SystemExit)
                self.assertNotIn(DENY_VALUE, output.lower())
                self.assertEqual(run_git(self.root, "for-each-ref", "refs/heads/portable", "refs/tags"), "")
                findings, _, _ = scan.scan(self.root, "publish-set", ["HEAD"])
                self.assertTrue(findings)
                self.assertNotIn(DENY_VALUE, scan.report(findings, 0, scan.load_rules(self.root), "test").lower())
                run_git(self.root, "rm", "-q", name)
                self.commit(f"remove {case}")

    def test_allow_lists_drop_no_local_finding(self):
        rules = scan.load_rules(self.root)
        line = f"host {DENY_VALUE}".encode()
        digest = scan.hashlib.sha256(line).hexdigest()
        rules = rules._replace(allow={("local-2", DENY_VALUE)}, allow_lines={("a.md", digest)})
        self.assertTrue(scan.scan_bytes(rules, "a.md", line))

    def test_earlier_snapshot_commit_with_a_deny_value_stops_a_later_snapshot(self):
        self.publish()
        clean = run_git(self.root, "rev-parse", "portable")
        # A snapshot commit that no scan saw, with the neutral identity, then a clean tree again.
        self.write("leak.md", f"{DENY_VALUE}\n")
        self.commit("leak")
        head = run_git(self.root, "rev-parse", "HEAD")
        publish_portable.snapshot(self.root, publish_check.publish_entries(self.root, head), head, "portable",
                                  message="snapshot", verify=None)
        self.assertNotEqual(run_git(self.root, "rev-parse", "portable"), clean)
        run_git(self.root, "rm", "-q", "leak.md")
        self.write("more.md", "public\n")
        self.commit("clean again")
        before = run_git(self.root, "rev-parse", "portable")
        result, _ = self.publish()
        self.assertIsInstance(result, SystemExit)
        self.assertEqual(run_git(self.root, "rev-parse", "portable"), before)


class PushRefusalTests(PortableCase):
    def test_push_refuses_a_repository_that_is_not_private(self):
        remote = self.bare_remote()
        for answer in (False, None):
            with self.subTest(private=answer):
                result, _ = self.publish("--push", private=lambda repository, answer=answer: answer)
                self.assertIsInstance(result, SystemExit)
                self.assertEqual(run_git(remote, "for-each-ref"), "")

    def test_only_a_json_true_of_gh_is_private(self):
        def answer(returncode, stdout):
            return publish_portable.gh_private("owner/name", which=lambda name: "gh",
                                               run=lambda *args, **kwargs: types.SimpleNamespace(
                                                   returncode=returncode, stdout=stdout, stderr=""))

        self.assertIs(answer(0, '{"isPrivate": true}'), True)
        for returncode, stdout in ((0, '{"isPrivate": false}'), (0, '{"isPrivate": "true"}'), (0, '{"isPrivate": 1}'),
                                   (0, "{}"), (0, "true"), (0, ""), (0, "not json"), (1, '{"isPrivate": true}')):
            with self.subTest(returncode=returncode, stdout=stdout):
                self.assertIsNot(answer(returncode, stdout), True)
        self.assertIsNot(publish_portable.gh_private("owner/name", which=lambda name: None), True)

    def test_push_sends_the_snapshot_and_its_tag_and_no_development_commit(self):
        remote = self.bare_remote()
        result, text = self.publish("--push", private=lambda repository: True)
        self.assertEqual(result, 0, text)
        snapshot = run_git(self.root, "rev-parse", "portable")
        refs = dict(line.split()[::-1] for line in run_git(remote, "for-each-ref",
                                                           "--format=%(objectname) %(refname)").splitlines())
        self.assertEqual(refs.pop("refs/heads/main"), snapshot)
        self.assertTrue(refs)
        for ref, value in refs.items():
            self.assertTrue(ref.startswith("refs/tags/portable/"), ref)
            self.assertEqual(run_git(remote, "rev-parse", f"{value}^{{commit}}"), snapshot)
        self.assertEqual(run_git(remote, "rev-list", "--all"), snapshot)
        development = run_git(self.root, "rev-parse", "local-dev")
        missing = subprocess.run(["git", "cat-file", "-e", development], cwd=remote, capture_output=True)
        self.assertNotEqual(missing.returncode, 0)

    def test_push_refuses_a_remote_that_holds_a_ref_that_is_no_snapshot(self):
        remote = self.bare_remote()
        run_git(self.root, "push", "-q", publish_portable.DEFAULT_REMOTE, "HEAD:refs/heads/existing")
        before = run_git(remote, "for-each-ref")
        result, _ = self.publish("--push", private=lambda repository: True)
        self.assertIsInstance(result, SystemExit)
        self.assertEqual(run_git(remote, "for-each-ref"), before)

    def test_tag_that_is_no_scanned_tag_of_this_command_stops_the_push(self):
        remote = self.bare_remote()
        result, text = self.publish()
        self.assertEqual(result, 0, text)
        neutral = publish_portable.identity_env()
        # A person made these tags on the snapshot commit, with a snapshot name.
        cases = {"another tagger": (dict(os.environ), "a clean message"),
                 "deny value in the message": (neutral, f"built on {DENY_VALUE}")}
        for case, (env, message) in cases.items():
            with self.subTest(case=case):
                subprocess.run(["git", "tag", "-a", "-m", message, "portable/20260101-abcdef0", "portable"],
                               cwd=self.root, env=env, check=True)
                result, output = self.publish("--push", private=lambda repository: True)
                self.assertIsInstance(result, SystemExit)
                self.assertNotIn(DENY_VALUE, output.lower())
                self.assertEqual(run_git(remote, "for-each-ref"), "")
                run_git(self.root, "tag", "-d", "portable/20260101-abcdef0")

    def test_push_refuses_a_remote_that_is_not_the_snapshot_repository(self):
        other = self.temp / "other.git"
        run_git(self.temp, "init", "-q", "--bare", str(other))
        for url in (str(other), "https://github.com/someone/sheprd.git", "ssh://git@forge.invalid/owner/tenant-sheprd.git"):
            with self.subTest(url=url):
                run_git(self.root, "remote", "add", publish_portable.DEFAULT_REMOTE, url)
                result, _ = self.publish("--push", private=lambda repository: True)
                self.assertIsInstance(result, SystemExit)
                self.assertEqual(run_git(other, "for-each-ref"), "")
                run_git(self.root, "remote", "remove", publish_portable.DEFAULT_REMOTE)

    def test_run_without_push_changes_no_remote_ref(self):
        remote = self.temp / "remote.git"
        run_git(self.temp, "init", "-q", "--bare", str(remote))
        run_git(self.root, "remote", "add", publish_portable.DEFAULT_REMOTE, str(remote))
        run_git(self.root, "push", "-q", publish_portable.DEFAULT_REMOTE, "HEAD:refs/heads/existing")
        before = run_git(remote, "for-each-ref")
        tracking = run_git(self.root, "for-each-ref", "refs/remotes")
        result, text = self.publish()
        self.assertEqual(result, 0, text)
        self.assertNotEqual(run_git(self.root, "for-each-ref", "refs/heads/portable"), "")
        self.assertEqual(run_git(remote, "for-each-ref"), before)
        self.assertEqual(run_git(self.root, "for-each-ref", "refs/remotes"), tracking)

    def test_snapshot_branch_with_development_history_is_refused(self):
        run_git(self.root, "branch", "portable", "local-dev")
        self.write("more.md", "public\n")
        self.commit("more")
        result, _ = self.publish()
        self.assertIsInstance(result, SystemExit)
        self.assertEqual(run_git(self.root, "rev-parse", "portable"), run_git(self.root, "rev-parse", "local-dev~1"))


if __name__ == "__main__":
    unittest.main()
