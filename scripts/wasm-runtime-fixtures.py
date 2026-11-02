#!/usr/bin/env python3
"""Compare pinned Swift eager policy admission and execution to Rust's public ABI."""
import argparse,hashlib,json,os,shutil,subprocess,tempfile
from pathlib import Path
ROOT=Path(__file__).resolve().parent.parent
p=argparse.ArgumentParser(description=__doc__);p.add_argument('lattice',type=Path);p.add_argument('wasmkit',type=Path);a=p.parse_args();lattice=a.lattice.resolve();wasmkit=a.wasmkit.resolve()
revisions={lattice:'dda2d852429a27be6043599b63bde81530cd9514',wasmkit:'5c389084423c08136040ab8a41f73d4e76fb7b21'}
for path,revision in revisions.items():
 if subprocess.check_output(['git','rev-parse','HEAD'],cwd=path,text=True).strip()!=revision:raise SystemExit(f'Revision mismatch: {path}')
 if subprocess.check_output(['git','status','--porcelain','--untracked-files=no'],cwd=path):raise SystemExit(f'Tracked modifications: {path}')
casefile=ROOT/'crates/lattice-wasm/tests/runtime-cases.json';cases=json.loads(casefile.read_text());sources=lattice/'Sources/LatticeValidation/Policy'
with tempfile.TemporaryDirectory(prefix='lattice-runtime-oracle-') as tmp:
 work=Path(tmp);fixtures=work/'fixtures';fixtures.mkdir();src=work/'Sources/Oracle';src.mkdir(parents=True)
 subprocess.run(['cargo','test','--locked','-p','lattice-wasm','--test','runtime_corpus'],cwd=ROOT,env={**os.environ,'LATTICE_RUNTIME_FIXTURES':str(fixtures)},check=True)
 (work/'Package.swift').write_text('''// swift-tools-version:6.0
import PackageDescription
let package=Package(name:"Oracle",platforms:[.macOS(.v14)],dependencies:[.package(url:"https://github.com/apple/swift-system",exact:"1.6.5"),.package(path:'''+json.dumps(str(wasmkit))+''')],targets:[.executableTarget(name:"Oracle",dependencies:[.product(name:"WasmKit",package:"WasmKit-reference"),.product(name:"WasmParser",package:"WasmKit-reference")])])
''')
 # Retain the exact public runtime enum, removing only the first async fetch-and-encode overload.
 original=(sources/'WasmPolicy.swift').read_text();errors=original[original.index('public enum WasmPolicyError'):original.index('private struct WasmPolicyContextCanonicalEncoder')];runtime=original[original.index('public enum WasmPolicyEvaluator'):];start=runtime.index('    public static func evaluate(');end=runtime.index('    public static func evaluate(',start+1);runtime=runtime[:start]+runtime[end:]
 (src/'WasmPolicyRuntime.swift').write_text('import Foundation\n@_spi(Fuzzing) import WasmKit\nimport WasmParser\n'+errors+runtime)
 (src/'WasmModuleCache.swift').write_text((sources/'WasmModuleCache.swift').read_text().replace('import cashew\n',''))
 shutil.copyfile(sources/'WasmPolicyDeterminismScan.swift',src/'WasmPolicyDeterminismScan.swift')
 # This supplies the PolicyRef ABI fields and replaces the cache-only CID key with exact bytes.
 shutil.copyfile(ROOT/'crates/lattice-wasm/tests/runtime-oracle.swift',src/'main.swift')
 subprocess.run(['swift','build','--package-path',str(work),'--product','Oracle'],check=True)
 binary=Path(subprocess.check_output(['swift','build','--package-path',str(work),'--show-bin-path'],text=True).strip())/'Oracle'
 result=subprocess.check_output([str(binary),str(casefile),str(fixtures)],text=True)
 actual={name:json.loads(data)for name,data in(line.split(':',1)for line in result.splitlines())}
 if len(actual)!=len(cases):raise SystemExit('Oracle case count mismatch')
 for c in cases:
  name=c['name'];rust=json.loads((fixtures/(name+'.json')).read_text())
  if actual[name]['result']!=rust:raise SystemExit(f'Swift/Rust runtime mismatch: {name}: Swift {actual[name]}, Rust {rust}')
  c['swift_result']=actual[name]['result'];c['raw_swift_evaluation']=actual[name]['raw_evaluation']
 evidence={'lattice_revision':revisions[lattice],'wasmkit_revision':revisions[wasmkit],'source_sha256':{n:hashlib.sha256((sources/n).read_bytes()).hexdigest()for n in ['WasmPolicy.swift','WasmModuleCache.swift','WasmPolicyDeterminismScan.swift']},'swift_version':subprocess.check_output(['swift','--version'],text=True).strip(),'swift_dependencies':json.loads((work/'Package.resolved').read_text()),'scope':'Exact pinned Swift runtime validate/evaluate/instantiate versus Rust WasmPolicy::new/evaluate twice. Context bytes supplied directly, no content-authentication/encoding comparison. Swift source extraction excludes async fetch/context encoder; cache key is collision-free base64 bytes in place of Cashew CID only. Eager validation is required before comparing execution.','cases':[{'name':c['name'],'wasm_sha256':hashlib.sha256((fixtures/(c['name']+'.wasm')).read_bytes()).hexdigest(),**actual[c['name']]}for c in cases]}
 casefile.write_text(json.dumps(cases,indent=2)+'\n');(ROOT/'crates/lattice-wasm/tests/swift-runtime-evidence.json').write_text(json.dumps(evidence,indent=2)+'\n')
 print(f'{len(cases)} pinned Swift/Rust policy runtime cases agree')
