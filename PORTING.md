# Platform port coverage

Reference: `0xLattice/Lattice@dda2d852429a27be6043599b63bde81530cd9514`.
Formal reference: `0xLattice/lattice-formal@a9f4c2879b6565ae11c17d35efe14475c5830978`.
The complete platform remains the objective; this record describes current evidence,
not a release or completion claim.

## Implemented and checked

- `lattice-pow`: current preimage, hashing, target/work, growable work, ASERT;
  305 actual Swift arithmetic cases, four prefix cases, 2,703 formal cases.
- `lattice-primitives`: initial actions, identities, ChainSpec arithmetic, signing,
  transaction signature-array representation, Block and Volume-backed state types.
  Block genesis CID and PoW hash match the current Swift serialization goldens.
  Twelve signing envelopes are checked in both Swift/Rust verification directions.
- `lattice-validation`: exact coinbase and balance arithmetic, account nonce floors,
  sorted contiguous first-signer sequences, general-state byte equality, receipt
  insertion, parent-receipt authorization, permanent spent-deposit markers, atomic
  state execution and account-growth accounting. Signature/action-shape admission
  distinguishes genesis from non-genesis. Admission composition follows below.
- Root-genesis validation now composes authenticated block/spec resolution, own
  PoW, temporal deferral, transaction admission, policy checks, commitments,
  bounded content, premine, growth and exact post-state recomputation. Success
  returns `VerifiedRootGenesis` with private fields and no deserializer, bound to
  the original block CID and retaining materialized state/spec/body content.
  Tests cover the pinned Swift genesis CID, real premine credit, incorrect roots,
  growth, invalid configured programs and invalid/unavailable check ordering.
  A full unchanged pinned Swift package also supplies 31 differential genesis
  cases; verdicts, accepted post-state CIDs and state-diff multiplicities agree.
- Root non-genesis execution composes predecessor/spec/state linkage, difficulty
  and clock rules, signatures, block limits, path/child commitments, WASM policies,
  coinbase and exact authenticated state execution. `VerifiedRootTransition`
  binds the predecessor and supplied/resolved anchors. It is conditional evidence:
  graph import must independently require held nonexcluded headers, executed
  ancestry and its own matching anchor. Tests cover three successive heights,
  missing anchors, invalid linkage/commitments, temporal deferral and coinbase.
- Child genesis and successor execution now use separate private result types,
  sharing authenticated transition checks while binding the positional path and
  both parent-state endpoints. They intentionally require graph import to supply
  securing-work evidence and executed parent-state continuity; child own-hash
  PoW is not required. Withdrawal preflight authenticates deposits and directory-
  scoped parent receipts before fee validation. Initial tests cover these bindings,
  wrong-directory receipts, duplicate withdrawals and missing content. Full child
  import integration and differential validation remain in progress.
- Block templates now build genesis/child genesis and successor blocks, with
  exact transaction-index numbering, CID-only child commitments, height/spec-
  checked difficulty-anchor traversal, explicit target overrides, coinbase,
  growth checks and materialized state diffs. Reference CPU nonce search uses
  the committed PoW prefix. Templates are not admission capabilities: like the
  Swift builder, construction does not authenticate signatures/policies or
  parent receipts. Tests reproduce the genesis CID, validate a rewarded successor,
  check its state diff, and distinguish missing ancestry from invalid linkage.
  Thirty-two full-source Swift builder cases match exact block/post-state CIDs,
  materialization flags and state diffs, including deliberately non-admissible
  templates. These finite cases do not establish exhaustive builder parity.
- Authenticated execution resolves touched paths, applies sparse trie mutations,
  loads collapse witnesses, preserves untouched commitments, and retains Volume
  boundaries. Tests compare sparse execution with a fully materialized final root,
  reject corrupted content, and avoid fetching unused substates on a no-op.
- Header admission classifies malformed/off-schedule/excluded/linked states and
  preserves retriable future-time/missing-anchor outcomes. Tests enforce schedule
  checks before wall-clock deferral. Genesis shape and premine bounds are checked.
- Exact growth/floor/ceiling calculations, child commitments and bounded logical
  block-content resolution are implemented. Unique owned bytes include the root,
  child index, transaction trie and transaction/body content; independent spec,
  state and child-block Volumes are excluded. Fetch errors remain unavailable
  evidence rather than size verdicts. Premine execution has its own genesis path.
- State transitions return materialized-reference StateDiff counts, including
  separate deposit-spend/insert stages. Broader Swift diff fixtures remain needed.
- WASM policy context encoding matches the complete pinned Swift golden.
  `lattice-wasm` implements an explicit integer/reference/bulk-memory allow-list,
  eager validation, exact export signatures, context allocation/copying, fresh
  instances and acceptance only for i32(1). All 38 scanner cases agree with the
  pinned Swift scanner/parser. Internal execution bytes carry a separate stack
  meter; original bytes remain the content-addressing input.
- Stack profiles reproduce WasmKit's constant/local register bookkeeping and
  call offsets: 55 actual compiled Swift profiles agree exactly. Runtime tests
  match Swift's 8,189/909 recursion boundaries with zero/64 extra locals, from
  both ordinary and 128 KiB caller threads. A dedicated worker keeps Wasmi's
  native compilation stack independent of the caller; this does not replace the
  protocol's 65,536-slot VM stack meter. Twenty thousand nested blocks compile
  and execute. Indirect calls, start traps, exact pointer bounds, export-name
  collisions and fresh state are covered. Seventy-three actual Swift/Rust runtime
  cases agree, comparing eager validation followed by two fresh evaluations.
  Raw Swift lazy evaluation can accept an unused malformed function that eager
  genesis validation rejects; the corpus records this distinction explicitly.
- Policy gates now authenticate module CID/CBOR before execution, deduplicate
  fetching, select scopes, and encode transaction/action contexts in original
  order. Genesis validates all configured entrypoints; an empty batch fetches
  selected modules without instantiating them. Missing/provider failures remain
  unavailable evidence, separate from authenticated defects and policy traps.
  These gates are composed into the root and child validation paths above; graph
  admission still supplies ancestry, securing work and continuity.
- WASM evidence lives in `crates/lattice-wasm/tests/swift-scan-evidence.json` and
  `swift-stack-evidence.json`, and `swift-runtime-evidence.json`. Regenerate with
  `scripts/wasm-scan-fixtures.py`, `scripts/wasm-stack-fixtures.py`, and
  `scripts/wasm-runtime-fixtures.py` (see each script's arguments).
- `lattice-block-tree` is under active implementation alongside `lattice-node-rs`;
  consult its current source/tests before relying on a specific API.

## Formal evidence

See [the platform validation map](docs/VALIDATION.md) for the scope of each
model bridge and remaining connections.

`python3 scripts/formal-check.py ../lattice-formal` verifies pinned provenance,
builds Lean and checks production Rust output: 2,703 ASERT vectors and 650 value
transitions. Value outputs also match the checked-in Swift traces. Both checkers
reject deliberately modified output, including both false acceptance and false
rejection of value transitions. The harness reconstructs the corpus from the
pinned upstream files, checks their hashes, and rejects altered/reordered input
cases from the Rust exporters. Save an auditable run with:

```sh
python3 scripts/formal-check.py ../lattice-formal --output-dir target/formal-run-001
```

The output directory must be new. It retains Rust traces, checker logs, negative
controls and a success/failure report with corpus/trace/checker hashes and compiler
versions. `python3 -m unittest discover -s scripts -p 'test_*.py'` checks the
harness trust boundary without invoking Lean. The value model excludes general keys,
nonces, signature/policy checks, Merkle authentication and state-growth limits;
separate tests cover some of those, and block admission must compose them.

The direct [work arithmetic bridge](docs/WORK_FORMAL.md) imports the unchanged
Lean `Work` model and checks 4,337 production target-work, hash-work and inclusive
target observations. Three deliberate output mutations must be rejected. Run
`python3.12 scripts/formal-work-check.py ../lattice-formal`; this finite check does
not cover carrier selection or prove the Rust implementation.

The upstream TLA+ `MCTie.cfg` model completed locally: 466,489 distinct states,
3,360,361 generated, no error. This is a baseline model check only. Rust fork-choice
and journal refinement require production traces; this run does not establish them.

## Transaction preflight

`TransactionPreflight` authenticates a supplied tip and classifies one signed
transaction as ready, future, invalid or unavailable. It checks path/surplus,
spec/growth floor, first-signer nonce, withdrawal evidence, state execution and
policies in reference order. Ready results retain authenticated net-debit
balances and bind transaction CID, tip CID, parent state, path and requested
policy mode. The host must separately require an executed tip and fence stale
jobs; this is not proof of graph execution or complete block admission.

Three Rust tests cover mode/order, balance/nonce rejection and source failures.
The initial 18 serialized-CAS scenarios match the full unchanged pinned Swift
`TransactionPreflight.classify`, including missing evidence and timestamp
exhaustion. The reproducible runner is
`crates/lattice-validation/tools/generate-preflight-vectors.py`; extended policy,
withdrawal and partial-CAS differential cases remain in progress. Policy mode
false is solely for host rechecks; block construction must judge policies again.

## Multikey compatibility boundary

The sibling Multikey compatibility decoder now reproduces the pinned Swift
codec's tenth-byte truncation and signed two-character hex pairs, with 297
actual Swift acceptance/rejection cases. Strict envelope parsing remains an
explicit separate API. Address derivation and transaction signature-array
normalization use the compatible decoder; direct signature verification still
requires exact canonical key text. Primitive regressions cover aliases,
address equality, direct-signature rejection and duplicate normalized keys.
This closes the prior overflow acceptance discrepancy without removing the
canonical signature admission gate. The same signed-pair compatibility now
applies to Swift Data(hex:) private-key parsing and signature-array normalization;
direct signatures still require canonical lowercase text.

## Remaining requirements

- Complete accepted/rejected Swift serialization and malformed-input parity,
  beyond the 297 malformed/nonminimal Multikey cases now matched to unchanged
  Swift, and broader cryptographic edge cases.
- Expand builder and child-genesis/non-genesis differential coverage, adversarial
  child proofs and state-diff fixtures. Node integration now composes private
  execution, securing-work and parent-continuity capabilities; see sibling
  lattice-node-rs/PORTING.md for persistence and formal-trace evidence. Individual
  execution gates must not be mistaken for full graph admission.
- Broader generated adversarial WASM runtime cases and cross-platform runs;
  authenticated module/context gates are already composed into block validation.
- Complete hosted-child daemon integration: child transaction workflows, scoped
  APIs and recovery alongside merged mining. Real two-daemon child mining/sync
  and restart now pass; private proof/continuity capabilities remain mandatory.
- Wire node retention modes and remaining operator/reconfiguration/scheduling
  behavior. The all-state node, SDK, worker/coordinator and sponsor have real
  root-node integration evidence; this does not finish the whole platform.
- Reproducible cross-repository CI (current local sibling path dependencies),
  cross-platform tests, adversarial/fuzz/resource tests and mixed-node operation.

No placeholder success result is an acceptable substitute for any missing layer.
