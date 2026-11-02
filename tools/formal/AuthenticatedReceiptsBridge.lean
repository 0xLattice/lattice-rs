import LatticeProofs.MerkleBatchProofs
import LatticeProofs.MerkleWitness
import LatticeProofs.KeysProofs
open Lattice
open Lattice.Merkle
set_option linter.unusedSimpArgs false
set_option maxRecDepth 20000
set_option maxHeartbeats 8000000
namespace AuthenticatedReceiptsBridge
abbrev Rt := Root Nat String String
abbrev Tr := Trie Nat String String
abbrev Ks := Kids Nat String String
abbrev En := Enc Nat String String
mutual
def trieEq : Tr → Tr → Bool
 | .stub a, .stub b => a == b
 | .node p v ks, .node q w ls => p == q && v == w && kidsEq ks ls
 | _, _ => false
def kidsEq : Ks → Ks → Bool
 | .nil, .nil => true
 | .cons c t ks, .cons d u ls => c == d && trieEq t u && kidsEq ks ls
 | _, _ => false
end
def rootEq (a b : Rt) : Bool := a.count == b.count && kidsEq a.kids b.kids
def optionalRootEq : Option Rt → Option Rt → Bool
 | none, none => true
 | some a, some b => rootEq a b
 | _, _ => false
def encEq : En → En → Bool
 | .node p v ks, .node q w ls => p == q && v == w && ks == ls
 | .root n ks, .root m ls => n == m && ks == ls
 | _, _ => false
/-- Missing encodings never silently acquire an arbitrary valid digest. -/
def digest (table : List (En × String)) (e : En) : String :=
 ((table.find? fun row => encEq row.1 e).map (·.2)).getD "MISSING_ENCODING"
mutual
def knownTrie (h : En → String) : Tr → Bool
 | .stub _ => true
 | .node p v ks => knownKids h ks && h (.node p v (ks.digests h)) != "MISSING_ENCODING"
def knownKids (h : En → String) : Ks → Bool
 | .nil => true
 | .cons _ t rest => knownTrie h t && knownKids h rest
end
def knownRoot (h : En → String) (r : Rt) : Bool :=
 knownKids h r.kids && r.digest h != "MISSING_ENCODING"
def agreesDigest (h : En → String) (a b : Option Rt) : Bool :=
 (a.map (·.digest h)) == (b.map (·.digest h))

structure Output where
 tree : Rt
 cid : String
 balances : Lattice.Map String Nat
 receipts : List (List Nat × String)
structure Case where
 before : Rt
 beforeCID : String
 balances : Lattice.Map String Nat
 existing : Lattice.Map ReceiptKey Address
 actions : List Action
 witness : Option Rt
 result : Option Output

def mapEq [BEq K] [BEq V] (a b : List (K × V)) : Bool :=
 a.length == b.length && a.all (fun (k,v) => (b.find? (fun row => row.1 == k)).map (·.2) == some v)
def encodeKey (hashes : List (List Char × Nat)) (k : ReceiptKey) : List Nat :=
 (Keys.receiptPath (fun text => (hashes.lookup text).getD 0) k).map Char.toNat

/-- Both the value transition and receipt writes come from unchanged imports.
The finite encoding table connects their outputs to actual production CIDs. -/
def check (table : List (En × String)) (hashes : List (List Char × Nat)) (c : Case) : Bool := Id.run do
 let h := digest table
 let enc := encodeKey hashes
 let m := applyBlock "Alpha" [] ⟨c.balances,[],c.existing⟩ ⟨c.actions,0,none⟩
 let writes := receiptWrites enc c.actions
 if !(knownRoot h c.before && c.before.digest h == c.beforeCID &&
   c.before.count == c.existing.length && c.existing.all (fun (k,v) => c.before.toMap (enc k) == some v)) then return false
 match m, c.result with
 | none, none => return c.witness.isNone
 | some expected, some actual =>
   return mapEq expected.balances actual.balances &&
     mapEq (expected.receipts.map (fun (k,v) => (enc k,v))) actual.receipts &&
     knownRoot h actual.tree && actual.tree.digest h == actual.cid &&
     optionalRootEq (c.before.batch writes) (some actual.tree) &&
     optionalRootEq (c.before.applyAll writes) (some actual.tree) &&
     c.witness.any (fun witness => knownRoot h witness && witness.digest h == c.beforeCID &&
       (witness.batch writes).any (fun after => knownRoot h after && after.digest h == actual.cid))
 | _, _ => return false
end AuthenticatedReceiptsBridge
