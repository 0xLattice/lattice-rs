#!/usr/bin/env python3
"""Pin CI to its checked-out revision, retaining exact reviewed sibling inputs."""
import argparse
import importlib.util
import json
from pathlib import Path

spec = importlib.util.spec_from_file_location("bootstrap", Path(__file__).with_name("platform-bootstrap.py"))
bootstrap = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bootstrap)


def current_manifest(manifest, source, repository):
    rows = bootstrap.validate_manifest(manifest)
    matches = [row for row in rows if row["repository"] == repository]
    bootstrap.require(len(matches) == 1, "CI repository must be in the reviewed manifest")
    row = matches[0]
    source = Path(source).resolve()
    bootstrap.require(Path(bootstrap.command(["git", "rev-parse", "--show-toplevel"], source, "CI root inspection")).resolve() == source, "CI source must be a checkout root")
    origin = bootstrap.command(["git", "config", "--get", "remote.origin.url"], source, "CI origin inspection")
    bootstrap.require(bootstrap.accepted_origin(origin, row), "CI checkout has wrong repository origin")
    # Ignore build caches but reject any tracked source/lock edits. The CI source
    # is used only to identify an object; bootstrap fetches the actual commit.
    dirty = bootstrap.command(["git", "status", "--porcelain=v1", "--untracked-files=no"], source, "CI source inspection")
    bootstrap.require(not dirty, "CI checkout has modified tracked files")
    commit = bootstrap.command(["git", "rev-parse", "--verify", "HEAD^{commit}"], source, "CI commit inspection")
    result = {"format": bootstrap.FORMAT, "organization": "0xLattice", "repositories": [dict(item, commit=commit) if item["repository"] == repository else dict(item) for item in rows]}
    bootstrap.validate_manifest(result)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        value = current_manifest(json.loads(args.manifest.read_text()), args.source, args.repository)
        with args.output.open("x") as output:
            json.dump(value, output, indent=2)
            output.write("\n")
    except (bootstrap.BootstrapError, OSError, ValueError) as error:
        print(str(error) if isinstance(error, bootstrap.BootstrapError) else "CI manifest input/output failed")
        return 1
    print("CI revision and reviewed sibling pins recorded")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
