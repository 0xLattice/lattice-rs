#!/usr/bin/env python3
"""Regenerate compiled Swift WasmKit stack-profile differential evidence."""
import argparse,hashlib,json,os,shutil,subprocess,tempfile
from pathlib import Path
ROOT=Path(__file__).resolve().parent.parent
parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('wasmkit',type=Path);args=parser.parse_args();wasmkit=args.wasmkit.resolve()
revision='5c389084423c08136040ab8a41f73d4e76fb7b21'
if subprocess.check_output(['git','rev-parse','HEAD'],cwd=wasmkit,text=True).strip()!=revision:raise SystemExit('WasmKit revision mismatch')
if subprocess.check_output(['git','status','--porcelain','--untracked-files=no'],cwd=wasmkit):raise SystemExit('WasmKit tracked modifications')
casefile=ROOT/'crates/lattice-wasm/tests/stack-profile-cases.json';cases=json.loads(casefile.read_text())
with tempfile.TemporaryDirectory(prefix='lattice-stack-oracle-') as tmp:
 work=Path(tmp);fixtures=work/'fixtures';fixtures.mkdir();sources=work/'Sources/Oracle';sources.mkdir(parents=True)
 subprocess.run(['cargo','test','--locked','-p','lattice-wasm','--lib','stack_profile'],cwd=ROOT,env={**os.environ,'LATTICE_STACK_FIXTURES':str(fixtures)},check=True)
 shutil.copyfile(ROOT/'crates/lattice-wasm/tests/stack-profile-oracle.swift',sources/'main.swift')
 (work/'Package.swift').write_text('''// swift-tools-version:6.0
import PackageDescription
let package=Package(name:"Oracle",platforms:[.macOS(.v14)],dependencies:[.package(url:"https://github.com/apple/swift-system",exact:"1.6.5"),.package(path:'''+json.dumps(str(wasmkit))+''')],targets:[.executableTarget(name:"Oracle",dependencies:[.product(name:"WasmKit",package:"WasmKit-reference")])])
''')
 subprocess.run(['swift','build','--package-path',str(work),'--product','Oracle'],check=True)
 binary=Path(subprocess.check_output(['swift','build','--package-path',str(work),'--show-bin-path'],text=True).strip())/'Oracle'
 result=subprocess.check_output([str(binary)]+[str(fixtures/(c['name']+'.wasm'))for c in cases],text=True)
 actual={name:json.loads(value)for name,value in (line.split(':',1)for line in result.splitlines())}
 if len(actual)!=len(cases):raise SystemExit('Oracle case count mismatch')
 for case in cases:
  name=case['name'];rust=json.loads((fixtures/(name+'.json')).read_text())
  if actual[name]!=rust:raise SystemExit(f'Swift/Rust disagreement: {name}: Swift {actual[name]}, Rust {rust}')
  case['swift_profile']=actual[name]
 evidence={'wasmkit_revision':revision,'translator_sha256':hashlib.sha256((wasmkit/'Sources/WasmKit/Translator.swift').read_bytes()).hexdigest(),'swift_version':subprocess.check_output(['swift','--version'],text=True).strip(),'swift_dependencies':json.loads((work/'Package.resolved').read_text()),'scope':'Compiled WasmKit InstructionSequence.maxStackHeight and emitted call spAddend sequence; not a full runtime refinement proof','cases':[{'name':c['name'],'wasm_sha256':hashlib.sha256((fixtures/(c['name']+'.wasm')).read_bytes()).hexdigest(),'profile':actual[c['name']]}for c in cases]}
 casefile.write_text(json.dumps(cases,indent=2)+'\n');(ROOT/'crates/lattice-wasm/tests/swift-stack-evidence.json').write_text(json.dumps(evidence,indent=2)+'\n')
 print(f'{len(cases)} compiled Swift/Rust stack profiles agree')
