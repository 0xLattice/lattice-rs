import LatticeProofs.KeysProofs
import LatticeProofs.ChildCommitProofs
set_option maxRecDepth 20000
set_option maxHeartbeats 8000000
namespace KeysChildBridge
open Lattice
abbrev Text := List Char

instance (s : Text) : Decidable (Keys.VisibleAscii s) := by unfold Keys.VisibleAscii; infer_instance
instance (s : Text) : Decidable (Keys.ValidDirectory s) := by unfold Keys.ValidDirectory; infer_instance
instance (s : Text) : Decidable (Keys.ValidAccount s) := by unfold Keys.ValidAccount; infer_instance
instance (b : ChildCommit.Carrier) : Decidable (ChildCommit.KeysDistinct b) := by unfold ChildCommit.KeysDistinct; infer_instance

def grammar (s : Text) (visible directory account : Bool) : Bool :=
 decide (Keys.VisibleAscii s) == visible &&
 decide (Keys.ValidDirectory s) == directory &&
 decide (Keys.ValidAccount s) == account

def hash (table : List (Text × Nat)) (input : Text) : Nat := (table.lookup input).getD 0

def key (table : List (Text × Nat)) (k : ReceiptKey) (deposit receipt path : Text) : Bool :=
 Keys.depositText ⟨k.demander, k.amountDemanded, k.nonce⟩ == deposit &&
 Keys.receiptText k == receipt &&
 (table.lookup (Keys.receiptPreimage k)).isSome &&
 Keys.receiptPath (hash table) k == path

def cid (s : Text) (accepted : Bool) : Bool := (Keys.cidDecode s).isSome == accepted

def powBytes (b : Keys.Header) (bytes : List Nat) : Bool :=
 (Keys.powPrefix b).map Char.toNat == bytes

/-- `loaded` is an acquisition boundary absent from the Boolean formal model.
The unloaded production API must return an error if a commitment needs the index.
For stateSucceeded, every action is valid and every old value matches the input
map, so duplicate touched keys are the only modeled transition failure. -/
def carrier (b : ChildCommit.Carrier) (loaded : Bool)
    (result : Option Bool) (stateSucceeded : Bool) : Bool :=
 stateSucceeded == decide (ChildCommit.KeysDistinct b) &&
 if loaded || (ChildCommit.commitments b.actions).isEmpty then
   result == some (ChildCommit.commitsValid b)
 else result.isNone
end KeysChildBridge
