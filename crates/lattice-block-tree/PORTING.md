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
now uses an AVL Euler index for subtree weights, routed membership and batched
ancestor enclosure. Producer discovery/anchoring still walks the graph, and
canonical projection plus authenticated preparation still have whole-graph/path
costs. Large-chain end-to-end performance parity is not claimed. Child restore must use `restore_with_parent` after restoring and serving
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


## Euler work index

`euler.rs` follows pinned Swift `EulerWorkIndex.swift`: stable block handles name
OPEN/CLOSE nodes, OPEN reads the single direct-work table, and subtree totals
are exact prefix differences. Leaf insertion and direct-work changes touch AVL
paths rather than chain ancestors. Disconnected components are toured and
spliced once when their parent becomes routed. Exclusion never deletes index
entries or subtracts work; it changes selection only. Unary canonical descent
avoids weight queries. Recovery builds a balanced index directly from the final
routed forest rather than replaying every rotation.

Rust uses `usize` handles and `BigUint` sums, rather than Swift's UInt32 links and
inline 320-bit/spilled representation. It adds no fixed numeric work ceiling.
Malformed splice forests are checked entirely before mutation, including
out-of-domain handles. These are internal derived structures, not persisted
facts or new network authority.

`python3 tools/generate-euler-vectors.py` compiles the unchanged pinned Swift
Euler source and checks that the linked WorkSum/FixedWidthWork arithmetic files
are byte-identical to the pin. It exports 330 deterministic observations covering
multiple roots, middle leaf insertions, orphan forests, strongest updates,
malformed splice refusal, bulk rebuilds, sums beyond 320 bits, single/batched
enclosure, full tour order and exact sequence-tree visit counts. Rust consumes
that fixture in `euler::tests::unchanged_pinned_swift_differential`. Separate
invariant tests check AVL balance, heights, parent links and aggregate values;
depth 200/400/800 counters assert logarithmic index work without wall-clock
thresholds. An independent adjacency graph oracle checks live grafting, work
upgrades, exclusions, canonical CID tie-breaking and shuffled durable recovery.

Limits: index mutation/query work is logarithmic in sequence size, not necessarily
in bit complexity of arbitrary-size sums. Graph metadata remains in string-keyed
maps; tours and canonical projection retain lookup/allocation costs. Producer
lookup and anchored-status checks remain scans/walks. `prepare_batches` still
clones the entire graph twice and compares whole basis structures, so this slice
does not make authenticated admission logarithmic. The index's linear recovery
builder does not make the entire journal replay and graph materialization linear.
Existing node recovery/formal traces test consensus observations separately;
this data-structure oracle is not a formal proof of the implementation.
