# Transaction preflight differential evidence

Reference: `0xLattice/Lattice@dda2d852429a27be6043599b63bde81530cd9514`,
`Sources/LatticeImport/TransactionPreflight.swift`. The 51-case corpus invokes
its actual unchanged `TransactionPreflight.classify`, with the real pinned
Cashew, signature verification, state execution and Wasm policy implementation.
No copied classification algorithm or stub validator is used.

```sh
python3 tools/generate-preflight-vectors.py /tmp/lattice-genesis-oracle
cargo test --test preflight_swift
cargo clippy --example preflight_samples --test preflight_swift -- -D warnings
```

The generator checks the clean pinned reference and every original Swift source
against its cached package, plus the exact Package.resolved SHA-256. The cache
must already contain the full source package and dependency resolution; it is
shared with other differential oracles, so do not run concurrent builds there.
The source itself is never patched. Only a separate executable oracle is added.
`preflight-swift-evidence.json` records source, harness, corpus and dependency-lock
hashes. Inputs are deterministic, using the established RFC8032 test seed.

## Coverage

All four dispositions appear: ready, future, unavailable and invalid. Cases cover
nonce freshness/exhaustion, insufficient balances, negative surplus, signatures,
chain path, floor growth, invalid spec, timestamp overflow, and missing body,
spec, state and account evidence.

Policy cases include accepting/rejecting modules, ready/future gating,
missing-module retry, cheap invalidity before a missing/rejecting module,
missing-state resolution before policy, disabled-policy ready/future rechecks,
and missing-parent withdrawal policy gating. Ready positive cases are explicitly
asserted by name, so matching rejections cannot silently weaken the corpus.

Child withdrawals include valid receipt/deposit, missing parent argument,
missing parent CAS, absent receipt/deposit, known wrong withdrawer, spent deposit,
wrong amount and high nonce values. Partial CAS cases intentionally distinguish
absent keys from unavailable content: Swift resolves deposit evidence before
judging an already-known contradicting receipt; missing deposit content therefore
returns unavailable. An absent receipt with a resolved parent returns unavailable
before a rejecting policy. These are observed source behavior, not inferred rules.

State cases include duplicate general actions, inserting an existing key,
incorrect old value, valid update, reserved account keys, authenticated bytes with
an incorrect account value type, and a state node with duplicate CBOR map keys.
Every Rust run decodes the transaction from CAS and passes unresolved tip and
parent headers; retained materialized nodes cannot hide deliberately removed CAS.

Four additional actual Swift encoding probes show withdrawal nonce UInt64.max is
encodable, whereas 2^64, 2^80 and UInt128.max are rejected by DagCBOR. Rust checks
the same boundary. The classifier corpus accepts withdrawals at UInt64.max and
UInt64.max-1; it does not claim that every value in the UInt128 field type has a
valid wire representation.

## Limits

This invokes the snapshot-based classifier, not ChainLevel's executed-frontier
selection or the node's epoch/pool-version fencing. It does not establish that a
ready transaction is valid in a multi-transaction block; aggregate conservation,
count/growth, consensus parent continuity and mining-template selection remain
separate responsibilities. Timing uses now=0 with controlled tip timestamps;
height overflow (Swift may trap), adversarial scheduling, and every malformed
CBOR shape are not covered. Policy-disabled results are conditional rechecks,
not authority to skip policy validation when building a block.
