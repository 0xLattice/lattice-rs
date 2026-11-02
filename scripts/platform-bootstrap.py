#!/usr/bin/env python3
"""Exact 0xLattice sibling checkouts; credentials use ordinary configured Git auth."""
import argparse
import contextlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import tomllib

REPOSITORIES = (
    "array-trie-rs", "multikey-rs", "uint256-rs", "trie-dictionary-rs",
    "cashew-rs", "tally-rs", "volume-broker-rs", "ivy-rs", "lattice-rs",
    "lattice-sdk-rs", "lattice-storage-rs", "lattice-miner-gpu",
    "lattice-mining-rs", "lattice-node-rs", "lattice-sponsor-rs",
    "lattice-light-client-rs", "lattice-ctl-rs",
)
FORMAT = "lattice-platform-revisions-v1"


class BootstrapError(Exception):
    """Messages intentionally omit Git output, credentials and local paths."""


def require(condition, message):
    if not condition:
        raise BootstrapError(message)


def validate_manifest(value):
    require(isinstance(value, dict) and set(value) == {"format", "organization", "repositories"}, "invalid manifest fields")
    require(value["format"] == FORMAT and value["organization"] == "0xLattice", "manifest must use 0xLattice and supported format")
    rows = value["repositories"]
    require(isinstance(rows, list) and len(rows) == len(REPOSITORIES), "manifest must name all 17 siblings")
    seen, remotes = set(), set()
    for row in rows:
        require(isinstance(row, dict) and set(row) == {"directory", "repository", "commit"}, "invalid repository fields")
        name, repository, commit = row["directory"], row["repository"], row["commit"]
        require(isinstance(name, str) and name in REPOSITORIES and name not in seen, "unknown or duplicate sibling")
        require(isinstance(repository, str) and re.fullmatch(r"0xLattice/[A-Za-z0-9][A-Za-z0-9_.-]*", repository) is not None, "repository must be a 0xLattice owner/name without credentials or URL")
        require(repository not in remotes and not repository.endswith(".git"), "duplicate or ambiguous repository")
        require(isinstance(commit, str) and re.fullmatch(r"[0-9a-f]{40}", commit) is not None, "revision must be a full lowercase 40-character commit ID")
        seen.add(name)
        remotes.add(repository)
    require(seen == set(REPOSITORIES), "incomplete sibling inventory")
    return sorted(rows, key=lambda row: REPOSITORIES.index(row["directory"]))


def command(argv, cwd, label, env=None):
    # Never log argv/stdout/stderr: credential helpers and build scripts may
    # include secrets or personal paths. All commands use argv, never a shell.
    try:
        environment = os.environ.copy() if env is None else env.copy()
        environment["GIT_TERMINAL_PROMPT"] = "0"
        result = subprocess.run(argv, cwd=cwd, env=environment, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
    except OSError:
        raise BootstrapError(label + " could not start") from None
    require(result.returncode == 0, label + " failed")
    return result.stdout.decode("utf-8", errors="replace").strip()


def canonical_remote(row):
    return "https://github.com/" + row["repository"] + ".git"


def accepted_origin(origin, row):
    path = row["repository"]
    return origin in ("https://github.com/" + path, "https://github.com/" + path + ".git",
                      "git@github.com:" + path, "git@github.com:" + path + ".git",
                      "ssh://git@github.com/" + path, "ssh://git@github.com/" + path + ".git")


def inspect_repository(directory, row):
    name = row["directory"]
    require(directory.is_dir() and not directory.is_symlink(), name + ": real directory required")
    require((directory / ".git").is_dir() and not (directory / ".git").is_symlink(), name + ": standalone Git checkout required")
    top = command(["git", "rev-parse", "--show-toplevel"], directory, name + ": Git root inspection")
    require(Path(top).resolve() == directory.resolve(), name + ": wrong Git root")
    origin = command(["git", "config", "--get", "remote.origin.url"], directory, name + ": origin inspection")
    require(accepted_origin(origin, row), name + ": wrong organization or repository origin")
    commit = command(["git", "rev-parse", "--verify", "HEAD^{commit}"], directory, name + ": commit inspection")
    require(commit == row["commit"], name + ": commit differs from manifest")
    dirty = command(["git", "status", "--porcelain=v1", "--untracked-files=all", "--ignored=matching"], directory, name + ": status inspection")
    require(not dirty, name + ": destination has modified, untracked or ignored files")
    entries = command(["git", "ls-files", "--stage", "-z"], directory, name + ": file-mode inspection")
    require(not any(entry.startswith(("120000 ", "160000 ")) for entry in entries.split("\0")), name + ": symlinks and submodules are not supported")


def validate_dependencies(root, rows):
    owners = [(root / row["directory"]).resolve() for row in rows]
    def path_values(value):
        if isinstance(value, dict):
            if "path" in value:
                yield value["path"]
            for child in value.values():
                yield from path_values(child)
        elif isinstance(value, list):
            for child in value:
                yield from path_values(child)
    count = 0
    for row in rows:
        repo = root / row["directory"]
        require((repo / "Cargo.toml").is_file() and (repo / "Cargo.lock").is_file(), row["directory"] + ": Cargo manifest/lock missing")
        for manifest in repo.rglob("Cargo.toml"):
            if ".git" in manifest.relative_to(repo).parts:
                continue
            require(not manifest.is_symlink(), row["directory"] + ": symlinked manifest unsupported")
            try:
                data = tomllib.loads(manifest.read_text())
            except (OSError, ValueError):
                raise BootstrapError(row["directory"] + ": invalid Cargo manifest") from None
            workspace, targets = data.get("workspace", {}), data.get("target", {})
            require(isinstance(workspace, dict) and isinstance(targets, dict), row["directory"] + ": invalid workspace/target tables")
            groups = [data, workspace] + list(targets.values())
            require(all(isinstance(group, dict) for group in groups), row["directory"] + ": invalid target table")
            for group in groups:
                for table in ("dependencies", "dev-dependencies", "build-dependencies", "patch", "replace"):
                    for path in path_values(group.get(table, {})):
                        require(isinstance(path, str) and not Path(path).is_absolute(), row["directory"] + ": relative dependency path required")
                        target = (manifest.parent / path).resolve()
                        require(any(target.is_relative_to(owner) for owner in owners) and (target / "Cargo.toml").is_file(), row["directory"] + ": dependency escapes or is missing from sibling layout")
                        count += 1
    return count


def fetch_repository(stage, row, source=None):
    directory = stage / row["directory"]
    directory.mkdir()
    label = row["directory"]
    command(["git", "init", "-q"], directory, label + ": initialization")
    command(["git", "remote", "add", "origin", canonical_remote(row)], directory, label + ": origin setup")
    # Existing clean sources are copied through Git objects into isolated clones;
    # no build runs in a user's existing checkout.
    remote = str(source) if source is not None else "origin"
    command(["git", "fetch", "--quiet", "--no-tags", "--depth", "1", remote, row["commit"]], directory, label + ": exact revision fetch")
    command(["git", "checkout", "--quiet", "--detach", row["commit"]], directory, label + ": detached checkout")
    inspect_repository(directory, row)


def bootstrap(value, destination, build=False):
    rows = validate_manifest(value)
    destination = Path(destination).absolute()
    existing = destination.exists() or destination.is_symlink()
    if existing:
        require(destination.is_dir() and not destination.is_symlink(), "destination must be a real directory")
        require({p.name for p in destination.iterdir()} == {row["directory"] for row in rows}, "existing destination must contain exactly the complete manifest; partial or unrelated files are never overwritten")
        for row in rows:
            inspect_repository(destination / row["directory"], row)
        validate_dependencies(destination, rows)
    destination.parent.mkdir(parents=True, exist_ok=True)
    result = {"format": FORMAT, "organization": "0xLattice", "passed": False, "existing_destination": existing, "repositories": rows, "checks": []}
    with tempfile.TemporaryDirectory(prefix=".lattice-bootstrap-", dir=destination.parent) as temporary:
        temporary = Path(temporary)
        stage = temporary / "sources"
        stage.mkdir()
        for row in rows:
            fetch_repository(stage, row, destination / row["directory"] if existing else None)
        result["path_dependency_edges"] = validate_dependencies(stage, rows)
        if build:
            env = os.environ.copy()
            env["CARGO_TARGET_DIR"] = str(temporary / "target")
            for row in rows:
                name = row["directory"]
                command(["cargo", "check", "--locked", "--workspace", "--all-targets"], stage / name, name + ": locked all-target build", env=env)
                result["checks"].append({"repository": name, "passed": True})
        for row in rows:
            inspect_repository(stage / row["directory"], row)
        if existing:
            for row in rows:
                inspect_repository(destination / row["directory"], row)
        else:
            require(not destination.exists() and not destination.is_symlink(), "destination appeared during bootstrap; nothing installed")
            # A concurrent nonempty directory makes rename fail, not overwrite.
            try:
                stage.rename(destination)
            except OSError:
                raise BootstrapError("destination installation failed; no user files overwritten") from None
        result["passed"] = True
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--destination", type=Path, required=True)
    parser.add_argument("--check", action="store_true", help="Locked native all-target checks in isolated checkouts")
    parser.add_argument("--report", type=Path, help="New output file only; never overwritten")
    args = parser.parse_args()
    # Reserve the report before network work; never replace an existing report.
    try:
        report = args.report.open("x") if args.report else None
    except OSError:
        print("cannot create a new report; nothing changed", file=__import__("sys").stderr)
        return 1
    with contextlib.ExitStack() as stack:
        if report:
            stack.enter_context(report)
        try:
            value = json.loads(args.manifest.read_text())
            result = bootstrap(value, args.destination, args.check)
        except (BootstrapError, OSError, ValueError) as error:
            detail = str(error) if isinstance(error, BootstrapError) else "bootstrap input or filesystem operation failed"
            result = {"passed": False, "error": detail + "; existing user files were not overwritten"}
            if report:
                json.dump(result, report, indent=2)
                report.write("\n")
            print(result["error"], file=__import__("sys").stderr)
            return 1
        if report:
            json.dump(result, report, indent=2)
            report.write("\n")
        print(json.dumps({"passed": result["passed"], "repositories": len(result["repositories"]), "checks": len(result["checks"])}))
        return 0


if __name__ == "__main__":
    raise SystemExit(main())
