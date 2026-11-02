#!/usr/bin/env python3
"""Run full pinned Lattice validation sources, with an added same-package executable.
No consensus source is extracted, replaced, or edited. The driver calls the actual public BlockBuilder entrypoints. No admission
validation is applied; template behavior is intentionally distinct.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

repo = Path(__file__).resolve().parents[1]
upstream = Path(sys.argv[1] if len(sys.argv)>1 else repo.parent / 'lattice').resolve()
sha = 'dda2d852429a27be6043599b63bde81530cd9514'
assert subprocess.check_output(['git','-C',str(upstream),'rev-parse','HEAD'],text=True).strip()==sha
assert not subprocess.check_output(['git','-C',str(upstream),'status','--porcelain','--untracked-files=no'],text=True).strip()
work=Path(os.environ.get('LATTICE_BUILDER_WORK', tempfile.mkdtemp(prefix='lattice-builder-oracle-')))
work.mkdir(parents=True,exist_ok=True)
for name in ['Sources','Tests']:
    shutil.copytree(upstream/name,work/name,dirs_exist_ok=True)
shutil.copy2(upstream/'Package.resolved',work/'Package.resolved')
manifest=(upstream/'Package.swift').read_text()
manifest=manifest.replace('\n    targets: [','\n    targets: [\n        .executableTarget(name: "BuilderOracle", dependencies: ["LatticePrimitives", "LatticePoW", "LatticeValidation", .product(name: "cashew", package: "cashew")]),',1)
(work/'Package.swift').write_text(manifest)
source=work/'Sources/BuilderOracle';source.mkdir(exist_ok=True)
shutil.copy2(repo/'crates/lattice-validation/tests/builder-oracle.swift',source/'Oracle.swift')
input_file=work/'input.json'
subprocess.run(['cargo','test','-p','lattice-validation','--test','builder_swift'],cwd=repo,env={**os.environ,'LATTICE_BUILDER_EXPORT':str(input_file)},check=True)
run=subprocess.run(['swift','run','--package-path',str(work),'--force-resolved-versions','BuilderOracle',str(input_file)],text=True,capture_output=True)
if run.returncode:
    sys.stderr.write(run.stderr)
    raise SystemExit(run.returncode)
outputs=json.loads(run.stdout)
rows=json.loads(input_file.read_text())
for row in rows: row['swift']=outputs[row['name']]
fixture=repo/'crates/lattice-validation/tests/builder-swift.json'
fixture.write_text('[\n'+',\n'.join(json.dumps(row,separators=(',',':')) for row in rows)+'\n]\n')
evidence={'upstream_sha':sha,'sources':'Full unchanged Sources/LatticePrimitives, LatticePoW, LatticeValidation; same-package executable driver.',
 'oracle_diagnostics':run.stderr,
 'scope':'Actual buildGenesisWithTransition/buildChildGenesisWithTransition/buildBlockWithTransition. Compares template CID, post-state CID, materialization presence, and StateDiff. No admission or import persistence.',
 'swift_version':subprocess.check_output(['swift','--version'],text=True).strip(),
 'package_resolved':{**json.loads((work/'Package.resolved').read_text()),'pins':[{k:v for k,v in pin.items() if k!='location'} for pin in json.loads((work/'Package.resolved').read_text())['pins']]},
 'privacy_note':'Dependency locations omitted; package identities, revisions, versions and validation observations unchanged.',
 'source_sha256':{str(p.relative_to(upstream)):hashlib.sha256(p.read_bytes()).hexdigest() for directory in ['LatticePrimitives','LatticePoW','LatticeValidation'] for p in sorted((upstream/'Sources'/directory).rglob('*.swift'))},
 'fixture_sha256':hashlib.sha256(fixture.read_bytes()).hexdigest(),'cases':len(rows)}
(repo/'crates/lattice-validation/tests/builder-swift-evidence.json').write_text(json.dumps(evidence,indent=2)+'\n')
subprocess.run(['cargo','test','-p','lattice-validation','--test','builder_swift'],cwd=repo,check=True)
print(f'{len(rows)} actual pinned Swift builder cases agree; harness retained at {work}')
