#!/usr/bin/env python3
"""Local-only fixture repositories. No actual remote publication or network."""
import copy
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

spec = importlib.util.spec_from_file_location("bootstrap", Path(__file__).with_name("platform-bootstrap.py"))
tool = importlib.util.module_from_spec(spec)
spec.loader.exec_module(tool)


def git(argv, cwd, env):
    return subprocess.check_output(["git"] + argv, cwd=cwd, env=env, stderr=subprocess.DEVNULL, text=True).strip()


class BootstrapTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.fixture = tempfile.TemporaryDirectory(prefix="lattice-bootstrap-fixtures-")
        cls.base = Path(cls.fixture.name)
        cls.remotes = cls.base / "remotes"
        cls.remotes.mkdir()
        cls.env = os.environ.copy()
        cls.env.update(GIT_CONFIG_NOSYSTEM="1", GIT_ALLOW_PROTOCOL="file", CARGO_NET_OFFLINE="true", GIT_CONFIG_GLOBAL=str(cls.base / "gitconfig"),
                       GIT_AUTHOR_NAME="Fixture", GIT_AUTHOR_EMAIL="fixture@example.invalid",
                       GIT_COMMITTER_NAME="Fixture", GIT_COMMITTER_EMAIL="fixture@example.invalid")
        (cls.base / "gitconfig").write_text('[url "' + cls.remotes.as_uri() + '/"]\n\tinsteadOf = https://github.com/0xLattice/\n')
        rows = []
        for name in tool.REPOSITORIES:
            repo = cls.remotes / (name + ".git")
            repo.mkdir()
            git(["init", "-q"], repo, cls.env)
            (repo / "src").mkdir()
            (repo / "src/lib.rs").write_text("pub fn fixture() -> u32 { 1 }\n")
            (repo / ".gitignore").write_text("/target/\n")
            (repo / "Cargo.toml").write_text('[package]\nname="' + name + '"\nversion="0.1.0"\nedition="2024"\n')
            (repo / "Cargo.lock").write_text('version = 4\n\n[[package]]\nname = "' + name + '"\nversion = "0.1.0"\n')
            git(["add", "."], repo, cls.env)
            git(["commit", "-qm", "Fixture source"], repo, cls.env)
            rows.append({"directory": name, "repository": "0xLattice/" + name, "commit": git(["rev-parse", "HEAD"], repo, cls.env)})
        cls.manifest = {"format": tool.FORMAT, "organization": "0xLattice", "repositories": rows}

    @classmethod
    def tearDownClass(cls):
        cls.fixture.cleanup()

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="lattice-bootstrap-test-")
        self.base = Path(self.tmp.name)
        self.env_patch = mock.patch.dict(os.environ, self.env, clear=True)
        self.env_patch.start()

    def tearDown(self):
        self.env_patch.stop()
        self.tmp.cleanup()

    def create(self, build=False):
        destination = self.base / "sources"
        result = tool.bootstrap(self.manifest, destination, build=build)
        self.assertTrue(result["passed"])
        return destination, result

    def test_exact_local_checkout_and_locked_build_of_all_seventeen(self):
        destination, result = self.create(build=True)
        self.assertEqual(len(result["checks"]), 17)
        self.assertEqual(len(list(destination.iterdir())), 17)
        for row in self.manifest["repositories"]:
            self.assertEqual(git(["rev-parse", "HEAD"], destination / row["directory"], self.env), row["commit"])
            self.assertFalse((destination / row["directory"] / "target").exists())

    def test_clean_existing_destination_is_verified_without_replacing_files(self):
        destination, _ = self.create()
        file = destination / tool.REPOSITORIES[0] / "src/lib.rs"
        before = file.stat()
        result = tool.bootstrap(self.manifest, destination)
        self.assertTrue(result["existing_destination"])
        self.assertEqual(file.stat().st_ino, before.st_ino)
        self.assertEqual(file.stat().st_mtime_ns, before.st_mtime_ns)

    def test_dirty_and_ignored_files_refuse_before_checkout(self):
        destination, _ = self.create()
        repo = destination / tool.REPOSITORIES[0]
        user = repo / "user-data.txt"
        user.write_text("keep this")
        with self.assertRaisesRegex(tool.BootstrapError, "modified, untracked"):
            tool.bootstrap(self.manifest, destination)
        self.assertEqual(user.read_text(), "keep this")
        user.unlink()
        (repo / "target").mkdir()
        ignored = repo / "target/private.txt"
        ignored.write_text("keep ignored")
        with self.assertRaisesRegex(tool.BootstrapError, "modified, untracked"):
            tool.bootstrap(self.manifest, destination)
        self.assertEqual(ignored.read_text(), "keep ignored")

    def test_wrong_origin_and_commit_are_refused(self):
        destination, _ = self.create()
        repo = destination / tool.REPOSITORIES[0]
        git(["remote", "set-url", "origin", "https://github.com/other/repository.git"], repo, self.env)
        with self.assertRaisesRegex(tool.BootstrapError, "wrong organization"):
            tool.bootstrap(self.manifest, destination)
        git(["remote", "set-url", "origin", tool.canonical_remote(self.manifest["repositories"][0])], repo, self.env)
        changed = copy.deepcopy(self.manifest)
        changed["repositories"][0]["commit"] = "0" * 40
        with self.assertRaisesRegex(tool.BootstrapError, "commit differs"):
            tool.bootstrap(changed, destination)

    def test_unavailable_commit_does_not_install_partial_tree(self):
        changed = copy.deepcopy(self.manifest)
        changed["repositories"][-1]["commit"] = "0" * 40
        destination = self.base / "sources"
        with self.assertRaisesRegex(tool.BootstrapError, "exact revision fetch failed"):
            tool.bootstrap(changed, destination)
        self.assertFalse(destination.exists())
        self.assertEqual(list(self.base.iterdir()), [])

    def test_bad_manifest_and_user_destination_never_run_git(self):
        variants = []
        for field, value in [("organization", "other"), ("format", "unknown")]:
            m = copy.deepcopy(self.manifest)
            m[field] = value
            variants.append(m)
        for field, value in [("repository", "https://token@github.com/0xLattice/repo"),
                             ("repository", "other/repo"), ("commit", "main"), ("directory", "../escape")]:
            m = copy.deepcopy(self.manifest)
            m["repositories"][0][field] = value
            variants.append(m)
        with mock.patch.object(tool, "command", side_effect=AssertionError("Git must not run")):
            for m in variants:
                with self.subTest(m=m), self.assertRaises(tool.BootstrapError):
                    tool.bootstrap(m, self.base / "sources")
            destination = self.base / "existing"
            destination.mkdir()
            (destination / "user.txt").write_text("preserve")
            with self.assertRaises(tool.BootstrapError):
                tool.bootstrap(self.manifest, destination)
            self.assertEqual((destination / "user.txt").read_text(), "preserve")

    def test_dependency_escape_and_missing_sibling_are_refused(self):
        destination, _ = self.create()
        manifest = destination / tool.REPOSITORIES[0] / "Cargo.toml"
        original = manifest.read_text()
        for path in ["../../outside", "../missing", "/absolute"]:
            manifest.write_text(original + '\n[dependencies]\nmissing={path="' + path + '"}\n')
            with self.assertRaises(tool.BootstrapError):
                tool.validate_dependencies(destination, self.manifest["repositories"])
        manifest.write_text(original + '\n[dependencies]\nother={path="../multikey-rs",package="multikey-rs"}\n')
        self.assertEqual(tool.validate_dependencies(destination, self.manifest["repositories"]), 1)

    def test_git_failure_does_not_echo_sensitive_diagnostics(self):
        failed = subprocess.CompletedProcess(["git"], 1, stdout=b"private-output", stderr=b"credential-secret")
        with mock.patch.object(subprocess, "run", return_value=failed):
            with self.assertRaises(tool.BootstrapError) as caught:
                tool.command(["git"], self.base, "fixture fetch")
        self.assertEqual(str(caught.exception), "fixture fetch failed")

    def test_cli_report_is_exclusive_and_failed_run_is_not_success(self):
        manifest = self.base / "manifest.json"
        manifest.write_text("{}")
        report = self.base / "report.json"
        args = ["python3.12", str(Path(tool.__file__)), str(manifest), "--destination", str(self.base / "out"), "--report", str(report)]
        result = subprocess.run(args, capture_output=True, env=self.env)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(json.loads(report.read_text())["passed"])
        original = report.read_bytes()
        result = subprocess.run(args, capture_output=True, env=self.env)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(report.read_bytes(), original)


if __name__ == "__main__":
    unittest.main()
