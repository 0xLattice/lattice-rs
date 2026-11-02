#!/usr/bin/env python3
"""Audit/check the separate sibling Rust repositories without modifying them.

This is a local development manifest, not a claim that unpublished repositories
or source commits are available remotely. A report names dirty/uncommitted inputs.
"""
import argparse
import datetime as dt
import hashlib
import json
from pathlib import Path
import subprocess
import sys
try:
    import tomllib
except ImportError:
    raise SystemExit("platform-check requires Python 3.11 or later (stdlib tomllib)")

REPOSITORIES = [
    "array-trie-rs", "multikey-rs", "trie-dictionary-rs", "cashew-rs",
    "tally-rs", "volume-broker-rs", "ivy-rs", "lattice-rs",
    "lattice-sdk-rs", "lattice-storage-rs", "lattice-miner-gpu",
    "lattice-mining-rs", "lattice-node-rs", "lattice-sponsor-rs",
    "lattice-light-client-rs", "uint256-rs", "lattice-ctl-rs",
]


def run(argv, cwd):
    return subprocess.run(argv, cwd=cwd, text=True, stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE, check=False)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def source_fingerprint(repo):
    listed = subprocess.run(["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
                            cwd=repo, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
    if listed.returncode:
        return None
    fingerprint = hashlib.sha256()
    for raw in sorted(set(listed.stdout.split(b"\0")) - {b""}):
        name = raw.decode("utf-8")
        path = repo / name
        # Only code and checked-in build/test inputs, never arbitrary operator
        # key files or local configuration. Cargo fingerprints outside this
        # set remain out of scope, as do native compilers/environment changes.
        source = path.suffix == ".rs" or path.name in ("Cargo.toml", "Cargo.lock", "build.rs")
        fixture = name.startswith(("tests/", "examples/", "crates/")) and path.suffix in (".json", ".wasm", ".wat", ".bin", ".c", ".h", ".metal", ".cl", ".cu")
        if (source or fixture) and path.is_file():
            fingerprint.update(raw + b"\0")
            with path.open("rb") as stream:
                while chunk := stream.read(1024 * 1024):
                    fingerprint.update(chunk)
            fingerprint.update(b"\0")
    return fingerprint.hexdigest()


def manifests(repo):
    root = repo / "Cargo.toml"
    data = tomllib.loads(root.read_text())
    found = [root]
    for member in data.get("workspace", {}).get("members", []):
        found.extend(p / "Cargo.toml" for p in repo.glob(member) if (p / "Cargo.toml").is_file())
    return sorted(set(found))


def inventory(root):
    records, issues = [], []
    known = {str((root / name).resolve()): name for name in REPOSITORIES}
    for name in REPOSITORIES:
        repo = root / name
        if not (repo / "Cargo.toml").is_file():
            issues.append(f"missing repository: {name}")
            continue
        head = run(["git", "rev-parse", "--verify", "HEAD"], repo)
        status = run(["git", "status", "--porcelain", "--untracked-files=normal"], repo)
        record = {"repository": name, "head": head.stdout.strip() if head.returncode == 0 else None,
                  "dirty": bool(status.stdout.strip()), "git_status_available": status.returncode == 0,
                  "manifests": {}, "path_dependencies": [], "packages": [],
                  "source_inputs_sha256": source_fingerprint(repo)}
        if status.returncode:
            issues.append(f"cannot inspect Git status: {name}")
        for manifest in manifests(repo):
            data = tomllib.loads(manifest.read_text())
            record["manifests"][str(manifest.relative_to(repo))] = digest(manifest)
            if "package" in data:
                record["packages"].append(data["package"]["name"])
            groups = [data, data.get("workspace", {})] + list(data.get("target", {}).values())
            for group in groups:
                for table in ("dependencies", "dev-dependencies", "build-dependencies"):
                    for dep, spec in group.get(table, {}).items():
                        if not isinstance(spec, dict) or "path" not in spec:
                            continue
                        target = (manifest.parent / spec["path"]).resolve()
                        owner = next((n for p, n in known.items() if target.is_relative_to(p)), None)
                        exists = (target / "Cargo.toml").is_file()
                        record["path_dependencies"].append({"manifest": str(manifest.relative_to(repo)),
                            "dependency": dep, "path": spec["path"], "repository": owner, "exists": exists})
                        if not exists:
                            issues.append(f"{name}/{manifest.relative_to(repo)}: missing dependency {dep}: {spec['path']}")
                        elif owner is None:
                            issues.append(f"{name}: dependency {dep} is outside the platform manifest")
        lock = repo / "Cargo.lock"
        record["cargo_lock_sha256"] = digest(lock) if lock.is_file() else None
        if not lock.is_file():
            issues.append(f"missing Cargo.lock: {name}")
        records.append(record)
    return records, issues


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--check", action="store_true", help="Run locked cargo check for every selected repository")
    parser.add_argument("--repository", action="append", choices=REPOSITORIES, help="Limit builds; inventory still covers all siblings")
    args = parser.parse_args()
    root = args.root.resolve()
    records, issues = inventory(root)
    report = {"started_at": dt.datetime.now(dt.timezone.utc).isoformat(), "root": str(root),
              "scope": "sibling dependency inventory and optional locked native all-target compilation; not release, test, formal, WASM or GPU runtime validation",
              "repositories": records, "issues": issues, "checks": []}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    if args.check and not issues:
        selected = set(args.repository or REPOSITORIES)
        for record in records:
            name = record["repository"]
            if name not in selected:
                continue
            print(f"checking {name}", flush=True)
            result = run(["cargo", "check", "--workspace", "--all-targets", "--locked"], root / name)
            log = args.output.with_name(args.output.stem + "-" + name + ".log")
            log.write_text(result.stdout + result.stderr)
            report["checks"].append({"repository": name, "exit_code": result.returncode, "log": str(log.resolve())})
            if result.returncode:
                issues.append(f"cargo check failed: {name} (see {log})")
            # Preserve evidence even if interrupted after this repository.
            args.output.write_text(json.dumps(report, indent=2) + "\n")
    relevant = set(args.repository or REPOSITORIES) if args.check else set(REPOSITORIES)
    while True:
        dependencies = {d["repository"] for r in records if r["repository"] in relevant
                        for d in r["path_dependencies"] if d["repository"]}
        expanded = relevant | dependencies
        if expanded == relevant:
            break
        relevant = expanded
    final, final_issues = inventory(root)
    # Running agents may edit manifests while checking. Do not certify those
    # later inputs from an earlier successful build.
    before = {r["repository"]: r for r in records}
    report["inputs_changed"] = []
    for record in final:
        old = before.get(record["repository"], {})
        if any(record.get(k) != old.get(k) for k in ("manifests", "cargo_lock_sha256", "head", "source_inputs_sha256")):
            report["inputs_changed"].append(record["repository"])
            if record["repository"] in relevant:
                issues.append(f"inputs changed during audit: {record['repository']}; rerun that check")
    for issue in final_issues:
        if issue not in issues:
            issues.append(issue)
    report["finished_at"] = dt.datetime.now(dt.timezone.utc).isoformat()
    report["status"] = "failed" if issues else "passed"
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(f"{report['status']}: {len(records)} repositories, {len(report['checks'])} checks; {args.output}")
    for issue in issues:
        print(issue, file=sys.stderr)
    return bool(issues)


if __name__ == "__main__":
    raise SystemExit(main())
