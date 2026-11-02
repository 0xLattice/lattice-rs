#!/usr/bin/env python3
"""Compare Rust scan decisions with the exact pinned Swift allow-list and parser."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("lattice", type=Path)
parser.add_argument("wasmkit", type=Path)
args = parser.parse_args()
lattice, wasmkit = args.lattice.resolve(), args.wasmkit.resolve()
for path, revision in [(lattice, "dda2d852429a27be6043599b63bde81530cd9514"),
                       (wasmkit, "5c389084423c08136040ab8a41f73d4e76fb7b21")]:
    if subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=path, text=True).strip() != revision:
        raise SystemExit(f"revision mismatch: {path}")
    if subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=no"], cwd=path):
        raise SystemExit(f"tracked modifications: {path}")
source = lattice / "Sources/LatticeValidation/Policy/WasmPolicyDeterminismScan.swift"
cases = json.loads((ROOT / "crates/lattice-wasm/tests/scan-cases.json").read_text())
with tempfile.TemporaryDirectory(prefix="lattice-wasm-scan-") as tmp:
    work = Path(tmp)
    fixtures = work / "fixtures"
    fixtures.mkdir()
    subprocess.run(["cargo", "test", "--locked", "-p", "lattice-wasm", "--test", "scan"], cwd=ROOT,
                   env={**os.environ, "LATTICE_WASM_FIXTURES": str(fixtures)}, check=True)
    sources = work / "Sources/Oracle"
    sources.mkdir(parents=True)
    # Exact source copy, with only its surrounding error type supplied locally.
    shutil.copyfile(source, sources / source.name)
    (work / "Package.swift").write_text('''// swift-tools-version:6.0
import PackageDescription
let package = Package(name: "Oracle", platforms: [.macOS(.v14)],
    dependencies: [.package(url: "https://github.com/apple/swift-system", exact: "1.6.5"), .package(path: ''' + json.dumps(str(wasmkit)) + ''')],
    targets: [.executableTarget(name: "Oracle", dependencies: [
        .product(name: "WasmKit", package: "WasmKit-reference"),
        .product(name: "WasmParser", package: "WasmKit-reference")])])
''')
    (sources / "main.swift").write_text('''import Foundation
import WasmKit
import WasmParser
enum WasmPolicyError: Error { case nondeterministicConstruct(String) }
for path in CommandLine.arguments.dropFirst() {
    let bytes = Array(try Data(contentsOf: URL(fileURLWithPath: path)))
    do {
        try WasmPolicyDeterminismScan.scan(moduleBytes: bytes, features: [.referenceTypes])
        _ = try parseWasm(bytes: bytes, features: [.referenceTypes])
        print(URL(fileURLWithPath: path).deletingPathExtension().lastPathComponent + ":accept")
    } catch {
        print(URL(fileURLWithPath: path).deletingPathExtension().lastPathComponent + ":reject")
    }
}
''')
    subprocess.run(["swift", "build", "--package-path", str(work), "--product", "Oracle"], check=True)
    executable_dir = subprocess.check_output(["swift", "build", "--package-path", str(work), "--show-bin-path"], text=True).strip()
    result = subprocess.check_output([str(Path(executable_dir) / "Oracle")] + [str(fixtures / (r["name"] + ".wasm")) for r in cases], text=True)
    actual = dict(line.split(":") for line in result.splitlines())
    if len(actual) != len(cases):
        raise SystemExit("Swift oracle case count mismatch")
    for case in cases:
        expected = "accept" if case["accept"] else "reject"
        if actual[case["name"]] != expected:
            raise SystemExit(f"Swift/Rust disagreement: {case['name']}: Swift {actual[case['name']]}, Rust {expected}")
    evidence = {"lattice_revision": "dda2d852429a27be6043599b63bde81530cd9514",
                "wasmkit_revision": "5c389084423c08136040ab8a41f73d4e76fb7b21",
                "scanner_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
                "swift_version": subprocess.check_output(["swift", "--version"], text=True).strip(),
                "swift_dependencies": json.loads((work / "Package.resolved").read_text()),
                "scope": "opcode scan and parsing only; not runtime or complete module admission",
                "cases": [{"name": r["name"], "accept": r["accept"],
                           "wasm_sha256": hashlib.sha256((fixtures / (r['name'] + '.wasm')).read_bytes()).hexdigest()} for r in cases]}
    (ROOT / "crates/lattice-wasm/tests/swift-scan-evidence.json").write_text(json.dumps(evidence, indent=2) + "\n")
    print(f"{len(cases)} Swift/Rust scanner decisions agree")
