#!/usr/bin/env python3
"""CI revision/credential guards using disposable local fixture repositories."""
import copy
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest import mock


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


fixtures = load("fixtures", "test_platform_bootstrap.py")
manifest_tool = load("manifest_tool", "platform-ci-manifest.py")
checks = load("checks", "platform-ci-check.py")


class CiTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        fixtures.BootstrapTests.setUpClass()
        cls.fixture = fixtures.BootstrapTests
        for row in cls.fixture.manifest["repositories"]:
            repo = cls.fixture.remotes / (row["directory"] + ".git")
            (repo / "src/lib.rs").write_text("pub fn fixture() -> u32 {\n    1\n}\n")
            fixtures.git(["add", "."], repo, cls.fixture.env)
            fixtures.git(["commit", "-qm", "Formatted fixture"], repo, cls.fixture.env)
            row["commit"] = fixtures.git(["rev-parse", "HEAD"], repo, cls.fixture.env)

    @classmethod
    def tearDownClass(cls):
        fixtures.BootstrapTests.tearDownClass()

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="lattice-ci-fixtures-")
        self.base = Path(self.tmp.name)
        self.env = dict(self.fixture.env)
        for key in ("GH_TOKEN", "GITHUB_TOKEN", "LATTICE_PLATFORM_READ_TOKEN"):
            self.env.pop(key, None)
        self.patch = mock.patch.dict(os.environ, self.env, clear=True)
        self.patch.start()
        self.sources = self.base / "sources"
        fixtures.tool.bootstrap(self.fixture.manifest, self.sources)

    def tearDown(self):
        self.patch.stop()
        self.tmp.cleanup()

    def test_checkout_revision_replaces_only_its_own_pin(self):
        source = self.sources / "lattice-rs"
        (source / "src/lib.rs").write_text("pub fn changed() -> u32 { 2 }\n")
        fixtures.git(["add", "."], source, self.env)
        fixtures.git(["commit", "-qm", "Changed fixture"], source, self.env)
        actual = fixtures.git(["rev-parse", "HEAD"], source, self.env)
        original = copy.deepcopy(self.fixture.manifest)
        result = manifest_tool.current_manifest(original, source, "0xLattice/lattice-rs")
        self.assertEqual(original, self.fixture.manifest)
        expected = {row["directory"]: row["commit"] for row in original["repositories"]}
        expected["lattice-rs"] = actual
        self.assertEqual({row["directory"]: row["commit"] for row in result["repositories"]}, expected)
        self.assertNotEqual(actual, next(row["commit"] for row in original["repositories"] if row["directory"] == "lattice-rs"))
        fixtures.git(["push", "origin", "HEAD:refs/heads/ci-fixture"], source, self.env)
        fetched = self.base / "tested-revision"
        fixtures.tool.bootstrap(result, fetched)
        self.assertEqual((fetched / "lattice-rs/src/lib.rs").read_text(), "pub fn changed() -> u32 { 2 }\n")

    def test_dirty_foreign_and_nonroot_checkout_refused(self):
        source = self.sources / "lattice-rs"
        with self.assertRaises(manifest_tool.bootstrap.BootstrapError):
            manifest_tool.current_manifest(self.fixture.manifest, source, "0xLattice/ivy-rs")
        with self.assertRaises(manifest_tool.bootstrap.BootstrapError):
            manifest_tool.current_manifest(self.fixture.manifest, source / "src", "0xLattice/lattice-rs")
        (source / "src/lib.rs").write_text("changed tracked input")
        with self.assertRaisesRegex(manifest_tool.bootstrap.BootstrapError, "modified tracked"):
            manifest_tool.current_manifest(self.fixture.manifest, source, "0xLattice/lattice-rs")

    def test_builds_reject_fetch_token_before_running_code(self):
        with mock.patch.dict(os.environ, {"GH_TOKEN": "fixture-not-a-credential"}), mock.patch.object(checks.subprocess, "run", wraps=checks.subprocess.run) as run:
            with self.assertRaisesRegex(checks.bootstrap.BootstrapError, "remove fetch credentials"):
                checks.check(self.fixture.manifest, self.sources, self.base / "target", self.base / "result.json")
            self.assertTrue(all(call.args[0][0] == "git" for call in run.call_args_list))
        self.assertFalse((self.base / "result.json").exists())

    def test_full_fixture_suite_and_source_integrity(self):
        report = self.base / "result.json"
        self.assertTrue(checks.check(self.fixture.manifest, self.sources, self.base / "target", report))
        result = json.loads(report.read_text())
        self.assertTrue(result["passed"])
        self.assertEqual(len(result["checks"]), 51)
        self.assertTrue(all(row["passed"] for row in result["checks"]))
        self.assertFalse(any((self.sources / row["directory"] / "target").exists() for row in self.fixture.manifest["repositories"]))
        with self.assertRaises(FileExistsError):
            checks.check(self.fixture.manifest, self.sources, self.base / "target", report)
        self.assertTrue(json.loads(report.read_text())["passed"])

    def test_failing_format_produces_failed_report(self):
        # Capture the deliberately unformatted file as a real input commit.
        source = self.sources / "array-trie-rs"
        (source / "src/lib.rs").write_text("pub fn poorly_formatted()->u32{1}\n")
        fixtures.git(["add", "."], source, self.env)
        fixtures.git(["commit", "-qm", "Unformatted fixture"], source, self.env)
        manifest = manifest_tool.current_manifest(self.fixture.manifest, source, "0xLattice/array-trie-rs")
        report = self.base / "result.json"
        self.assertFalse(checks.check(manifest, self.sources, self.base / "target", report))
        result = json.loads(report.read_text())
        self.assertFalse(result["passed"])
        self.assertEqual(result["checks"], [{"repository": "array-trie-rs", "check": "format", "passed": False}])


if __name__ == "__main__":
    unittest.main()
