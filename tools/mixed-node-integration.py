#!/usr/bin/env python3
"""Run unchanged pinned Swift and production Rust root node hosts on localhost."""
import argparse,base64,copy,hashlib,json,os,pathlib,shutil,socket,subprocess,tempfile,time,urllib.request,urllib.error
ROOT=pathlib.Path(__file__).resolve().parents[1]; RESEARCH=ROOT.parent
GENESIS='bafyreiggtg4ezifboyekbf4gxst2jr3mpjqcxsbmopy7w6fp46g4ngpgxa'
def rpc(base,cookie,route,body=None):
 req=urllib.request.Request(base+route,data=None if body is None else json.dumps(body).encode(),headers={'Content-Type':'application/json','Authorization':'Basic '+base64.b64encode(cookie.read_bytes().strip()).decode()})
 try:
  with urllib.request.urlopen(req,timeout=10) as r:return r.status,json.load(r)
 except urllib.error.HTTPError as e:return e.code,e.read().decode()
def port():
 with socket.socket() as s:s.bind(('127.0.0.1',0));return s.getsockname()[1]
def launch(cmd,path,env=None):
 log=path.open('w');p=subprocess.Popen(cmd,stdout=log,stderr=subprocess.STDOUT,env=env);return p,log
def wait_for(fn,seconds=60):
 deadline=time.monotonic()+seconds;last=None
 while time.monotonic()<deadline:
  try:
   last=fn()
   if last:return last
  except (OSError,ValueError) as e:last=str(e)
  time.sleep(.1)
 raise RuntimeError(('timeout',last))
def main():
 parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--swift-bin',type=pathlib.Path,default=pathlib.Path('/tmp/lattice-node-mixed-build/debug/lattice-node'));args=parser.parse_args()
 out=ROOT/'target/mixed-node';out.mkdir(parents=True,exist_ok=True)
 report={'passed':False,'started_unix':time.time(),'scope':'actual Swift and Rust hosts, loopback only'};(out/'report.json').write_text(json.dumps(report,indent=2))
 processes=[]
 swift_source=RESEARCH/'lattice-node'
 assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=swift_source,text=True).strip()=='3df9e0a786c7b103a3da88d1c3881efc81706255'
 assert not subprocess.check_output(['git','status','--porcelain','--untracked-files=no'],cwd=swift_source,text=True).strip()
 def snapshot():
  paths=[pathlib.Path(__file__).resolve(),ROOT/'crates/lattice-validation/examples/nexus_genesis_volumes.rs',ROOT/'crates/lattice-validation/examples/mixed_node_signer.rs']
  for name in ['lattice-rs','lattice-node-rs','cashew-rs','array-trie-rs','trie-dictionary-rs','ivy-rs','tally-rs','volume-broker-rs']:
   repo=RESEARCH/name
   paths += list(repo.glob('Cargo.*'))+list(repo.glob('src/**/*.rs'))+list(repo.glob('crates/*/Cargo.toml'))+list(repo.glob('crates/*/src/**/*.rs'))
  paths += list(swift_source.glob('Sources/**/*.swift'))+[swift_source/'Package.swift',swift_source/'Package.resolved']
  return {str(p.relative_to(RESEARCH)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(set(paths))}
 inputs=snapshot()
 def stop(process):
  process.terminate()
  try:code=process.wait(timeout=15)
  except subprocess.TimeoutExpired:process.kill();process.wait();raise RuntimeError('shutdown timed out')
  assert code==0,code
 try:
  with (out/'build.log').open('w') as buildlog:
   subprocess.run(['swift','build','--package-path',str(swift_source),'--scratch-path',str(args.swift_bin.parent.parent),'--product','lattice-node','--disable-automatic-resolution'],stdout=buildlog,stderr=subprocess.STDOUT,check=True)
   subprocess.run(['cargo','build','--locked','--bin','lattice-node'],cwd=RESEARCH/'lattice-node-rs',stdout=buildlog,stderr=subprocess.STDOUT,check=True)
  def swift_dependencies():
   pins=json.loads((swift_source/'Package.resolved').read_text())['pins']
   checkouts=args.swift_bin.parent.parent/'checkouts'
   directories={p.name.lower():p for p in checkouts.iterdir() if p.is_dir()}
   result={}
   for pin in pins:
    directory=directories[pin['identity'].lower()]
    revision=subprocess.check_output(['git','rev-parse','HEAD'],cwd=directory,text=True).strip()
    assert revision==pin['state']['revision'],(directory,revision)
    assert not subprocess.check_output(['git','status','--porcelain','--untracked-files=no'],cwd=directory,text=True).strip(),directory
    result[pin['identity']]=revision
   return result
  dependency_revisions=swift_dependencies()


  subprocess.run(['cargo','build','--quiet','--locked','-p','lattice-validation','--example','mixed_node_signer'],cwd=ROOT,check=True)
  with tempfile.TemporaryDirectory(prefix='lattice-mixed-node-') as d:
   tmp=pathlib.Path(d);bootstrap=tmp/'bootstrap.json'
   with bootstrap.open('w') as f:subprocess.run(['cargo','run','--quiet','--locked','-p','lattice-validation','--example','nexus_genesis_volumes'],cwd=ROOT,stdout=f,check=True)
   binaries=tmp/'binaries';binaries.mkdir()
   rustbin=pathlib.Path(shutil.copy2(RESEARCH/'lattice-node-rs/target/debug/lattice-node',binaries/'rust-node'))
   swiftbin=pathlib.Path(shutil.copy2(args.swift_bin,binaries/'swift-node'))
   signer=pathlib.Path(shutil.copy2(ROOT/'target/debug/examples/mixed_node_signer',binaries/'signer'))
   report['binary_sha256']={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in [rustbin,swiftbin,signer]}
   identity=json.loads(subprocess.check_output([str(signer)],text=True));owner=identity['address']
   config={'directory':'rust','identityKey':'rust.key','genesisCid':GENESIS,'bootstrapFiles':['bootstrap.json'],'listen':'127.0.0.1:0','rpc':'127.0.0.1:0'}
   cfg=tmp/'node.json';cfg.write_text(json.dumps(config))
   r,log=launch([str(rustbin),'--config',str(cfg)],out/'rust.log');processes.append((r,log))
   def ready():
    assert r.poll() is None,(out/'rust.log').read_text()
    for line in (out/'rust.log').read_text().splitlines():
     if line.startswith('{'):
      value=json.loads(line)
      if 'rpc' in value:return value
   ready=wait_for(ready);rbase='http://'+ready['rpc'];rcookie=tmp/'rust/.cookie'
   sport,rpcport=port(),port();sbase=f'http://127.0.0.1:{rpcport}';scookie=tmp/'swift/.cookie'
   peer=ready['publicKey']+'@'+ready['overlay']
   env=os.environ.copy();env['LATTICE_FORBID_DEFAULT_PEERS']='1'
   swift_command=[str(swiftbin),'--data-directory',str(tmp/'swift'),'--identity-key',str(tmp/'swift.key'),'--listen-port',str(sport),'--rpc-port',str(rpcport),'--websocket-port','0','--no-default-peers','--peer',peer,'--peer-search-interval','0','--provider-walk-sessions','0']
   s,log=launch(swift_command,out/'swift.log',env);processes.append((s,log))
   wait_for(lambda:scookie.exists() and rpc(sbase,scookie,'/status')[0]==200)
   before={'rust':rpc(rbase,rcookie,'/status'),'swift':rpc(sbase,scookie,'/status')};report['before']=before
   assert all(code==200 and value['tipCID']==GENESIS and value['height']=='0' for code,value in before.values()),before
   assert before['rust'][1]['templateDigest']==before['swift'][1]['templateDigest']
   print(json.dumps(before),flush=True)
   def status(base,cookie):
    code,value=rpc(base,cookie,'/status');assert code==200,(code,value);return value
   def mine(label,base,cookie,recipient):
    code,template=rpc(base,cookie,'/mining/templates',{'recipients':[{'chainPath':['Nexus'],'address':recipient}]});assert code==200,(label,code,template)
    template_path=tmp/'template.json';template_path.write_text(json.dumps(template))
    work=json.loads(subprocess.check_output([str(signer),'--mine',str(template_path)],text=True))
    digest=hashlib.sha256(bytes(work['prefix'])+work['nonce'].to_bytes(8,'big')).digest()
    assert bytes(work['hash'])==digest and int.from_bytes(digest,'big')<=int(template['searchTarget'],16)
    code,result=rpc(base,cookie,'/mining/work',{'workID':template['workID'],'nonce':work['nonce']});assert code==200 and result['accepted'] and result['disposition']=='canonicalized',(label,code,result)
    report.setdefault('mined',[]).append({'label':label,'template':template,'work':work,'result':result})
    print(label,'submitted',result,flush=True)
    return result['tipCID']
   def same_tip(expected,height):
    def check():
     left,right=status(rbase,rcookie),status(sbase,scookie)
     return (left,right) if left.get('tipCID')==right.get('tipCID')==expected and left.get('height')==right.get('height')==str(height) else None
    left,right=wait_for(check,90)
    assert left['templateDigest']==right['templateDigest'],(left,right)
    return {'rust':left,'swift':right}
   def account(base,cookie,address,tip):
    code,value=rpc(base,cookie,'/accounts/'+address+'?block='+tip);assert code==200,(code,value);return value
   def same_accounts(tip,sender_balance,nonce,receiver_balance):
    result={}
    for address,balance,next_nonce in [(owner,sender_balance,nonce),(GENESIS,receiver_balance,0)]:
     left,right=account(rbase,rcookie,address,tip),account(sbase,scookie,address,tip)
     assert left==right and left['balance']==str(balance) and left['nonce']==str(next_nonce),(left,right,balance,next_nonce)
     result[address]=left
    return result
   expected=mine('fund-known-identity',rbase,rcookie,owner)
   report['funded']=same_tip(expected,1)
   report['payments']=[]
   for nonce,amount,label,base,cookie,other,other_cookie in [(0,17,'swift-to-rust',sbase,scookie,rbase,rcookie),(1,23,'rust-to-swift',rbase,rcookie,sbase,scookie)]:
    tx=json.loads(subprocess.check_output([str(signer),str(nonce),str(amount),GENESIS],text=True))
    bad=copy.deepcopy(tx);bad['transaction']['signatures'][identity['publicKey']]='00'*64
    negative=[]
    for endpoint,auth in [(base,cookie),(other,other_cookie)]:
     code,value=rpc(endpoint,auth,'/transactions',bad);assert 400<=code<500,(code,value);negative.append({'status':code,'body':value})
    code,submitted=rpc(base,cookie,'/transactions',tx);assert code==200,(code,submitted)
    report['pending_transaction']={'submitted':submitted,'local_status':status(base,cookie),'transaction_read':rpc(base,cookie,'/transactions/'+submitted['transactionCID'])}
    print('submitted tx',report['pending_transaction'],flush=True)
    def propagated():
     value=status(other,other_cookie);report['pending_transaction']['remote_status']=value;return value if value['mempoolCount']==1 else None
    pool=wait_for(propagated,30)
    # Mine on the other implementation, so the transaction must arrive via gossip.
    expected=mine(label,other,other_cookie,owner)
    sync=same_tip(expected,nonce+2)
    balances=same_accounts(expected,(nonce+2)*1048576-(17 if nonce==0 else 40),nonce+1,17 if nonce==0 else 40)
    assert all(state['mempoolCount']==0 for state in sync.values()),sync
    report['payments'].append({'direction':label,'transaction':tx,'rejected_signature':negative,'submitted':submitted,'remote_pool':pool,'synced':sync,'accounts':balances})
   def shutdown_all():
    for p,log in processes:stop(p);log.close()
    processes.clear()
   # Partition using actual stopped processes, never fabricated graph facts.
   stop(s);pair=next(pair for pair in processes if pair[0] is s);pair[1].close();processes.remove(pair)
   branch_a=mine('partition-rust-height4',rbase,rcookie,owner)
   a_account=account(rbase,rcookie,owner,branch_a)
   assert a_account['balance']==str(4*1048576-40),a_account
   shutdown_all()
   s,log=launch(swift_command,out/'swift-partition.log',env);processes.append((s,log))
   wait_for(lambda:status(sbase,scookie).get('tipCID')==expected)
   branch_b=mine('partition-swift-height4',sbase,scookie,GENESIS)
   assert branch_b!=branch_a
   winner=mine('partition-swift-height5',sbase,scookie,GENESIS)
   shutdown_all()
   config.update(bootstrapFiles=[],listen=ready['overlay'],rpc=ready['rpc']);cfg.write_text(json.dumps(config))
   r,log=launch([str(rustbin),'--config',str(cfg)],out/'rust-rejoin.log');processes.append((r,log))
   wait_for(lambda:status(rbase,rcookie).get('tipCID')==branch_a)
   s,log=launch(swift_command,out/'swift-rejoin.log',env);processes.append((s,log))
   rejoined=same_tip(winner,5)
   balances=same_accounts(winner,3*1048576-40,2,2*1048576+40)
   report['reorg']={'losing_tip':branch_a,'winning_branch_height4':branch_b,'winning_tip':winner,'losing_branch_sender':a_account,'rejoined':rejoined,'accounts':balances}
   shutdown_all()
   r,log=launch([str(rustbin),'--config',str(cfg)],out/'rust-restart.log');processes.append((r,log))
   s,log=launch(swift_command,out/'swift-restart.log',env);processes.append((s,log))
   report['restarted']=same_tip(winner,5)
   report['restarted_accounts']=same_accounts(winner,3*1048576-40,2,2*1048576+40)
   shutdown_all()
   assert swift_dependencies()==dependency_revisions
   report['swift_dependency_revisions']=dependency_revisions
   final_inputs=snapshot()
   assert final_inputs==inputs,('source changed during mixed-runtime run',[p for p in set(inputs)|set(final_inputs) if inputs.get(p)!=final_inputs.get(p)])
   report.update(passed=True,completed_unix=time.time(),inputs_sha256=inputs,swift_version=subprocess.check_output(['swift','--version'],text=True).strip(),rust_version=subprocess.check_output(['rustc','--version'],text=True).strip())
 except Exception as error:
  report['error']=str(error)
  raise
 finally:
  for p,log in processes:
   p.terminate()
   try:p.wait(timeout=15)
   except subprocess.TimeoutExpired:p.kill();p.wait()
   log.close()
  (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
if __name__=='__main__':main()
