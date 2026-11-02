import Foundation
import Lattice
import cashew
struct Input:Decodable {let name:String;let tip:String;let transaction:String;let entries:[String:[UInt8]];let checksPolicies:Bool;let chainPath:[String];let parentState:String?}
@main struct Oracle {
 static func main() async throws {
  let rows=try JSONDecoder().decode([Input].self,from:Data(contentsOf:URL(fileURLWithPath:CommandLine.arguments[1])))
  var results:[String:String]=[:]
  for row in rows {
   let source=InMemoryContentSource(row.entries.mapValues{Data($0)})
   let tip=try await BlockHeader(rawCID:row.tip).resolve(fetcher:source).node!
   let tx=try await HeaderImpl<Transaction>(rawCID:row.transaction).resolve(fetcher:source).node!
   let snapshot=TipBlockSnapshot(postStateCID:tip.postState.rawCID,prevStateCID:tip.prevState.rawCID,parentStateCID:tip.parentState.rawCID,specCID:tip.spec.rawCID,target:tip.target,nextTarget:tip.nextTarget,tipHeight:tip.height,timestamp:tip.timestamp)
   let result=await TransactionPreflight.classify(tx,tipCID:row.tip,snapshot:snapshot,chainPath:row.chainPath,parentState:row.parentState.map{LatticeStateHeader(rawCID:$0)},fetcher:source,validationContext:ValidationContext(nowMilliseconds:0),checksPolicies:row.checksPolicies)
   results[row.name]=String(describing:result.disposition)
  }
  for nonce:UInt128 in [UInt128(UInt64.max),UInt128(1)<<64,UInt128(1)<<80,UInt128.max] {
   let action=WithdrawalAction(withdrawer:"owner",nonce:nonce,demander:"owner",amountDemanded:1,amountWithdrawn:1)
   do {_ = try DagCBOR.encode(action);results["encoding-nonce-\(nonce)"]="encoded"}catch {results["encoding-nonce-\(nonce)"]="rejected"}
  }
  let e=JSONEncoder();e.outputFormatting=[.sortedKeys];print(String(decoding:try e.encode(results),as:UTF8.self))
 }
}
