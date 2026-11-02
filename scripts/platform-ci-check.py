#!/usr/bin/env python3
"""Run format, strict Clippy and native tests against exact sibling revisions."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import subprocess

spec = importlib.util.spec_from_file_location("bootstrap", Path(__file__).with_name("platform-bootstrap.py"))
bootstrap = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bootstrap)


def check(manifest, sources, target, report):
    rows = bootstrap.validate_manifest(manifest)
    sources, target = Path(sources).resolve(), Path(target).resolve()
    bootstrap.require(not target.is_relative_to(sources), "CI target directory must be outside source")
    for row in rows:
        bootstrap.inspect_repository(sources / row["directory"], row)
    bootstrap.validate_dependencies(sources, rows)
    result = {"passed": False, "repositories": rows, "checks": []}
    env = os.environ.copy()
    # Authentication belongs only to the earlier fetch step, never test code.
    bootstrap.require(not any(env.get(key) for key in ("GH_TOKEN", "GITHUB_TOKEN", "LATTICE_PLATFORM_READ_TOKEN")), "remove fetch credentials before running builds")
    env["CARGO_TARGET_DIR"] = str(target)
    commands = [("format", ["fmt", "--all", "--check"]),
                ("clippy", ["clippy", "--workspace", "--all-targets", "--locked", "--", "-D", "warnings"]),
                ("test", ["test", "--workspace", "--all-targets", "--locked"])]
    with Path(report).open("x") as output:
        def save():
            output.seek(0)
            json.dump(result, output, indent=2)
            output.write("\n")
            output.truncate()
            output.flush()
        save()
        for row in rows:
            name = row["directory"]
            for label, args in commands:
                print(name + ": " + label, flush=True)
                completed = subprocess.run(["cargo", *args], cwd=sources / name, env=env, check=False)
                result["checks"].append({"repository": name, "check": label, "passed": completed.returncode == 0})
                save()
                if completed.returncode:
                    # Keep checking independent repositories so one platform
                    # failure cannot hide the remaining compatibility gaps.
                    break
        # Tests may write ignored diagnostics, but no tracked source/lock edits.
        for row in rows:
            dirty = bootstrap.command(["git", "status", "--porcelain=v1", "--untracked-files=no"], sources / row["directory"], row["directory"] + ": post-test source inspection")
            bootstrap.require(not dirty, row["directory"] + ": build changed tracked source")
        result["passed"] = len(result["checks"]) == len(rows) * len(commands) and all(item["passed"] for item in result["checks"])
        save()
    return result["passed"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--sources", type=Path, required=True)
    parser.add_argument("--target", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    try:
        return 0 if check(json.loads(args.manifest.read_text()), args.sources, args.target, args.report) else 1
    except (bootstrap.BootstrapError, OSError, ValueError) as error:
        print(str(error) if isinstance(error, bootstrap.BootstrapError) else "CI check input/output failed")
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
