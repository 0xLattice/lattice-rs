#!/usr/bin/env python3
"""Check production Rust traces against pinned lattice-formal Lean models."""
import argparse
import copy
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent.parent
REVISION = "a9f4c2879b6565ae11c17d35efe14475c5830978"
SUITES = [
    ("lattice-pow", "asert_trace", "asert-vectors.json", "asertcheck", "nextTarget"),
    ("lattice-validation", "value_trace", "value-vectors.json", "tracecheck", "result"),
]


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def load_corpus(formal, corpus):
    """Bind local fixtures to the actual files in the pinned formal checkout."""
    provenance = json.loads((corpus.parent / "provenance.json").read_text())
    if (provenance["repository"] != "0xLattice/lattice-formal"
            or provenance["revision"] != REVISION
            or sha256(corpus) != provenance["sha256"]):
        raise ValueError(f"corpus provenance mismatch: {corpus}")
    sources = provenance.get("sources")
    if sources is None:
        sources = [{"path": provenance["path"], "sha256": provenance["sha256"]}]
    upstream = []
    for source in sources:
        path = (formal / source["path"]).resolve()
        if not path.is_relative_to(formal.resolve()) or sha256(path) != source["sha256"]:
            raise ValueError(f"upstream corpus provenance mismatch: {source['path']}")
        upstream.extend(json.loads(path.read_text()))
    expected = json.loads(corpus.read_text())
    if not expected or len(expected) != provenance["cases"] or expected != upstream:
        raise ValueError(f"corpus does not reproduce pinned upstream cases: {corpus}")
    return expected, provenance


def check_inputs(actual, expected, output):
    """A generator may replace outputs, never inputs, order, or case membership."""
    if not isinstance(actual, list) or len(actual) != len(expected):
        raise ValueError("formal case count mismatch")
    for index, (row, oracle) in enumerate(zip(actual, expected)):
        if not isinstance(row, dict) or set(row) != set(oracle):
            raise ValueError(f"formal case fields mismatch at {index}")
        if any(row[key] != value for key, value in oracle.items() if key != output):
            raise ValueError(f"formal input mismatch at {index}")


def normalize(state):
    if state is None:
        return None
    return {k: sorted(json.dumps(pair, sort_keys=True) for pair in pairs)
            for k, pairs in state.items()}


def run(formal, output_dir, report):
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=formal, text=True).strip()
    if revision != REVISION:
        raise ValueError(f"formal revision mismatch: {revision}")
    if subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=no"], cwd=formal):
        raise ValueError("formal checkout has tracked modifications")
    report["formal_revision"] = revision
    report["rustc"] = subprocess.check_output(["rustc", "--version"], cwd=ROOT, text=True).strip()
    report["cargo_lock_sha256"] = sha256(ROOT / "Cargo.lock")
    report["lean_toolchain"] = (formal / "lean/lean-toolchain").read_text().strip()
    # Check provenance before spending time building or running executables.
    corpora = []
    for package, example, name, checker, output in SUITES:
        corpus = ROOT / "crates" / package / "tests/formal" / name
        expected, provenance = load_corpus(formal, corpus)
        corpora.append((package, example, checker, output, corpus, expected, provenance))
    subprocess.run(["lake", "build", "LatticeProofs", "asertcheck", "tracecheck"], cwd=formal / "lean", check=True)
    for package, example, checker_name, output, corpus, expected, provenance in corpora:
        trace = output_dir / f"{example}.json"
        with trace.open("w") as out:
            subprocess.run(["cargo", "run", "--locked", "--quiet", "-p", package, "--example", example, "--", str(corpus)], cwd=ROOT, stdout=out, check=True)
        rows = json.loads(trace.read_text())
        check_inputs(rows, expected, output)
        checker = formal / "lean/.lake/build/bin" / checker_name
        result = subprocess.run([str(checker), str(trace)], capture_output=True, text=True, check=True)
        (output_dir / f"{example}.log").write_text(result.stdout + result.stderr)
        print(result.stdout, end="", flush=True)
        # Also require the exact recorded Swift outputs, normalizing map ordering.
        for index, (actual, oracle) in enumerate(zip(rows, expected)):
            lhs, rhs = actual[output], oracle[output]
            if output == "result":
                lhs, rhs = normalize(lhs), normalize(rhs)
            if lhs != rhs:
                raise ValueError(f"Swift result mismatch at {index}")
        mutants = []
        if output == "result":
            accepted = next(i for i, row in enumerate(rows) if row[output] is not None)
            rejected = next(i for i, row in enumerate(rows) if row[output] is None)
            mutants = [("reject-valid", accepted, None),
                       ("accept-invalid", rejected, rows[rejected]["prev"])]
        else:
            mutants = [("alter-target", 0, f'{(int(rows[0][output], 16) ^ 1):064x}')]
        controls = []
        for name, index, value in mutants:
            mutated = copy.deepcopy(rows)
            mutated[index][output] = value
            mutant = output_dir / f"mutant-{example}-{name}.json"
            mutant.write_text(json.dumps(mutated))
            result = subprocess.run([str(checker), str(mutant)], capture_output=True, text=True)
            (output_dir / f"mutant-{example}-{name}.log").write_text(result.stdout + result.stderr)
            if result.returncode != 1 or "model " not in result.stdout:
                raise ValueError(f"negative control did not fail as expected: {result.stdout} {result.stderr}")
            controls.append({"name": name, "case": index, "rejected": True})
            print(f"{example}: {name} negative control rejected by Lean", flush=True)
        report["suites"].append({
            "package": package, "example": example, "cases": len(rows),
            "corpus_sha256": provenance["sha256"], "trace_sha256": sha256(trace),
            "checker_sha256": sha256(checker), "swift_outputs_match": True,
            "negative_controls": controls,
        })


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("formal", type=Path, help="pinned lattice-formal checkout")
    parser.add_argument("--output-dir", type=Path, help="new directory for traces, logs, and report.json")
    args = parser.parse_args()
    report = {"status": "running", "started_at": datetime.now(timezone.utc).isoformat(),
              "scope": "ASERT and value trace conformance; not a proof of the whole Rust platform",
              "suites": []}
    # Never mix artifacts from two runs or leave an old success report on failure.
    with tempfile.TemporaryDirectory(prefix="lattice-rust-formal-") as tmp:
        output_dir = args.output_dir.resolve() if args.output_dir else Path(tmp)
        if args.output_dir:
            output_dir.mkdir(parents=True, exist_ok=False)
        try:
            run(args.formal.resolve(), output_dir, report)
            report["status"] = "passed"
        except Exception as error:
            report["status"] = "failed"
            report["error"] = str(error)
            raise
        finally:
            report["finished_at"] = datetime.now(timezone.utc).isoformat()
            (output_dir / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print("Trace conformance passed; this is not a proof of the whole Rust platform.")


if __name__ == "__main__":
    main()
