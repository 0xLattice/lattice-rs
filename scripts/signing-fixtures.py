#!/usr/bin/env python3
"""Compile pinned Swift envelope/sign functions with Apple's CryptoKit backend."""
from pathlib import Path
import json, subprocess, sys, tempfile
repo=Path(__file__).resolve().parents[1]
up=Path(sys.argv[1]).resolve()
expected=json.loads((repo/'upstream-lock.json').read_text())['Lattice']
assert subprocess.check_output(['git','-C',str(up),'rev-parse','HEAD'],text=True).strip()==expected
source=up/'Sources/LatticePrimitives'
signing=(source/'Transaction/TransactionSigning.swift').read_text()
crypto=(source/'CryptoUtils.swift').read_text()
preimage=signing.split('    public static func preimage(bodyCID:',1)[1].split('    public static func preimage(body:',1)[0]
sign=crypto.split('    public static func sign(message:',1)[1].split('    // MARK: - Verification',1)[0]
payload=crypto.split('    static func signaturePayload(',1)[1].split('    private static func verifyWithMultikey',1)[0]
helpers=crypto.split('public extension Data {',1)[1]
with tempfile.TemporaryDirectory(prefix='lattice-signatures-') as tmp:
    p=Path(tmp)/'main.swift'
    p.write_text('''import Foundation
import CryptoKit
struct TransactionSigning {
    static let domain = "lattice-tx-v1"
    public static func preimage(bodyCID:''' + preimage + '''}
struct CryptoUtils {
    static let signatureDomain = "lattice-tx-v1:"
    public static func sign(message:''' + sign + '''
    static func signaturePayload(''' + payload + '''}
public extension Data {''' + helpers + '''
if CommandLine.arguments.count > 1 {
    let data = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
    let rows = try JSONSerialization.jsonObject(with: data) as! [[String:Any]]
    for row in rows {
        let raw = Data(hex: String((row["publicKey"] as! String).dropFirst(4)))!
        let key = try Curve25519.Signing.PublicKey(rawRepresentation: raw)
        let signature = Data(hex: row["signature"] as! String)!
        precondition(key.isValidSignature(signature, for: CryptoUtils.signaturePayload(row["preimage"] as! String)))
    }
    print("Swift verified \(rows.count) Rust signatures")
    exit(0)
}
var vectors:[[String:Any]]=[]
for path in [["Nexus"], ["Nexus","Payments"], ["Nexus","é","支付"], ["Nexus","a\\nb:c"]] {
    for nonce:UInt64 in [0,1,UInt64.max] {
        let cid="bafy-test-body"
        let preimage=TransactionSigning.preimage(bodyCID:cid,chainPath:path,nonce:nonce)
        let secret=String(repeating:"01",count:32)
        let key=try Curve25519.Signing.PrivateKey(rawRepresentation:Data(hex:secret)!)
        vectors.append(["path":path,"nonce":String(nonce),"bodyCID":cid,"preimage":preimage,
            "privateKey":secret,"publicKey":"ed01"+key.publicKey.rawRepresentation.hexString,
            "signature":CryptoUtils.sign(message:preimage,privateKeyHex:secret)!])
    }
}
print(String(data:try JSONSerialization.data(withJSONObject:vectors,options:[.sortedKeys,.prettyPrinted]),encoding:.utf8)!)
''')
    binary=str(Path(tmp)/'oracle')
    subprocess.run(['swiftc',str(p),'-o',binary],check=True)
    output=subprocess.check_output([binary],text=True)
    (repo/'crates/lattice-primitives/tests/swift-signing.json').write_text(output)
    print(f'Wrote {len(json.loads(output))} signature vectors')

    rust_trace = Path(tmp) / 'rust-signatures.json'
    with rust_trace.open('w') as out:
        subprocess.run(['cargo','run','--quiet','--locked','-p','lattice-primitives','--example','signing_trace','--',str(repo/'crates/lattice-primitives/tests/swift-signing.json')], cwd=repo, stdout=out, check=True)
    subprocess.run([binary, str(rust_trace)], check=True)
