import Foundation
@testable import WasmKit
for path in CommandLine.arguments.dropFirst(){
 do {
  let bytes=Array(try Data(contentsOf:URL(fileURLWithPath:path)))
  let module=try parseWasm(bytes:bytes,features:[.referenceTypes])
  let store=Store(engine:Engine(configuration:EngineConfiguration(threadingModel:.token)))
  let instance=try module.instantiate(store:store)
  var profiles:[[String:Any]]=[]
  var i=0
  while let f=instance.exports[function:"f\(i)"]{
   let seq=try f.handle.wasm.ensureCompiled(store:StoreRef(store))
   var calls:[Int]=[]
   var pc=seq.baseAddress
   while pc<seq.baseAddress+seq.instructions.count{
    let instruction=Instruction.load(from:&pc)
    switch instruction{case .call(let op),.compilingCall(let op),.internalCall(let op):calls.append(Int(op.spAddend));case .callIndirect(let op):calls.append(Int(op.spAddend));default:break}
   }
   profiles.append(["max":seq.maxStackHeight,"calls":calls]);i+=1
  }
  let data=try JSONSerialization.data(withJSONObject:profiles,options:[.sortedKeys])
  print(URL(fileURLWithPath:path).deletingPathExtension().lastPathComponent+":"+String(decoding:data,as:UTF8.self))
 }catch{print(URL(fileURLWithPath:path).deletingPathExtension().lastPathComponent+":\"error\"")}
}
