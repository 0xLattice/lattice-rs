#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
source="$root/../../../Lattice/Sources/LatticeProofs/ChildBlockProof.swift"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
python3 - "$source" "$tmp" <<'PY'
import pathlib,sys
source=pathlib.Path(sys.argv[1]).read_text();out=pathlib.Path(sys.argv[2])
fields=source[source.index('public struct ChildBlockProof:'):source.index('    // MARK: - Generation')]
wire=source[source.index('    public func serialize()'):source.index('// MARK: - Internal helpers')]
helpers=source[source.index('private func writeU16'):]
shim='''import Foundation
public enum ChildProofSerializationError:Error {case valueTooLarge}
enum CIDIdentity {static func canonicalString(_ s:String)->String? {s}}
enum ChildProofWireLimits {static let maximumDepth=Int(UInt16.max);static let maximumDirectoryBytes=Int(UInt16.max)}
'''
out.joinpath('Wire.swift').write_text(shim+fields+wire+helpers)
PY
swiftc -O "$tmp/Wire.swift" "$root/tools/GenerateProofWire.swift" -o "$tmp/generate"
"$tmp/generate" > "$root/tests/fixtures/swift-proof-wire.json"
