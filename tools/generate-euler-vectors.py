#!/usr/bin/env python3
"""Run unchanged pinned Swift EulerWorkIndex; export reproducible differential rows."""
import argparse,hashlib,json,pathlib,random,subprocess,tempfile
root=pathlib.Path(__file__).resolve().parents[1]
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--swift-build',type=pathlib.Path,default=pathlib.Path('/tmp/lattice-node-mixed-build'))
a=p.parse_args();reference=root.parent/'Lattice';node=root.parent/'lattice-node';pin='dda2d852429a27be6043599b63bde81530cd9514'
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=reference,text=True).strip()==pin
assert not subprocess.check_output(['git','diff','--name-only','--','Sources'],cwd=reference,text=True).strip()
source=reference/'Sources/LatticeBlockTree/EulerWorkIndex.swift'
out=root/'target/euler-swift';out.mkdir(parents=True,exist_ok=True)
with (out/'build.log').open('w') as log:
 subprocess.run(['swift','build','--package-path',str(node),'--scratch-path',str(a.swift_build),'--product','lattice-node','--disable-automatic-resolution'],stdout=log,stderr=subprocess.STDOUT,check=True)
# The node dependency can be an older package commit. The actual arithmetic
# files linked by this build must nevertheless be byte-identical to the pin.
math_sources={}
for name in ['WorkSum.swift','FixedWidthWork.swift']:
 pinned=reference/'Sources/LatticePoW'/name
 linked=a.swift_build/'checkouts/Lattice/Sources/LatticePoW'/name
 assert pinned.read_bytes()==linked.read_bytes(), 'linked work arithmetic differs from pin'
 math_sources[name]=hashlib.sha256(pinned.read_bytes()).hexdigest()
rng=random.Random(1739);rows=[]
def add(op,**fields):
 fields.update(op=op,query=rng.sample(range(180),8)+[0,1],pairs=[[rng.randrange(180),rng.randrange(180)] for _ in range(8)]+[[0,40],[1,40],[40,40]],ancestors=rng.sample(range(180),12)+[0],descendants=rng.sample(range(180),12))
 rows.append(fields)
add('root',h=0);add('root',h=1)
for h in range(2,62):add('leaf',h=h,parent=h-1 if h>2 else 0)
for h in range(62,112):add('leaf',h=h,parent=rng.randrange(h))
for h in range(112,132):add('add',h=h,delta=hex(1<<(h+220))[2:])
events=[]
for h in range(112,122):events.append([0,h])
for h in reversed(range(112,122)):events.append([1,h])
for h in range(122,132):events.extend([[0,h],[1,h]])
add('splice',parent=40,events=events)
for events in [[[0,132]],[[1,132]],[[0,132],[1,133]],[[0,132],[0,133],[1,132],[1,133]],[[0,132],[1,132],[0,132],[1,132]],[[0,40],[1,40]]]:add('splice',parent=0,events=events)
add('splice',parent=179,events=[[0,132],[1,132]])
add('root',h=0)
for i in range(180):
 if i%29==0:add('build')
 add('add',h=rng.randrange(180),delta=hex(rng.getrandbits(rng.choice([8,128,256,321,450])))[2:])
add('splice',parent=1,events=[[0,132],[0,133],[1,133],[1,132],[0,134],[1,134]])
add('build')
objects=[p for p in (a.swift_build/'debug/lattice-node.product/Objects.LinkFileList').read_text().splitlines() if '/LatticeNodeDaemon.build/' not in p]
modulemaps=list((a.swift_build/'debug').glob('*.build/module.modulemap'))
modulemaps += [p for p in (a.swift_build/'checkouts').glob('*/Sources/*/include/module.modulemap') if p.parent.parent.name not in {q.parent.name.removesuffix('.build') for q in modulemaps}]
with tempfile.TemporaryDirectory(prefix='euler-oracle-') as tmp:
 tmp=pathlib.Path(tmp);(tmp/'main.swift').write_bytes((root/'tools/EulerOracle.swift').read_bytes());(tmp/'EulerWorkIndex.swift').write_bytes(source.read_bytes());(tmp/'objects.txt').write_text('\n'.join(objects))
 command=['swiftc','-D','DEBUG','-package-name','lattice','-I',str(a.swift_build/'debug/Modules')]
 for path in modulemaps:command+=['-Xcc','-fmodule-map-file='+str(path)]
 command += [str(tmp/'EulerWorkIndex.swift'),str(tmp/'main.swift'),'@'+str(tmp/'objects.txt'),'-lsqlite3','-lz','-o',str(tmp/'oracle')]
 with (out/'compile.log').open('w') as log:subprocess.run(command,stdout=log,stderr=subprocess.STDOUT,check=True)
 expected=json.loads(subprocess.check_output([str(tmp/'oracle')],input=json.dumps(rows).encode()))
fixture={'sourceCommit':pin,'sourceSHA256':hashlib.sha256(source.read_bytes()).hexdigest(),'mathSourceSHA256':math_sources,'rows':rows,'expected':expected}
(root/'crates/lattice-block-tree/tests/fixtures/swift-euler.json').write_text(json.dumps(fixture,separators=(',',':'))+'\n')
print(json.dumps({'observations':len(rows),'sourceCommit':pin,'sourceSHA256':fixture['sourceSHA256']}))
