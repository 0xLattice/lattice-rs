# Work arithmetic against lattice-formal

Run from this repository with the sibling formal checkout:

```sh
python3.12 scripts/formal-work-check.py ../lattice-formal
```

The harness requires the tracked-clean `0xLattice/lattice-formal` revision
`a9f4c2879b6565ae11c17d35efe14475c5830978`. It builds its unchanged `Work` and
`WorkProofs` modules. Our local Lean adapter imports `LatticeProofs.Work` directly;
it does not translate the model into a second Rust or Python implementation.

The Rust `lattice-pow` example calls the production `work_for_target`,
`work_for_hash`, and `validate` functions. Inputs include zero, maximum UInt256,
every power of two and adjacent values, hashes at/below/above each target, and
512 deterministic SHA-256-derived target/hash pairs. The harness checks that
Rust preserves every input and case before invoking Lean. All 4,337 observations
conformed in the initial run.

Three deliberately altered outputs must fail with a model disagreement: credit
for target zero, failure to saturate the work of hash zero, and rejecting a hash
equal to a nonzero target. These mutate observations, not production source; they
verify the checker detects these errors, not that every possible implementation
mutation is detected.

The report records model, adapter, Rust source, lockfile, input and trace hashes,
toolchain versions, case count, and negative checks. Logs and vectors live under
`target/formal-work/`; a failed invocation leaves `passed: false`.

This is a finite executable refinement check against the proven mathematical
work model, not a proof of the Rust implementation. It covers arithmetic and
inclusive target comparison at width 2^256. It does not cover SHA-256 correctness,
carrier-ladder credit selection, child commitment verification, authenticated
block admission, scheduling, or network behavior. Those need separate bridges
and tests. The existing ASERT/value, hierarchy, continuity, and recovery checks
also retain their own documented scopes.
