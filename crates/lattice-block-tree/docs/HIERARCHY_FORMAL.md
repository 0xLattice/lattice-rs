# Production hierarchy event traces

Reference: `0xLattice/lattice-formal@a9f4c2879b6565ae11c17d35efe14475c5830978`.
The runner refuses another revision or tracked modifications. It imports the
original `scripts/hierarchy_trace_check.py`, which runs the unchanged
`specs/TraceHierarchy.tla` and `Hierarchy.tla` through TLC. No replacement
fork-choice or attribution algorithm exists in the exporter or Python adapter.

From this crate:

```sh
cargo test --example hierarchy_trace
python3 tools/formal-hierarchy-check.py
cargo clippy --example hierarchy_trace -- -D warnings
```

Pass an alternate pinned formal checkout as the positional argument and
`--output-dir PATH` to retain a separate report. Default artifacts go to the
workspace `target/formal-hierarchy/`: production traces, negative-control traces,
and a JSON report including model/checker/exporter/Cargo.lock hashes and Rust
version. The runner uses locked Cargo resolution. `JAVA` overrides the runtime;
on macOS it discovers Homebrew OpenJDK when unset.

## Evidence

Two distinct arrival schedules, parent-first and child-first, each emit 36 actual
production observations across Nexus, Alpha, and Beta (72 total). They cover:

- Parent runs arriving before and after the committed child is held.
- A root non-carrier's stronger work reaching both child and grandchild; carrier
  own-grind subtraction; additional shared merged grind observations at all three
  levels with different per-chain quantities.
- Repeated equal-credit no-op, conflicting grind-location refusal, and a refused
  child-root exclusion before another child root executes. Refused replay must
  leave the observable graph unchanged.
- Child and grandchild anchors held before their parent state executes, then
  proven through actual `prove_parent_continuity` and
  `verify_child_parent_anchor` capabilities.
- An executed child anchored on a noncanonical parent branch, a transition
  exclusion while its anchor remains held, exclusion retaining work, and eventual
  executed tips at all three levels.
- Actual raw CID-byte rank ordering and final state agreement across schedules.

The checker verifies every event is a legal model step with its reported
accept/refuse, reason and cause. After every event it compares canonical tips,
validated tips, every held block's subtree work, and lower-chain continuity.
The Rust example's test additionally requires meaningful positive executions,
all three refusal reasons, and a noncanonical-anchor execution witness.

Ten negative controls must fail: the upstream eight applicable controls alter
child/grandchild weights and continuity, the child validated tip, a held refusal,
an exclusion cause and a conflicting-location reason. Two additional controls
remove precisely the observed child or grandchild increase from the root
non-carrier's work raise. Both must fail `WeightsMatch`.

## Boundary and limitations

This is **finite trusted-fact reducer event refinement**, with actual production
parent-anchor capabilities. The fixture supplies immutable header metadata,
verified-work facts and transition verdicts through the durable-replay codec.
`ChainTree::replay` intentionally does not authenticate external facts. Thus an
accepted validation event does not establish that Rust performed PoW, signatures,
Wasm policy evaluation or a state transition in this test. The schedule supplies
valid verdicts; the formal checker checks that applying them is a legal step.
Refusals for location conflicts/root exclusion are real replay outcomes; held
anchors are real capability outcomes. The synthetic block used for an anchor
capability is not a cryptographically authenticated block admission.

The exporter explicitly propagates `apply_parent_run` from P to C to G after each
event, using the production implementation. This checks the resulting graph,
not that a future node event loop always schedules these calls. It does not test
incremental `parent_blocks` filtering, async derived-proof completion, crash
recovery or liveness. Header-rule exclusion, dynamic availability, invalid proof
bytes, root-chain genesis alternatives, and all other interleavings are not
covered by these two traces. Child genesis alternatives are covered.

The model has one child directory per level and identifies each parent block
with a distinct changed state. Repeated/no-op states, arbitrary Unicode names,
and multiple sibling directories need separate refinement coverage. Content
addressing/canonical decoding and collision resistance remain assumptions at
this boundary. Wider authenticated admission tests and existing Swift fixtures
are complementary evidence, not conclusions of this TLC run.
