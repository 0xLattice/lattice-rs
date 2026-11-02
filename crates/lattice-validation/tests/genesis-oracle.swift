import Foundation
import LatticePrimitives
import LatticePoW
import LatticeValidation
import cashew
struct Input: Decodable { let name: String; let cid: String; let entries: [String:[UInt8]] }
struct Observation: Encodable {
    let valid: Bool
    var post_state: String? = nil
    var diff: StateDiff? = nil
}
@main struct Oracle {
    static func main() async throws {
        let input = try JSONDecoder().decode([Input].self, from: Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1])))
        var outputs: [String:Observation] = [:]
        for row in input {
            do {
                let source = InMemoryContentSource(row.entries.mapValues { Data($0) })
                let header = try await BlockHeader(rawCID: row.cid).resolve(fetcher: source)
                guard let block = header.node,
                      block.validateProofOfWork(nexusHash: block.proofOfWorkHash()) else {
                    outputs[row.name] = Observation(valid:false); continue
                }
                let (valid, diff, state) = try await block.validateGenesisTransition(
                    fetcher: source, chainPath: ["Nexus"], reportTemporalFailure: true,
                    validationContext: ValidationContext(nowMilliseconds: Int64.max))
                if valid {
                    // The final bootstrap gate reads the authenticated child index.
                    let children = try await block.children.resolve(fetcher: source)
                    guard children.node?.isWellFormed == true, let state else {
                        outputs[row.name] = Observation(valid:false); continue
                    }
                    outputs[row.name] = Observation(valid:true, post_state:try LatticeStateHeader(node:state).rawCID, diff:diff)
                } else { outputs[row.name] = Observation(valid:false) }
            } catch {
                // All cases supply complete CAS evidence; distinguish no unavailable classifications here.
                outputs[row.name] = Observation(valid:false)
                FileHandle.standardError.write(Data("\(row.name): \(error)\n".utf8))
            }
        }
        let encoder=JSONEncoder();encoder.outputFormatting=[.sortedKeys]
        print(String(decoding:try encoder.encode(outputs),as:UTF8.self))
    }
}
