import Foundation
struct Vector:Codable {let root:String;let path:[String];let entries:[[String]];let wire:String}
@main struct Generate {
 static func main() throws {
  var rows:[Vector]=[]
  for i in 0..<100 {
   let path=(0..<(i%6)).map { "Directory\($0)"+(i%3==0 ? "é" : "") }
   let entries=(0..<(i%9)).map { (cid:"cid/\($0)",data:Data((0..<(i+$0)).map {UInt8($0%256)})) }.reversed()
   let proof=ChildBlockProof(rootCID:"root/\(i)",directoryPath:path,entries:Array(entries))
   rows.append(Vector(root:proof.rootCID,path:path,entries:proof.entries.map {[$0.cid,$0.data.map{String(format:"%02x",$0)}.joined()]},wire:try proof.serialize().map{String(format:"%02x",$0)}.joined()))
  }
  let encoder=JSONEncoder();encoder.outputFormatting=[.sortedKeys,.withoutEscapingSlashes];print(String(data:try encoder.encode(rows),encoding:.utf8)!)
 }
}
