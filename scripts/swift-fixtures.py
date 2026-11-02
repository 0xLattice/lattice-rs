#!/usr/bin/env python3
"""Run pinned upstream arithmetic in a minimal Swift harness, not a reimplementation.

Usage: python3 scripts/swift-fixtures.py /path/to/lattice-rust-research
Uses the actual UInt256 package and unmodified function bodies from Lattice.
Only ChainSpec's two scheduling fields are stubbed; this is not a full block oracle.
"""
import json
from pathlib import Path
import random
import subprocess
import sys
import tempfile

root = Path(sys.argv[1]).resolve()
repo = Path(__file__).resolve().parents[1]
lock = json.loads((repo / 'upstream-lock.json').read_text())
for name, sha in lock.items():
    actual = subprocess.check_output(['git', '-C', str(root / name), 'rev-parse', 'HEAD'], text=True).strip()
    if actual != sha:
        raise SystemExit(f'{name}: expected {sha}, got {actual}')
pow_dir = root / 'Lattice/Sources/LatticePoW'
work_functions = (pow_dir / 'ProofOfWork.swift').read_text().split('public func workForTarget', 1)[1].split('/// The block the difficulty', 1)[0]
asert = (pow_dir / 'ChainSpec+Asert.swift').read_text().replace('import LatticePrimitives', '')
maximum = (1 << 256) - 1
rng = random.Random(0x1A771CE)
cases = []
# Explicit extreme timestamps, large divisors, zero schedules, saturation, and
# 16.16 fractional transitions; ordinary random cases cannot reach these edges.
for target in [0, 1, 2, maximum // 2, maximum]:
    for timestamp in [-(1 << 63), -1, 0, 1, 500, 1000, 1001, 2000, (1 << 63) - 1]:
        cases.append([f'{target:064x}', 0, 1, timestamp, 2, 1000, 1])
for height in [1, 2, 239, 240, 241, 256, (1 << 64)-1]:
    cases.append([f'{maximum:064x}', 0, 1, 0, height, 1000, 1])
for period, half in [(0, 1), (1, 0), ((1<<64)-1, (1<<64)-1)]:
    cases.append([f'{maximum:064x}', -(1<<63), 0, (1<<63)-1, (1<<64)-1, period, half])
for _ in range(250):
    cases.append([f'{rng.getrandbits(256):064x}', rng.randrange(-100000, 100000),
                  1, rng.randrange(-100000, 100000), rng.randrange(1, 500),
                  rng.randrange(1, 10000), rng.randrange(1, 200)])
with tempfile.TemporaryDirectory(prefix='lattice-swift-oracle-') as temp:
    directory = Path(temp)
    source = directory / 'Sources/Oracle'
    source.mkdir(parents=True)
    (directory / 'Package.swift').write_text('''// swift-tools-version: 6.0
import PackageDescription
let package = Package(name: "Oracle", dependencies: [
    .package(path: %s)
], targets: [.executableTarget(name: "Oracle", dependencies: ["UInt256"])])
''' % json.dumps(str(root / 'UInt256')))
    (source / 'Asert.swift').write_text('public struct ChainSpec { let targetBlockTime: UInt64; let halfLife: UInt64 }\n' + asert)
    (source / 'Work.swift').write_text('import UInt256\npublic func workForTarget' + work_functions)
    prefix_source = (pow_dir / 'ProofOfWork.swift').read_text().split('public extension Block {', 1)[1].split('    func proofOfWorkHash()', 1)[0]
    (source / 'Prefix.swift').write_text('''import Foundation
import UInt256
public struct Ref { let rawCID: String }
public struct Block {
    let version: UInt16
    let parent: Ref?
    let transactions: Ref
    let target: UInt256
    let nextTarget: UInt256
    let spec: Ref
    let parentState: Ref
    let prevState: Ref
    let postState: Ref
    let children: Ref
    let height: UInt64
    let timestamp: Int64
    let rewardRecipient: String?
}
public extension Block {
''' + prefix_source + '\n}\n')
    rows = []
    for t, ats, ah, ts, h, period, half in cases:
        rows.append(f'emit("{t}", {ats}, {ah}, {ts}, {h}, {period}, {half})')
    (source / 'main.swift').write_text('''import Foundation
import UInt256
func target(_ hex: String) -> UInt256 {
    var words: [UInt64] = []
    for start in stride(from: 0, to: 64, by: 16) {
        let a = hex.index(hex.startIndex, offsetBy: start)
        let b = hex.index(a, offsetBy: 16)
        words.append(UInt64(hex[a..<b], radix: 16)!)
    }
    return UInt256(words)
}
func emit(_ hex: String, _ ats: Int64, _ ah: UInt64, _ ts: Int64, _ h: UInt64, _ period: UInt64, _ half: UInt64) {
    let value = target(hex)
    let result = ChainSpec(targetBlockTime: period, halfLife: half).calculateAsertTarget(
        anchorTarget: value, anchorTimestamp: ats, anchorHeight: ah, blockTimestamp: ts, blockHeight: h)
    print(result.toHexString() + " " + workForTarget(value).toHexString() + " " + workForHash(value).toHexString())
}
''' + '\n'.join(rows) + '''
for recipient: String? in [nil, "", "ed01abcd", "é/支付"] {
    let block = Block(version: 65535, parent: nil, transactions: Ref(rawCID: "tx"),
        target: target(String(repeating: "f", count: 64)), nextTarget: UInt256(1),
        spec: Ref(rawCID: "spec"), parentState: Ref(rawCID: "parent"),
        prevState: Ref(rawCID: "prev"), postState: Ref(rawCID: "post"),
        children: Ref(rawCID: "children"), height: UInt64.max, timestamp: Int64.min, rewardRecipient: recipient)
    print(Block.makeProofOfWorkPreimagePrefix(block: block).map { String(format: "%02x", $0) }.joined())
}
''')
    output = subprocess.check_output(['swift', 'run', '--package-path', temp, '-c', 'release', 'Oracle'], text=True)
    lines = output.splitlines()
    assert len(lines) == len(cases) + 4, (len(lines), len(cases))
    vectors = [dict(anchor=t, anchor_timestamp=ats, anchor_height=ah, timestamp=ts, height=h,
                    target_block_time=period, half_life=half, asert=line.split()[0],
                    work_for_target=line.split()[1], work_for_hash=line.split()[2])
               for (t, ats, ah, ts, h, period, half), line in zip(cases, lines)]
    destination = repo / 'crates/lattice-pow/tests/swift-vectors.json'
    destination.write_text(json.dumps({'upstream': lock, 'vectors': vectors, 'prefixes': lines[len(cases):]}, indent=2) + '\n')
    print(f'Wrote {len(vectors)} Swift-derived vectors to {destination}')
