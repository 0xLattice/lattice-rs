# Block tree port coverage

Ported from 0xLattice/Lattice `dda2d852429a27be6043599b63bde81530cd9514`,
`Sources/LatticeBlockTree`. This crate owns shared durable fact types and consensus
replay; the node repository owns their SQLite journal.

Implemented: Swift fact/ID codecs, immutable block metadata, positive globally
located grind work with monotone strongest credit, exact arbitrary-precision
subtree sums, GHOST descent with raw CID-byte tie break, irreversible validation
and exclusion, excluded work conservation, root pins, orphan grafts, replay
ordering and revision floors, executed-set ancestry/production, executed tip
fallback, parent run attribution with own-grind subtraction, state-matched child
routing and idempotent monotone derived work. Root schedule specs are bound to
content IDs; header linkage delegates to the shared validator and preserves retry
versus off-schedule versus excluded outcomes.

Ten graph invariant tests cover replay permutation, revision floor/reservation,
one-grind location and atomic reject, orphan graft, exclusion conservation,
execution ancestry, contradictory verdicts, run idempotency, CID alias normalization, and greater-than-256-bit child run recovery. The node crate's
original-Swift fixture generator verifies the shared durable codecs byte for byte.
These are not yet a full original-Swift graph differential suite.

The verified batch constructor is crate-private. Authenticated root/child header
preparation now derives real work/linkage and creates private PreparedAdmission
tokens; applying a token verifies its base has not changed. Public serde and replay exist for **locally authenticated
durable records**, never network admission. `header_linkage` is diagnostic linkage
only: it cannot mint verified facts and does not replace PoW, execution or policy.

Remaining: full authenticated execution/genesis/connect tier,
complete execution-frontier verdict APIs, Swift differential graph traces, exhaustive
malformed replay cases, and source performance behavior. The current Rust graph
uses adjacency traversal for subtree weights and ancestors; it does not implement
Swift's EulerWorkIndex logarithmic updates. Large-chain performance parity is not
claimed. Child restore must use `restore_with_parent` after restoring and serving
the parent, so derived work precedes final revision-floor sealing.

ChildBlockProof now includes exact binary serialization/deserialization, sealed
minimal CAS verification, parent-state bindings, root-most met-target pricing,
single-hop generation/composition, terminal hop regeneration, scheduling target
extraction and DirectChildEdge identity. VerifiedChildEvidence has private fields,
no Deserialize and binds the verified chain path. `ConnectJob` is privately derived
from held graph state; execution acceptance still awaits the composed validator.

`tools/generate-proof-wire.sh` compiles the actual pinned Swift struct fields and
wire methods to generate 100 byte-exact proof vectors, including UTF-8 lengths.
The only shim preserves opaque CID labels and defines original UInt16 wire bounds;
this checks the wire codec, not structural verification. Real CID proof tests
separately check exact consumed entries, malformed padding, composed paths,
inverted target pricing and terminal direct hops. Parent-state invalidity is
classified separately from malformed bytes.

The node's formal-recovery-check.py additionally drives this production reducer
and actual SQLite journal against the original pinned Recovery.tla via TLC: 101
observations and five rejecting negative controls. See the node PORTING.md for
scope limits (finite recovery traces, not whole-node admission/liveness proof).

Root genesis preparation now consumes the sealed `VerifiedRootGenesis` capability
from the composed validator, checks the configured CID pin, and derives exact
block/work/validation facts and state diff. The same immutable preparation token
is staged by NodeStorage before graph visibility. Node tests include unloaded-CAS
input, complete recursive persistence, SQL failure without graph advancement,
retry, and revalidation/recovery from the reopened database.

Private parent-continuity and child-anchor capabilities now prevent carrier-only
proofs or equality of unproduced states from asserting parent execution. Tests
cover executed noncanonical branches, backward/sideways refusals, unexecuted
producers, path mismatch, forged equal anchors and empty child-genesis anchors.
Child execution preparation composes these with the distinct composed child
validator capabilities and sealed securing-work evidence, comparing all path,
block, predecessor, state and difficulty-anchor bindings.
