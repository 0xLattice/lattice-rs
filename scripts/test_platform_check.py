import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("platform_check", Path(__file__).with_name("platform-check.py"))
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


class InventoryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.original = audit.REPOSITORIES
        audit.REPOSITORIES = ["one", "two"]
        for name in audit.REPOSITORIES:
            repo = self.root / name
            repo.mkdir()
            subprocess.run(["git", "init", "-q", str(repo)], check=True)
            (repo / "Cargo.toml").write_text(f'[package]\nname="{name}"\nversion="0.1.0"\n')
            (repo / "Cargo.lock").write_text('version=4\n')
            (repo / "src").mkdir()
            (repo / "src/lib.rs").write_text('pub fn value()->u8{1}\n')

    def tearDown(self):
        audit.REPOSITORIES = self.original
        self.temp.cleanup()

    def test_workspace_target_dependency_and_missing_path_are_reported(self):
        repo = self.root / "one"
        (repo / "Cargo.toml").write_text('[workspace]\nmembers=["crates/*"]\n')
        member = repo / "crates/inner"
        member.mkdir(parents=True)
        (member / "Cargo.toml").write_text('[package]\nname="inner"\nversion="0.1.0"\n'
            '[target.\'cfg(unix)\'.dependencies]\ntwo={path="../../../two"}\n'
            '[dev-dependencies]\nmissing={path="../../../absent"}\n')
        records, issues = audit.inventory(self.root)
        self.assertEqual(records[0]["packages"], ["inner"])
        edge = next(d for d in records[0]["path_dependencies"] if d["dependency"] == "two")
        self.assertEqual(edge["repository"], "two")
        self.assertTrue(edge["exists"])
        self.assertTrue(any("missing dependency missing" in issue for issue in issues))

    def test_uncommitted_code_and_fixture_changes_affect_fingerprint(self):
        repo = self.root / "one"
        before = audit.source_fingerprint(repo)
        (repo / "src/lib.rs").write_text('pub fn value()->u8{2}\n')
        changed = audit.source_fingerprint(repo)
        self.assertNotEqual(before, changed)
        (repo / "tests").mkdir()
        (repo / "tests/vector.json").write_text('{"expected":2}')
        fixture = audit.source_fingerprint(repo)
        self.assertNotEqual(changed, fixture)
        (repo / "identity.key").write_text('not-a-real-key')
        self.assertEqual(fixture, audit.source_fingerprint(repo))

    def test_new_uncommitted_repositories_are_not_claimed_as_pinned(self):
        records, issues = audit.inventory(self.root)
        self.assertEqual(issues, [])
        self.assertTrue(all(r["head"] is None and r["dirty"] for r in records))
        (self.root / "two/Cargo.lock").unlink()
        _, issues = audit.inventory(self.root)
        self.assertIn("missing Cargo.lock: two", issues)


if __name__ == "__main__":
    unittest.main()
