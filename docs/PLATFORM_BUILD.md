# Building the separate Rust platform repositories

The platform remains split across repositories. Current development uses sibling
checkouts because several Rust ports have not been published or assigned remote
commit pins yet. This layout is not a reproducible remote release: local work
includes new, uncommitted files. Do not replace missing pins with the original
outdated Rust repository HEADs and assume equivalent functionality.

The local inventory is explicit in `scripts/platform-check.py`:

| Repository | Role |
|---|---|
| array-trie-rs | Array trie |
| multikey-rs | Key encoding |
| uint256-rs | Reusable 256-bit arithmetic |
| trie-dictionary-rs | Trie dictionary |
| cashew-rs | Canonical content, Merkle structures, volumes |
| tally-rs | Peer accounting |
| volume-broker-rs | Retained content and disk storage |
| ivy-rs | Core/native/browser networking, host adapters and simulation |
| lattice-rs | Protocol primitives, work, validation, block tree and WASM |
| lattice-sdk-rs | Client, transactions, discovery, canonical content and sponsor APIs |
| lattice-storage-rs | File volume layout |
| lattice-miner-gpu | Existing CPU/Metal/OpenCL/CUDA worker |
| lattice-mining-rs | Mining coordinator and worker supervision |
| lattice-node-rs | Durable platform node and RPC |
| lattice-sponsor-rs | Sponsor accounting and service |
| lattice-light-client-rs | Sealed-witness account and state dictionary proofs |
| lattice-ctl-rs | Local operator keys, RPC and signed transactions; host lifecycle pending |

With these directories under one parent, run from lattice-rs:

```sh
python3.12 scripts/platform-check.py --output target/platform-inventory.json
python3.12 scripts/platform-check.py --check --output target/platform-check.json
```

Python 3.11+ is required. `--root /path/to/parent` selects another sibling layout.
`--repository lattice-sdk-rs` limits compilation; the inventory still checks all
listed repositories and local path edges. The tool reads manifests (including
workspace and target-specific dependencies), verifies each local path exists,
and records Git heads, dirty state, Cargo manifest/lock hashes and a source/test
input fingerprint. New repositories with no commit are recorded with a null
head. It never resets, commits, fetches, installs dependencies, or edits source.
Cargo may download locked dependencies and populate normal target caches.

The build phase runs `cargo check --workspace --all-targets --locked` separately
in each repository and saves individual logs. If relevant inputs change during
the audit, the report fails rather than attributing a previous build result to
later source. This matters while agents are implementing several repositories.
Fingerprints cover Rust source, manifests/locks, and selected test/native fixture
formats; they do not capture every possible external build input or environment.

A successful report proves local path availability and native default-feature
compilation of the inspected inputs. It does not run tests or prove Swift
compatibility, formal conformance, WASM builds, optional GPU features, GPU hardware
correctness, full platform functionality, or release availability. Keep those
checks and their reports separate. Repository PORTING documents identify the
remaining implementation gaps and narrower verification evidence.

Before standalone CI/release can be claimed, create actual reviewable commits,
resolve sibling dependencies to real published revisions or versioned crates,
update each repository's CI checkout layout, and build/test those exact revisions
from clean checkouts. Existing standalone Ivy CI currently does not provide its
new sibling Tally dependency; this audit reports the local arrangement, not a
claim that that remote workflow is fixed.

## Exporting exact local source inputs

`scripts/platform-snapshot.py` captures the current files without manufacturing
commits or remote revision pins. It exports all 17 repositories as siblings in a
deterministic uncompressed tar archive, preserving executable bits and recording
each selected file's SHA-256, size and mode in `SNAPSHOT.json`. Previously exported archives keep their own repository inventory; adding a new
repository does not invalidate old archives. The manifest also
records actual Git HEADs (null where no commit exists), dirty state and dependency
inventory. Identical selected inputs and Git metadata produce identical archives;
archive timestamps, owner names and IDs are normalized.

```sh
python3.12 scripts/platform-snapshot.py export --output target/platform-source.tar
python3.12 scripts/platform-snapshot.py verify target/platform-source.tar --output target/source-integrity.json
python3.12 scripts/platform-snapshot.py check target/platform-source.tar --output target/source-build.json
```

The exporter refuses to replace an existing archive. Use a new filename for each
checkpoint. It includes Git-listed tracked and untracked nonignored source,
fixtures, documentation, build configuration and tools through an explicit suffix
and filename allowlist. It excludes `.git`, ignored build caches, unselected file
types such as logs/bytecode, and reports omitted Git-listed names. It is not a
secret scanner: review the exported inventory before distributing source. Symlinks
and submodules are rejected rather than followed; tracked deletions remain absent.
The exporter rereads the selected source tree after capture and rejects an input
change during export.

`verify` checks inventory, content hashes, executable modes and path containment
while unpacking a fresh temporary tree. It rejects duplicate members, unexpected
files, absolute/traversing paths and links. `check` additionally validates every
local path dependency against that fresh sibling layout and executes
`cargo check --locked --workspace --all-targets` in each selected repository.
`--repository NAME` can be repeated to limit compilation; archive verification
and dependency-layout validation still cover all repositories. Source files are
checked again after compilation, and Cargo build outputs use a separate fresh
target directory. Build logs and a report survive temporary-tree removal. A
failed/interrupted run cannot leave a stale `passed: true` report.

This verifies an exported source copy, not a Git clean checkout or a published
release. Cargo registry sources and installed toolchains are not bundled; network
access, the normal Cargo cache, compiler/native libraries and environment remain
external inputs. The report records Rust/Cargo versions and the exact archive
hash. No source-export result implies reproducible binary bytes, an offline build,
Swift/formal proof coverage, optional feature compilation, or passing tests.
`python3.12 scripts/test-platform-snapshot.py` checks archive corruption, path
escape, duplicate/link rejection and post-extraction mutation detection.

## Current standalone workflow audit

Only four of the 17 local repositories currently have GitHub workflows:
`multikey-rs`, `ivy-rs`, `lattice-rs` and `lattice-miner-gpu`. Multikey and the GPU
worker have self-contained Cargo dependency layouts; their existing workflows
still need results against the final revisions. Lattice and Ivy workflows check
out only themselves, while their current manifests require sibling repositories.
The local export can validate that sibling layout now; it cannot make those
remote workflow runs valid. The remaining 13 repositories need standalone CI.

No sibling checkout is added with an old or invented revision: current ports
include unpublished/uncommitted source, so existing remote HEADs cannot identify
these inputs. Publication still requires reviewable commits in the separate
repositories, actual published/versioned dependency pins (or an explicit CI
manifest of real source revisions), checkout steps matching those dependencies,
and fresh CI build/test results for that exact set. The source archive is a
reviewable local checkpoint that can be transferred; it is not a substitute for
those publication steps.

The 2026-11-02 local checkpoint `target/platform-source-20261102-v2.tar`
(SHA-256 `d73d5ee00e00dda436a65d9df96659b094125ddc25d5ed6c75a3aa0f53e968e1`)
passed all 16 locked native all-target checks from a fresh extracted source tree;
see `target/platform-source-check-20261102-v2.json`. The first export caught a
stale `lattice-storage-rs/Cargo.lock` after SDK dependencies changed; regenerating
that lock from the actual local manifests fixed the clean-copy failure. These
results apply to the archive's exact inputs, not later ongoing source edits.

## Exact committed sibling checkout

[scripts/platform-bootstrap.py](../scripts/platform-bootstrap.py) validates a
complete manifest of real repository commit IDs and builds isolated sibling
checkouts with configured Git authentication. It refuses dirty or partial
destinations and never invents revision pins. See [BOOTSTRAP.md](BOOTSTRAP.md).
Local fixture tests cover all 17 repositories; an actual remote bootstrap still
requires publishing the reviewed commits and access to private repositories.
