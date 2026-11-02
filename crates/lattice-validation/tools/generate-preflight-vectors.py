#!/usr/bin/env python3
"""Run unchanged pinned Swift preflight against actual Rust-generated CAS cases."""
import hashlib,json,pathlib,subprocess,sys,tempfile
crate=pathlib.Path(__file__).resolve().parents[1]
workspace=crate.parents[1]
reference=workspace.parent/'Lattice'
cache=pathlib.Path(sys.argv[1] if len(sys.argv)>1 else '/tmp/lattice-genesis-oracle')
revision='dda2d852429a27be6043599b63bde81530cd9514'
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=reference,text=True).strip()==revision
assert not subprocess.check_output(['git','status','--porcelain','--untracked-files=no'],cwd=reference,text=True).strip()
for original in (reference/'Sources').rglob('*.swift'):
 assert (cache/original.relative_to(reference)).read_bytes()==original.read_bytes(),f'changed Swift source: {original}'
resolved=hashlib.sha256((cache/'Package.resolved').read_bytes()).hexdigest()
assert resolved=='958f69bd3ec734a372e8cd94e3d09d593674a1025979a2abe2b45a6f08cfc9a6'
target=cache/'Sources/PreflightOracle';target.mkdir(exist_ok=True)
(target/'Oracle.swift').write_bytes((crate/'tests/preflight-oracle.swift').read_bytes())
p=cache/'Package.swift';s=p.read_text()
if 'name: "PreflightOracle"' not in s:
 s=s.replace('    targets: [\n','    targets: [\n        .executableTarget(name: "PreflightOracle", dependencies: ["Lattice", .product(name: "cashew", package: "cashew")]),\n',1);p.write_text(s)
with tempfile.TemporaryDirectory() as temp:
 path=pathlib.Path(temp)/'input.json'
 with path.open('w') as output:subprocess.run(['cargo','run','--quiet','-p','lattice-validation','--example','preflight_samples','--locked'],cwd=workspace,stdout=output,check=True)
 rows=json.loads(path.read_text())
 result=json.loads(subprocess.check_output(['swift','run','--package-path',str(cache),'--force-resolved-versions','PreflightOracle',str(path)]))
 for row in rows:
  assert result[row['name']]==row['expected'],(row['name'],row['expected'],result[row['name']])
  row['swift']=result[row['name']]
 data=json.dumps(rows,sort_keys=True,indent=2)+'\n';(crate/'tests/preflight-swift.json').write_text(data)
 evidence={'encoding_boundaries':{k:v for k,v in result.items() if k.startswith('encoding-')},'revision':revision,'cases':len(rows),'package_resolved_sha256':resolved,'fixture_sha256':hashlib.sha256(data.encode()).hexdigest(),'source_sha256':{}}
 for p in [reference/'Sources/LatticeImport/TransactionPreflight.swift',crate/'tests/preflight-oracle.swift',crate/'examples/preflight_samples.rs']:
  evidence['source_sha256'][str(p.relative_to(workspace.parent))]=hashlib.sha256(p.read_bytes()).hexdigest()
 (crate/'tests/preflight-swift-evidence.json').write_text(json.dumps(evidence,indent=2)+'\n')
 print(f'{len(rows)} actual Swift preflight verdicts match')
