import Foundation
import WasmKit
public struct WasmPolicyRef{static let currentABIVersion:UInt16=1;let abiVersion:UInt16;let entrypoint:String}
struct WasmPolicyModule{let bytes:Data}
struct WasmPolicyModuleHeader{let rawCID:String;init(node:WasmPolicyModule)throws{rawCID=node.bytes.base64EncodedString()}}
let cases=try JSONSerialization.jsonObject(with:Data(contentsOf:URL(fileURLWithPath:CommandLine.arguments[1]))) as! [[String:Any]]
let directory=URL(fileURLWithPath:CommandLine.arguments[2])
for c in cases{
 let name=c["name"] as! String
 let bytes=try Data(contentsOf:directory.appendingPathComponent(name+".wasm"))
 let policy=WasmPolicyRef(abiVersion:UInt16(c["abi"] as! Int),entrypoint:c["entrypoint"] as! String)
 let hex=Array((c["context_hex"] as! String).utf8);var context=Data()
 for i in stride(from:0,to:hex.count,by:2){context.append(UInt8(String(decoding:hex[i..<i+2],as:UTF8.self),radix:16)!)}
 func evaluate()->String{do{return try WasmPolicyEvaluator.evaluate(policy:policy,contextData:context,moduleBytes:bytes) ? "accept":"reject"}catch{return "error"}}
 let raw=evaluate()
 var result:[String:Any]
 do{try WasmPolicyEvaluator.validate(policy:policy,moduleBytes:bytes);result=["validation":"accept","evaluations":[evaluate(),evaluate()]]}catch{result=["validation":"error"]}
 let data=try JSONSerialization.data(withJSONObject:["result":result,"raw_evaluation":raw],options:[.sortedKeys])
 print(name+":"+String(decoding:data,as:UTF8.self))
}
