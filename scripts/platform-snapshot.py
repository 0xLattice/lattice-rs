#!/usr/bin/env python3
"""Export exact local source inputs and check a fresh sibling copy, without Git commits.

Archives contain no Git databases or build caches. This is a source snapshot,
not a published release or a hermetic compiler/registry dependency bundle.
"""
import argparse
import datetime as dt
import hashlib
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import stat
import subprocess
import tarfile
import tempfile

spec = importlib.util.spec_from_file_location("platform_audit", Path(__file__).with_name("platform-check.py"))
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)
NAMES = audit.REPOSITORIES
SUFFIXES = set(".rs .toml .lock .json .jsonl .md .py .swift .mjs .js .ts .sh .yml .yaml .tsv .lean .hex .txt .html .resolved .wasm .wat .bin .c .h .metal .cl .cu .ptx .css .svg".split())
BASENAMES = {".gitignore", "LICENSE", "UPSTREAM", "Dockerfile", "Makefile", "CMakeLists.txt", "lean-toolchain"}


def sha(data):
    return hashlib.sha256(data).hexdigest()


def safe_name(name):
    path = PurePosixPath(name)
    return not path.is_absolute() and bool(path.parts) and all(p not in (".", "..", "") for p in path.parts) and "\\" not in name and str(path) == name


def selected(root):
    files, omitted, repositories = {}, [], []
    for name in NAMES:
        repo = root / name
        listed = subprocess.check_output(["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"], cwd=repo)
        head = audit.run(["git", "rev-parse", "--verify", "HEAD"], repo)
        status = audit.run(["git", "status", "--porcelain", "--untracked-files=normal"], repo)
        repositories.append({"repository": name, "head": head.stdout.strip() if head.returncode == 0 else None, "dirty": bool(status.stdout.strip())})
        for raw in sorted(set(listed.split(b"\0")) - {b""}):
            relative = raw.decode("utf-8")
            assert safe_name(relative), f"unsafe source path: {relative}"
            path = repo / relative
            if not path.exists() and not path.is_symlink():
                continue  # A tracked deletion is represented by absence.
            assert not path.is_symlink(), f"symlink not supported: {path}"
            assert path.is_file(), f"non-regular file/submodule not supported: {path}"
            key = name + "/" + relative
            if path.suffix not in SUFFIXES and path.name not in BASENAMES:
                omitted.append(key)
                continue
            mode = 0o755 if path.stat().st_mode & stat.S_IXUSR else 0o644
            data = path.read_bytes()
            files[key] = {"sha256": sha(data), "size": len(data), "mode": mode}
    return files, omitted, repositories


def export(root, output):
    assert not output.exists(), f"refusing to replace existing snapshot: {output}"
    inventory, issues = audit.inventory(root)
    assert not issues, issues
    before, omitted, repositories = selected(root)
    manifest = {"format": "lattice-platform-source-v1", "repositories": repositories,
                "files": before, "omitted": omitted, "dependency_inventory": inventory}
    # Source metadata is useful evidence but absolute working paths are excluded.
    encoded = (json.dumps(manifest, sort_keys=True, indent=2) + "\n").encode()
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_name(output.name + ".tmp")
    assert not temporary.exists(), f"temporary destination already exists: {temporary}"
    try:
        with tarfile.open(temporary, "w", format=tarfile.USTAR_FORMAT) as archive:
            import io
            for name, data, mode in [("SNAPSHOT.json", encoded, 0o644)] + [
                (name, (root / name).read_bytes(), meta["mode"]) for name, meta in sorted(before.items())
            ]:
                if name != "SNAPSHOT.json":
                    assert sha(data) == before[name]["sha256"], f"source changed while copying: {name}"
                member = tarfile.TarInfo(name)
                member.size, member.mode = len(data), mode
                member.mtime = member.uid = member.gid = 0
                member.uname = member.gname = ""
                archive.addfile(member, io.BytesIO(data))
        after, omitted_after, repositories_after = selected(root)
        assert (before, omitted, repositories) == (after, omitted_after, repositories_after), "source inputs changed during export; retry at a stable checkpoint"
        os.replace(temporary, output)
    finally:
        temporary.unlink(missing_ok=True)
    print(json.dumps({"archive": str(output), "sha256": sha(output.read_bytes()), "files": len(before), "repositories": len(repositories), "omitted": omitted}, indent=2))


def unpack(archive_path, destination):
    with tarfile.open(archive_path, "r:") as archive:
        members = archive.getmembers()
        names = [member.name for member in members]
        assert len(names) == len(set(names)), "duplicate archive member"
        assert all(safe_name(m.name) and m.isfile() for m in members), "unsafe/non-regular archive member"
        manifest = json.load(archive.extractfile("SNAPSHOT.json"))
        assert manifest["format"] == "lattice-platform-source-v1", "unsupported snapshot format"
        names_in_manifest = [r["repository"] for r in manifest["repositories"]]
        assert names_in_manifest and len(names_in_manifest) == len(set(names_in_manifest)), "duplicate/empty repository inventory"
        assert all(safe_name(name) and "/" not in name for name in names_in_manifest), "unsafe repository name"
        assert set(names) == set(manifest["files"]) | {"SNAPSHOT.json"}, "archive file inventory mismatch"
        for member in members:
            if member.name == "SNAPSHOT.json":
                continue
            assert member.name.split("/", 1)[0] in names_in_manifest, "file outside platform repositories"
            data = archive.extractfile(member).read()
            expected = manifest["files"][member.name]
            assert member.mode in (0o644, 0o755) and member.mode == expected["mode"], "mode mismatch"
            assert len(data) == expected["size"] and sha(data) == expected["sha256"], f"content mismatch: {member.name}"
            path = destination / member.name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
            path.chmod(member.mode)
    return manifest


def verify_tree(root, manifest):
    for name, expected in manifest["files"].items():
        path = root / name
        assert path.is_file() and not path.is_symlink() and sha(path.read_bytes()) == expected["sha256"], f"snapshot input changed: {name}"
    actual = {str(path.relative_to(root)) for name in [r["repository"] for r in manifest["repositories"]] for path in (root / name).rglob("*") if path.is_file()}
    assert actual == set(manifest["files"]), f"build wrote source files: {sorted(actual - set(manifest['files']))}"


def check(archive_path, output, repositories, verify_only):
    report = {"passed": False, "started": dt.datetime.now(dt.timezone.utc).isoformat(),
              "archive_sha256": sha(archive_path.read_bytes()), "checks": [],
              "scope": "fresh exported source tree; locked native all-target check; registry cache/toolchain not bundled; no tests or release claim"}
    output.parent.mkdir(parents=True, exist_ok=True)
    def save():
        output.write_text(json.dumps(report, indent=2) + "\n")
    save()
    try:
        with tempfile.TemporaryDirectory(prefix="lattice-source-check-") as tmp:
            base = Path(tmp)
            root = base / "sources"
            root.mkdir()
            manifest = unpack(archive_path, root)
            report["files"] = len(manifest["files"])
            report["repositories"] = manifest["repositories"]
            # Every path dependency must resolve in this fresh exported layout.
            archived_names = [r["repository"] for r in manifest["repositories"]]
            assert not repositories or set(repositories).issubset(archived_names), "selected repository absent from snapshot"
            known = {(root / name).resolve() for name in archived_names}
            for name in archived_names:
                for path in audit.manifests(root / name):
                    data = audit.tomllib.loads(path.read_text())
                    groups = [data, data.get("workspace", {})] + list(data.get("target", {}).values())
                    for group in groups:
                        for table in ("dependencies", "dev-dependencies", "build-dependencies"):
                            for value in group.get(table, {}).values():
                                if isinstance(value, dict) and "path" in value:
                                    target = (path.parent / value["path"]).resolve()
                                    assert any(target.is_relative_to(owner) for owner in known) and (target / "Cargo.toml").is_file(), f"unresolved exported dependency: {path}: {value}"
            if not verify_only:
                report["rustc"] = subprocess.check_output(["rustc", "-Vv"], text=True).strip()
                report["cargo"] = subprocess.check_output(["cargo", "-V"], text=True).strip()
                env = os.environ.copy()
                env["CARGO_TARGET_DIR"] = str(base / "target")
                for name in repositories or archived_names:
                    print("checking exported " + name, flush=True)
                    log = output.with_name(output.stem + "-" + name + ".log")
                    with log.open("w") as stream:
                        result = subprocess.run(["cargo", "check", "--locked", "--workspace", "--all-targets"], cwd=root / name, env=env, stdout=stream, stderr=subprocess.STDOUT)
                    report["checks"].append({"repository": name, "exit_code": result.returncode, "log": str(log.resolve())})
                    save()

            verify_tree(root, manifest)
            failures = [row["repository"] for row in report["checks"] if row["exit_code"]]
            assert not failures, f"exported builds failed: {failures}; see individual logs"
            report["passed"] = True
    except Exception as error:
        report["error"] = str(error)
        raise
    finally:
        report["completed"] = dt.datetime.now(dt.timezone.utc).isoformat()
        save()
    print(json.dumps({key: value for key, value in report.items() if key != "repositories"}, indent=2))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    create = sub.add_parser("export", help="Capture current Git-listed source and untracked source inputs")
    create.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    create.add_argument("--output", type=Path, required=True)
    for command in ("verify", "check"):
        consume = sub.add_parser(command)
        consume.add_argument("archive", type=Path)
        consume.add_argument("--output", type=Path, required=True)
        if command == "check":
            consume.add_argument("--repository", action="append", choices=NAMES)
    args = parser.parse_args()
    if args.command == "export":
        export(args.root.resolve(), args.output.resolve())
    else:
        check(args.archive.resolve(), args.output.resolve(), getattr(args, "repository", None), args.command == "verify")


if __name__ == "__main__":
    main()
