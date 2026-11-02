import Foundation
import LatticePrimitives
import LatticePoW
import LatticeValidation
import cashew
import UInt256
struct Anchor:Decodable {let height:UInt64;let timestamp:Int64;let target:UInt256}
struct Input:Decodable {
 let name:String;let mode:String;let spec:[UInt8];let previous:[UInt8]?;let parent_state:String?;let parent_chain:[UInt8]?
 let transactions:[[UInt8]];let children:[String:[UInt8]];let timestamp:Int64;let target:UInt256?;let next_target:UInt256?;let nonce:UInt64;let version:UInt16;let anchor:Anchor?;let recipient:String?;let entries:[String:[UInt8]]
}
struct Observation:Encodable {let built:Bool;var block:String?=nil;var post_state:String?=nil;var materialized:Bool?=nil;var diff:StateDiff?=nil}
@main struct Oracle {
 static func main() async throws {
  let rows=try JSONDecoder().decode([Input].self,from:Data(contentsOf:URL(fileURLWithPath:CommandLine.arguments[1])))
  var results:[String:Observation]=[:]
  for row in rows {
   do {
    let source=InMemoryContentSource(row.entries.mapValues{Data($0)})
    let spec=try DagCBOR.decode(ChainSpec.self,from:Data(row.spec))
    let txs=try row.transactions.map{try DagCBOR.decode(Transaction.self,from:Data($0))}
    let children=try row.children.mapValues{try DagCBOR.decode(Block.self,from:Data($0))}
    let result:BlockBuildResult
    if row.mode=="next" {
     let previous=try DagCBOR.decode(Block.self,from:Data(row.previous!))
     let carrier=try row.parent_chain.map{try DagCBOR.decode(Block.self,from:Data($0))}
     result=try await BlockBuilder.buildBlockWithTransition(previous:previous,transactions:txs,children:children,parentChainBlock:carrier,timestamp:row.timestamp,target:row.target,nextTarget:row.next_target,nonce:row.nonce,difficultyAnchor:row.anchor.map{DifficultyAnchor(blockHeight:$0.height,timestamp:$0.timestamp,target:$0.target)},rewardRecipient:row.recipient,fetcher:source)
    } else if row.mode=="child" {
     result=try await BlockBuilder.buildChildGenesisWithTransition(spec:spec,parentState:LatticeStateHeader(rawCID:row.parent_state!),transactions:txs,children:children,timestamp:row.timestamp,target:row.target!,nonce:row.nonce,version:row.version,fetcher:source)
    } else {
     result=try await BlockBuilder.buildGenesisWithTransition(spec:spec,transactions:txs,children:children,timestamp:row.timestamp,target:row.target!,nonce:row.nonce,version:row.version,fetcher:source)
    }
    results[row.name]=Observation(built:true,block:try BlockHeader(node:result.block).rawCID,post_state:result.block.postState.rawCID,materialized:result.materializedPostState != nil,diff:result.stateDiff)
   } catch {
    results[row.name]=Observation(built:false)
    FileHandle.standardError.write(Data("\(row.name): \(error)\n".utf8))
   }
  }
  let encoder=JSONEncoder();encoder.outputFormatting=[.sortedKeys]
  print(String(decoding:try encoder.encode(results),as:UTF8.self))
 }
}
