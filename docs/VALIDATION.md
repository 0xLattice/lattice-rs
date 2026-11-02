# Rust platform validation map

Reference models come from `0xLattice/lattice-formal` at
`a9f4c2879b6565ae11c17d35efe14475c5830978`. The model's proofs and exhaustive
bounded model checks are distinct from the finite traces emitted by Rust.
Neither a passing Swift differential corpus nor a passing model trace proves
whole-platform correctness.

| Production boundary | Executable evidence | Important limit |
|---|---|---|
| ASERT target arithmetic | `scripts/formal-check.py`: 2,703 observations through Lean Asert | Does not establish stored ancestor selection or admission time policy |
| Value transitions | Same harness: 650 observations through Lean value model and recorded Swift outputs | Excludes authenticated Merkle data, signatures, policies and nonces |
| Target/hash work and target comparison | `scripts/formal-work-check.py`: 4,337 observations, three corrupted outputs rejected | Does not check carrier selection, hashing or block admission |
| Durable node recovery | `lattice-node-rs/tools/formal-recovery-check.py`: production observations and negative controls | Does not implement every action of Recovery.Next or model every crash schedule |
| Parent-state continuity | `lattice-node-rs/tools/formal-continuity-check.py`: executed-parent observations and negative controls | Does not prove asynchronous child orchestration |
| Hierarchical work/fork choice | `crates/lattice-block-tree/tools/formal-hierarchy-check.py`: 72 events and ten negative controls | Reducer facts include fixture verdicts; not an end-to-end proof of admission |
| Cashew Merkle batch | `cashew-rs/tools/formal-merkle-check.py`: 385 production cases, 510 canonicality obligations, seven negative controls | Finite ASCII/u64 encoding table; does not prove global serialization, hashing, Unicode or encryption |
| Keys and child commitments | `tools/formal-keys-child-check.py`: 1,070 production observations and eight negative controls | Controlled action/state prerequisites; finite digest observations, not collision resistance or complete block validity |
| Authenticated receipt/value composition | `tools/formal-authenticated-receipts-check.py`: 128 production transitions, 168 canonicality obligations, 11 acquisition refusals and nine negative controls | Receipt-focused finite encoding/hash observations; not full state-root or signed-block refinement |
| Light-client witnesses | Separate lattice-light-client-rs Swift differential and adversarial checks | Proofs authenticate supplied commitments; they do not establish canonical chain membership |
| Ivy networking and sponsor orchestration | Swift differentials, native/browser/local HTTP tests | No lattice-formal network or sponsor refinement proof |

The detailed work scope is in [WORK_FORMAL.md](WORK_FORMAL.md), hierarchy scope in
[HIERARCHY_FORMAL.md](../crates/lattice-block-tree/docs/HIERARCHY_FORMAL.md), and
node recovery/continuity scope in the sibling node's PORTING.md. Key/commitment assumptions are in [KEYS_CHILD_FORMAL.md](KEYS_CHILD_FORMAL.md). The Cashew bridge
limits and its partial-proof difference are in the sibling
`cashew-rs/docs/FORMAL_MERKLE_BRIDGE.md`. Keep each
harness's report, source pins and hashes with the corresponding test results.
Do not count a model-only TLC run as production Rust refinement evidence.

The local [platform build audit](PLATFORM_BUILD.md) checks repository/path
completeness and locked native compilation. It cannot replace these semantic
checks. Likewise, compilation says nothing about live deployment, cross-machine
reproducibility or availability of unpublished sibling repositories.

The [authenticated receipt bridge](AUTHENTICATED_RECEIPTS_FORMAL.md) connects a
receipt-focused subset of value transitions to real CAS acquisition and Merkle
updates. Remaining formal connections include broader authenticated value/CAS composition,
complete recovery crash schedules, child derivation scheduling/liveness,
general key/hash refinement and child commitment proof composition. Hosted-child
orchestration and merged mining now have daemon evidence, but no full formal
refinement of their scheduling and admission composition.
