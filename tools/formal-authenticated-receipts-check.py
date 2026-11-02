#!/usr/bin/env python3
"""Finite unchanged-Lean checks of production authenticated receipt composition."""
import base64, copy, datetime, hashlib, json, pathlib, subprocess
ROOT=pathlib.Path(__file__).resolve().parents[1]
FORMAL=ROOT.parent/'lattice-formal'; LEAN=FORMAL/'lean'; OUT=ROOT/'target/formal-authenticated-receipts'
PIN='a9f4c2879b6565ae11c17d35efe14475c5830978'
OUT.mkdir(parents=True,exist_ok=True)
started=datetime.datetime.now(datetime.timezone.utc).isoformat()
def report(value):
 p=OUT/'report.tmp';p.write_text(json.dumps(value,indent=2)+'\n');p.replace(OUT/'report.json')
report({'passed':False,'started':started})
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def snapshot():
 paths=[pathlib.Path(__file__).resolve(),ROOT/'tools/formal/AuthenticatedReceiptsBridge.lean',ROOT/'crates/lattice-validation/examples/formal_authenticated_receipts.rs',ROOT/'Cargo.lock',ROOT/'Cargo.toml']
 paths+=list(ROOT.glob('crates/*/Cargo.toml'))+list(ROOT.glob('crates/*/src/**/*.rs'))
 for name in ['cashew-rs','array-trie-rs','trie-dictionary-rs','multikey-rs']:
  repo=ROOT.parent/name;paths+=list(repo.glob('Cargo.*'))+list(repo.glob('src/**/*.rs'))
 paths+=list(LEAN.glob('LatticeProofs/**/*.lean'))+[LEAN/'lean-toolchain',LEAN/'lakefile.toml',LEAN/'lake-manifest.json']
 return {str(p.relative_to(ROOT.parent)):sha(p) for p in sorted(set(paths))}
def pin():
 assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=FORMAL,text=True).strip()==PIN
 assert not subprocess.check_output(['git','status','--porcelain','--untracked-files=no'],cwd=FORMAL,text=True).strip()
pin();inputs=snapshot()
obs=OUT/'observations.json'
with obs.open('w') as stream:subprocess.run(['cargo','run','--locked','--quiet','-p','lattice-validation','--example','formal_authenticated_receipts'],cwd=ROOT,stdout=stream,check=True)
data=json.loads(obs.read_text())
# Independent decoder for exactly this finite unencrypted DAG-CBOR subset.
def decode(raw):
 pos=0
 def read(n):
  nonlocal pos
  assert pos+n<=len(raw); value=raw[pos:pos+n];pos+=n;return value
 def item():
  byte=read(1)[0];major,extra=byte>>5,byte&31
  if extra<24:n=extra
  else:
   assert extra in [24,25,26,27];width=1<<(extra-24);n=int.from_bytes(read(width),'big');assert n>=24 and (width==1 or n>=1<<(8*(width//2)))
  if major==0:return n
  if major==3:return read(n).decode('utf8')
  if major==4:return [item() for _ in range(n)]
  if major==5:
   result={}
   for _ in range(n):
    key=item();assert key not in result;result[key]=item()
   return result
  raise AssertionError(('unexpected CBOR kind',major,extra))
 value=item();assert pos==len(raw);return value

def encodings(rows):
 seen={}
 for row in rows:
  raw=bytes(row['bytes']);cid='b'+base64.b32encode(bytes([1,0x71,0x12,0x20])+hashlib.sha256(raw).digest()).decode().lower().rstrip('=')
  assert cid==row['cid'],'CID mismatch'
  enc=row['enc'];variant='node' if 'node' in enc else 'root'
  if variant=='root':count,children=enc[variant];expected={'count':count}
  else:
   prefix,value,children=enc[variant];expected={'prefix':bytes(prefix).decode('ascii')}
   if value is not None:expected['value']=value
  expected['children']=[{'key':chr(k),'value':{'rawCID':v}} for k,v in children]
  assert decode(raw)==expected,'abstract encoding differs from actual DAG-CBOR'
  if cid in seen:assert seen[cid]==enc,'finite trie hash collision'
  seen[cid]=enc
encodings(data['encodings'])
hashes={}
for c in data['cases']:
 for a in c['actions']+[v['action'] for v in c['existing']]:
  text=f'{a["directory"]}/{a["demander"]}/{a["amount"]}/{a["nonce"]}'
  preimage='lattice/receipt-state/v1\0'+text
  assert a['text']==text and hashlib.sha256(preimage.encode()).hexdigest()==a['path']
  hashes[preimage]=int(a['path'],16)
assert len(set(hashes.values()))==len(hashes),'finite receipt hash collision'
def boundaries(rows):
 assert rows and all(not row['accepted'] and row['error'] for row in rows),'unauthenticated acquisition accepted'
boundaries(data['boundaries'])
def string(s):return json.dumps(s)
def chars(xs):return '['+','.join(map(str,xs))+']'
def val(v):return 'none' if v is None else '(some '+string(v)+')'
def kids(xs):
 r='.nil'
 for k,t in reversed(xs):r=f'(.cons {k} {trie(t)} {r})'
 return r
def trie(t):
 if 'stub'in t:return '(.stub '+string(t['stub'])+')'
 p,v,ks=t['node'];return f'(.node {chars(p)} {val(v)} {kids(ks)})'
def root(r):return f'⟨{r["count"]},{kids(r["kids"])}⟩'
def optroot(r):return 'none' if r is None else '(some ('+root(r)+' : Rt))'
def links(xs):return '['+','.join(f'({k},{string(cid)})'for k,cid in xs)+']'
def encoding(e):
 if 'root'in e:n,ks=e['root'];return f'(.root {n} {links(ks)})'
 p,v,ks=e['node'];return f'(.node {chars(p)} {val(v)} {links(ks)})'
def key(a):return f'⟨{string(a["directory"])},{string(a["demander"])},{a["amount"]},{a["nonce"]}⟩'
def action(a):return f'(.receipt {string(a["withdrawer"])} {a["nonce"]} {string(a["demander"])} {a["amount"]} {string(a["directory"])})'
def balances(m):return '['+','.join(f'({string(k)},{v})'for k,v in m.items())+']'
def output(r):
 if r is None:return 'none'
 receipts='['+','.join('('+chars(k.encode())+','+string(v)+')'for k,v in r['receipts'].items())+']'
 return f'(some ⟨{root(r["tree"])},{string(r["cid"])},{balances(r["accounts"])},{receipts}⟩)'
def case(c,name):
 existing='['+','.join('('+key(v['action'])+','+string(v['value'])+')'for v in c['existing'])+']'
 return f'def {name} : Case := ⟨{root(c["before"])},{string(c["beforeCID"])},{balances(c["accounts"])},{existing},[{",".join(action(a)for a in c["actions"])}],{optroot(c["proof"])},{output(c["result"])}⟩\n'
base=(ROOT/'tools/formal/AuthenticatedReceiptsBridge.lean').read_text()+'\nopen AuthenticatedReceiptsBridge\n'
base+='def table : List (En × String) := ['+','.join('('+encoding(r['enc'])+','+string(r['cid'])+')'for r in data['encodings'])+']\n'
base+='def hashes : List (List Char × Nat) := ['+','.join('('+string(k)+'.toList,'+str(v)+')'for k,v in hashes.items())+']\n'
# Avoid source mutation: generated test programs import pinned models in place.
subprocess.run(['lake','build','LatticeProofs.KeysProofs','LatticeProofs.MerkleBatchProofs'],cwd=LEAN,stdout=subprocess.DEVNULL,check=True)
def check(name,source):
 p=OUT/(name+'.lean');p.write_text(source)
 result=subprocess.run(['lake','env','lean',str(p)],cwd=LEAN,text=True,capture_output=True)
 (OUT/(name+'.log')).write_text(result.stdout+result.stderr)
 assert result.returncode==0,(name,result.stdout[-5000:],result.stderr[-1000:])
body=base
canon='Root.Canon,Kids.Canon,Trie.Canon,Trie.HeadIs,Kids.keys,Kids.size,Trie.size,Kids.length'
for i,c in enumerate(data['cases']):
 body+=case(c,'case'+str(i))+f'#guard check table hashes case{i}\n'
 body+=f'example : case{i}.before.Canon := by simp [case{i},{canon}]\n'
 if c['result'] is not None:
  body+=f'example : ({root(c["result"]["tree"])} : Rt).Canon := by simp [{canon}]\n'
check('positive',body)
negative={}
def altered(name,c):
 check(name,base+case(c,'changed')+'#guard !(check table hashes changed)\n');negative[name]='rejected by same unchanged-model comparison'
valid=next(c for c in data['cases'] if c['result'] is not None and c['actions'])
c=copy.deepcopy(valid);c['result']['accounts']['withdrawer']+=1;altered('invented-balance',c)
c=copy.deepcopy(valid);k=next(iter(c['result']['receipts']));c['result']['receipts'][k]='forged-owner';altered('wrong-receipt-value',c)
c=copy.deepcopy(valid);c['result']['cid']=c['beforeCID'];altered('wrong-post-root',c)
c=copy.deepcopy(valid);c['proof']=None;altered('missing-witness',c)
c=copy.deepcopy(valid);c['actions'][0]['amount']+=1;altered('changed-demand',c)
c=copy.deepcopy(next(c for c in data['cases'] if c['result'] is None));c['result']=copy.deepcopy(valid['result']);altered('accepted-invalid-transition',c)
for name,kind in [('cbor-bytes','bytes'),('abstract-encoding','enc')]:
 rows=copy.deepcopy(data['encodings'])
 if kind=='bytes':rows[0]['bytes'][0]^=1
 else:rows[0]['enc']={'root':[999,[]]}
 try:encodings(rows)
 except AssertionError:negative[name]='independent CBOR/encoding/CID check rejected'
 else:raise AssertionError(name+' accepted')
rows=copy.deepcopy(data['boundaries']);rows[0]['accepted']=True
try:boundaries(rows)
except AssertionError:negative['accepted-corrupt-cas']='acquisition boundary checker rejected'
else:raise AssertionError('accepted altered acquisition')
assert snapshot()==inputs,'source changed during run';pin()
result={'passed':True,'started':started,'completed':datetime.datetime.now(datetime.timezone.utc).isoformat(),'formal_revision':PIN,'cases':len(data['cases']),'successful':sum(c['result']is not None for c in data['cases']),'encodings':len(data['encodings']),'acquisition_refusals':len(data['boundaries']),'canonicality_obligations':len(data['cases'])+sum(c['result']is not None for c in data['cases']),'negative_controls':negative,'observations_sha256':sha(obs),'inputs_sha256':inputs,'lean_version':subprocess.check_output(['lake','env','lean','--version'],cwd=LEAN,text=True).strip(),'rustc_version':subprocess.check_output(['rustc','--version'],text=True).strip()}
report(result);print(json.dumps({k:v for k,v in result.items()if k!='inputs_sha256'},indent=2))
