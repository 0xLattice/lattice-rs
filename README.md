# lattice-rs

Rust protocol implementation for the [0xLattice platform](https://github.com/0xLattice/Lattice),
kept separate from the node and reusable library repositories. The port targets
the core protocol, node and platform libraries; application chains are outside
scope. Development is local and unpublished, and the full platform port remains
in progress.

| Crate | Implemented boundary |
|---|---|
| `lattice-pow` | Consensus preimage, SHA-256 nonce search, target/hash work, exact cumulative work and ASERT |
| `lattice-primitives` | Actions, specifications, identities, signing, transactions, blocks and content-addressed state types |
| `lattice-validation` | Authenticated sparse state execution, root/child genesis and successor validation, policies, commitments, bounded content, preflight and block construction |
| `lattice-block-tree` | Exact-work graph, fork choice, private admission capabilities, child proofs, parent continuity, run attribution and durable replay |
| `lattice-wasm` | Protocol policy scanner, context encoding and bounded-stack execution with pinned Swift differentials |

Validation and graph admission are separate: a valid state transition alone
cannot assert securing work, executed ancestry or parent-state continuity.
Construction APIs produce untrusted templates, not admission capabilities.
See [PORTING.md](PORTING.md) for detailed coverage and limits.

The sibling `lattice-node-rs` repository runs a durable node with Ivy transport,
synchronization, transactions, mining and RPC. Mixed Swift/Rust root-node tests
exercise signed transaction exchange, competing branches and restart. Hosted
child orchestration and full child mining integration are still being completed.
The [repository inventory](docs/PLATFORM_BUILD.md) lists all sixteen Rust platform
repositories and their local dependency layout.

## Local checks

From this repository, with its sibling dependencies present:

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
python3.12 scripts/platform-check.py --check --output target/platform-check.json
```

The platform audit checks locked native compilation and records exact local
inputs. Tests, Swift differentials, formal checks, browser builds and GPU
hardware checks have distinct scopes; compilation does not replace them.

## Reference validation

Sources are pinned in `upstream-lock.json`. Compatibility evidence includes
actual pinned Swift arithmetic, serialization, signing, block construction,
validation and WASM executions. The node repository additionally compares root
and merged mining templates against Swift and runs real localhost integrations.

The pinned `0xLattice/lattice-formal` models validate production observations for
ASERT, value transitions, work, recovery, parent continuity, hierarchical work,
Merkle batches, keys and child commitments. Each bridge records its assumptions,
source/model hashes and negative controls. These finite checks are not a proof
of whole-platform correctness. See the [validation map](docs/VALIDATION.md) for
commands, evidence locations and remaining formal connections.

## Repository and release status

Sibling path dependencies currently refer to local, partly uncommitted checkouts.
Standalone remote CI and reproducible releases require actual versioned dependency
pins and clean-checkout verification. Do not substitute outdated upstream Rust
HEADs for these ports. No release or complete-platform compatibility is claimed.

MIT; upstream attribution is preserved in LICENSE.
