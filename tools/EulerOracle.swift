import Foundation
import LatticePoW
struct Work: DirectWorkSource {
    var values = Array(repeating: WorkSum.zero, count: 512)
    func addDirectWork(of block: UInt32, to total: inout WorkTotal) { total.add(values[Int(block)]) }
}
let rows = try JSONSerialization.jsonObject(with: FileHandle.standardInput.readDataToEndOfFile()) as! [[String:Any]]
var work = Work()
var index = EulerWorkIndex.empty
var output: [[String:Any]] = []
func number(_ row: [String:Any], _ key:String)->UInt32 { (row[key] as! NSNumber).uint32Value }
for row in rows {
    var result: Any = NSNull()
    switch row["op"] as! String {
    case "root": result = index.insertRoot(number(row,"h"), in:work)
    case "leaf": result = index.insertLeaf(number(row,"h"),under:number(row,"parent"),in:work) as Any? ?? NSNull()
    case "add":
        let h = number(row,"h")
        work.values[Int(h)] = work.values[Int(h)] + WorkSum(hex:row["delta"] as! String)!
        result = index.directWorkChanged(at:h,in:work) as Any? ?? NSNull()
    case "splice":
        let events = (row["events"] as! [[Int]]).map { $0[0] == 0 ? EulerWorkIndex.Event.open(UInt32($0[1])) : .close(UInt32($0[1])) }
        result = index.splice(events,under:number(row,"parent"),in:work) as Any? ?? NSNull()
    case "build":
        index = EulerWorkIndex.build(events:index.debugSequence.map { $0.isOpen ? .open($0.block) : .close($0.block) },in:work)
    default: fatalError("unknown operation")
    }
    let query = row["query"] as! [UInt32]
    let pairs = row["pairs"] as! [[UInt32]]
    output.append([
        "result":result,
        "weights":query.map { index.subtreeWork($0,in:work)?.toHexString() as Any? ?? NSNull() },
        "visits":query.map { index.subtreeWorkVisiting($0,in:work)?.visits as Any? ?? NSNull() },
        "encloses":pairs.map { index.encloses($0[0],$0[1]) as Any? ?? NSNull() },
        "any":index.anyEncloses(row["ancestors"] as! [UInt32],row["descendants"] as! [UInt32]),
        "sequence":index.debugSequence.map { [$0.isOpen ? 0:1,Int($0.block)] },
        "valid":index.debugInvariantsHold(in:work)
    ])
}
FileHandle.standardOutput.write(try JSONSerialization.data(withJSONObject:output,options:[.sortedKeys]))
