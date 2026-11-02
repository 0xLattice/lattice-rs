#!/usr/bin/env python3
"""Archive integrity and extraction-boundary regression checks."""
import importlib.util
import io
import contextlib
import os
import subprocess
import json
from pathlib import Path
import tarfile
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("snapshot", Path(__file__).with_name("platform-snapshot.py"))
snapshot = importlib.util.module_from_spec(spec)
spec.loader.exec_module(snapshot)


class ArchiveTests(unittest.TestCase):
    def archive(self, root, change=None):
        data = b"fn main() {}\n"
        filename = "lattice-rs/src/main.rs"
        manifest = {"format": "lattice-platform-source-v1",
                    "repositories": [{"repository": n} for n in snapshot.NAMES],
                    "files": {filename: {"sha256": snapshot.sha(data), "size": len(data), "mode": 0o644}}}
        rows = [("SNAPSHOT.json", json.dumps(manifest).encode(), tarfile.REGTYPE, 0o644),
                (filename, data, tarfile.REGTYPE, 0o644)]
        if change:
            change(rows)
        path = root / "fixture.tar"
        with tarfile.open(path, "w") as archive:
            for name, content, kind, mode in rows:
                member = tarfile.TarInfo(name)
                member.type, member.size, member.mode = kind, len(content), mode
                archive.addfile(member, io.BytesIO(content))
        return path

    def verify(self, change=None):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            destination = root / "unpacked"
            destination.mkdir()
            manifest = snapshot.unpack(self.archive(root, change), destination)
            snapshot.verify_tree(destination, manifest)

    def test_export_is_deterministic_for_uncommitted_sources(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for name in snapshot.NAMES:
                repo = root / name
                repo.mkdir()
                subprocess.run(["git", "init", "-q", str(repo)], check=True)
                (repo / "Cargo.toml").write_text(f'[package]\nname="{name}"\nversion="0.1.0"\n')
                (repo / "Cargo.lock").write_text("version = 4\n")
                (repo / "src").mkdir()
                (repo / "src/lib.rs").write_text("pub fn fixture() {}\n")
            first, second = root / "one.tar", root / "two.tar"
            with contextlib.redirect_stdout(io.StringIO()):
                snapshot.export(root, first)
                for name in snapshot.NAMES:
                    os.utime(root / name / "src/lib.rs", (123, 456))
                snapshot.export(root, second)
            self.assertEqual(first.read_bytes(), second.read_bytes())

    def test_regular_archive(self):
        self.verify()

    def test_changed_bytes_are_refused(self):
        with self.assertRaisesRegex(AssertionError, "content mismatch"):
            self.verify(lambda rows: rows.__setitem__(1, (rows[1][0], b"fn fake() {}\n", tarfile.REGTYPE, 0o644)))

    def test_unlisted_file_is_refused(self):
        with self.assertRaisesRegex(AssertionError, "inventory mismatch"):
            self.verify(lambda rows: rows.append(("lattice-rs/extra.rs", b"extra", tarfile.REGTYPE, 0o644)))

    def test_duplicate_is_refused(self):
        with self.assertRaisesRegex(AssertionError, "duplicate"):
            self.verify(lambda rows: rows.append(rows[1]))

    def test_escape_and_symlink_are_refused(self):
        for name, kind in [("../escaped", tarfile.REGTYPE), ("/absolute", tarfile.REGTYPE), ("lattice-rs/link", tarfile.SYMTYPE)]:
            with self.subTest(name=name), self.assertRaisesRegex(AssertionError, "unsafe"):
                self.verify(lambda rows: rows.append((name, b"", kind, 0o644)))

    def test_changed_source_after_unpack_is_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            destination = root / "unpacked"
            destination.mkdir()
            manifest = snapshot.unpack(self.archive(root), destination)
            (destination / "lattice-rs/src/main.rs").write_text("changed")
            with self.assertRaisesRegex(AssertionError, "snapshot input changed"):
                snapshot.verify_tree(destination, manifest)


if __name__ == "__main__":
    unittest.main()
