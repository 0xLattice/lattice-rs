#!/usr/bin/env python3
"""Check production key encodings and child commitments with unchanged Lean models."""
import copy,datetime,hashlib,json,pathlib,subprocess
ROOT=pathlib.Path(__file__).resolve().parents[1]
FORMAL=ROOT.parent/'lattice-formal';LEAN=FORMAL/'lean';OUT=ROOT/'target/formal-keys-child'
PIN='a9f4c2879b6565ae11c17d35efe14475c5830978'
OUT.mkdir(parents=True,exist_ok=True)
started=datetime.datetime.now(datetime.timezone.utc).isoformat()
def report(value):
 p=OUT/'report.tmp';p.write_text(json.dumps(value,indent=2)+'\n');p.replace(OUT/'report.json')
report({'passed':False,'started':started})
def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def formal_pin():
 assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=FORMAL,text=True).strip()==PIN
 assert not subprocess.check_output(['git','status','--porcelain','--untracked-files=no'],cwd=FORMAL,text=True).strip()
def snapshot():
 paths=[pathlib.Path(__file__).resolve(),ROOT/'tools/formal/KeysChildBridge.lean',ROOT/'crates/lattice-validation/examples/formal_keys_child.rs',ROOT/'Cargo.lock',ROOT/'Cargo.toml']
 paths+=list(ROOT.glob('crates/*/Cargo.toml'))+list(ROOT.glob('crates/*/src/**/*.rs'))
 for name in ['cashew-rs','array-trie-rs']:
  repo=ROOT.parent/name;paths+=[repo/'Cargo.toml']+list(repo.glob('src/**/*.rs'))
 paths+=list(LEAN.glob('LatticeProofs/**/*.lean'))+[LEAN/'lean-toolchain',LEAN/'lakefile.toml',LEAN/'lake-manifest.json']
 return {str(p.relative_to(ROOT.parent)):sha(p) for p in sorted(set(paths)) if p.exists()}
formal_pin();inputs=snapshot()
obs=OUT/'observations.json'
with obs.open('w') as f:subprocess.run(['cargo','run','--locked','--quiet','-p','lattice-validation','--example','formal_keys_child'],cwd=ROOT,stdout=f,check=True)
data=json.loads(obs.read_text())
def verify_hashes(rows):
 for row in rows:
  expected=hashlib.sha256(('lattice/receipt-state/v1\0'+row['receipt']).encode()).hexdigest()
  assert expected==row['path'],'receipt SHA256 mismatch'
verify_hashes(data['keys'])
def txt(s):return '(['+','.join(str(ord(c)) for c in s)+'] : List Nat).map Char.ofNat'
def string(s):return '(String.ofList ('+txt(s)+'))'
def boolean(b):return 'true' if b else 'false'
def optional(s):return 'none' if s is None else '(some ('+txt(s)+'))'
def optbool(b):return 'none' if b is None else '(some '+boolean(b)+')'
def grammar(r):return f'grammar ({txt(r["text"])}) {boolean(r["visible"])} {boolean(r["directory"])} {boolean(r["account"])}'
def key(r):return f'key hashes ⟨{string(r["directory"])},{string(r["demander"])},{r["amount"]},{r["nonce"]}⟩ ({txt(r["deposit"])}) ({txt(r["receipt"])}) ({txt(r["path"])})'
def cid(r):return f'cid ({txt(r["text"])}) {boolean(r["standard"])}'
def pow_bytes(r):
 b=r['block'];values=[str(b['version']),optional(b.get('parent',{}).get('rawCID')),txt(b['transactions']['rawCID']),str(int(b['target'],16)),str(int(b['nextTarget'],16)),txt(b['spec']['rawCID']),txt(b['parentState']['rawCID']),txt(b['prevState']['rawCID']),txt(b['postState']['rawCID']),txt(b['children']['rawCID']),str(b['height']),'('+str(b['timestamp'])+')',optional(b.get('rewardRecipient'))]
 return 'powBytes ⟨'+','.join('('+v+')' for v in values)+'⟩ ['+','.join(map(str,r['bytes']))+']'
def carrier(r):
 actions='['+','.join('⟨('+txt(a['key'])+'),'+optional(a.get('oldValue'))+','+optional(a.get('newValue'))+'⟩' for a in r['actions'])+']'
 children='['+','.join('(('+txt(d)+'),('+txt(c)+'))' for d,c in r['children'])+']'
 return f'carrier ⟨({txt(r["previous"])}),{actions},{children},{boolean(r["wellFormed"])}⟩ {boolean(r["loaded"])} {optbool(r["commitment"])} {boolean(r["stateSucceeded"])}'
functions={'grammar':grammar,'keys':key,'cids':cid,'prefixes':pow_bytes,'carriers':carrier}
base=(ROOT/'tools/formal/KeysChildBridge.lean').read_text()+'\nopen KeysChildBridge\n'
rows={ 'lattice/receipt-state/v1\0'+r['receipt']:int(r['path'],16) for r in data['keys'] }
table='def hashes : List (Text × Nat) := ['+','.join('('+txt(k)+','+str(v)+')' for k,v in rows.items())+']\n'
base+=table
subprocess.run(['lake','build','LatticeProofs.KeysProofs','LatticeProofs.ChildCommitProofs'],cwd=LEAN,check=True,stdout=subprocess.DEVNULL)
def check(name,source):
 p=OUT/(name+'.lean');p.write_text(source)
 result=subprocess.run(['lake','env','lean',str(p)],cwd=LEAN,capture_output=True,text=True)
 (OUT/(name+'.log')).write_text(result.stdout+result.stderr)
 assert result.returncode==0,(name,result.stdout[-4000:],result.stderr[-1000:])
positive=base
for group,fn in functions.items():
 for i,row in enumerate(data[group]):positive+=f'-- {group} {i}\n#guard '+fn(row)+'\n'
check('positive',positive)
negative={}
def altered(name,group,row):
 check(name,base+'#guard !('+functions[group](row)+')\n');negative[name]='rejected by same Lean comparison'
r=copy.deepcopy(data['grammar'][65]);r['visible']=not r['visible'];altered('grammar','grammar',r)
r=copy.deepcopy(data['keys'][0]);r['path']='0'*64;altered('receipt-path','keys',r)
r=copy.deepcopy(data['cids'][0]);r['standard']=False;altered('cid-spelling','cids',r)
r=copy.deepcopy(data['prefixes'][0]);r['bytes'][0]^=1;altered('pow-prefix','prefixes',r)
r=copy.deepcopy(next(r for r in data['carriers'] if r['commitment'] is True));r['commitment']=False;altered('commitment','carriers',r)
r=copy.deepcopy(next(r for r in data['carriers'] if not r['stateSucceeded']));r['stateSucceeded']=True;altered('duplicate-key','carriers',r)
r=copy.deepcopy(next(r for r in data['carriers'] if r['commitment'] is None));r['commitment']=True;altered('missing-index','carriers',r)
r=copy.deepcopy(data['keys']);r[0]['receipt']+='!'
try:verify_hashes(r)
except AssertionError:negative['receipt-hash-input']='rejected by independent SHA256 check'
else:raise AssertionError('altered receipt input accepted')
assert snapshot()==inputs,'source changed during bridge run';formal_pin()
result={'passed':True,'started':started,'completed':datetime.datetime.now(datetime.timezone.utc).isoformat(),'formal_revision':PIN,'cases':{k:len(v) for k,v in data.items()},'negative_controls':negative,'observations_sha256':sha(obs),'inputs_sha256':inputs,'lean_version':subprocess.check_output(['lake','env','lean','--version'],cwd=LEAN,text=True).strip(),'rustc_version':subprocess.check_output(['rustc','--version'],text=True).strip()}
report(result);print(json.dumps({k:v for k,v in result.items() if k!='inputs_sha256'},indent=2))
