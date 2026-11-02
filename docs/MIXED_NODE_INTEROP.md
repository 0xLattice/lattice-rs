# Full Swift / Rust node interoperability

Run from `lattice-rs`:

```sh
python3 tools/mixed-node-integration.py
```

The harness builds the unchanged pinned Swift `lattice-node` executable and the
current Rust `lattice-node` executable, then starts both with temporary data
and identity directories. Swift's dependency resolution is fixed by its
original `Package.resolved` (`--disable-automatic-resolution`). Every resolved
dependency checkout is checked for its exact pinned revision and absence of
tracked modifications before and after the scenario. The default
Swift scratch directory is `/tmp/lattice-node-mixed-build`; `--swift-bin`
selects another scratch directory's `debug/lattice-node` product.

Only a localhost peer is configured. Swift receives `--no-default-peers`,
`LATTICE_FORBID_DEFAULT_PEERS=1`, disabled provider-walk dials and disabled stale
peer search. Its WebSocket listener is disabled. RPC binds to localhost. The
Swift overlay listener uses the original daemon's default bind behavior;
this harness does not claim to add a sandbox or firewall. No public node,
miner, wallet, or production data directory is used. Building dependencies
may fetch pinned packages.

## Actual runtime path

1. The Rust `nexus_genesis_volumes` example uses the production genesis builder
   to construct the exact pinned `NexusGenesis.swift` specification, premine
   and timestamp. It asserts the canonical Swift genesis CID, then exports
   recursive Cashew volumes including the empty entering state. These bootstrap
   the Rust host through its ordinary validated bootstrap path. The Swift host
   independently builds its own canonical genesis through its normal startup.
2. The Swift host dials the Rust host's native Ivy endpoint. The harness does
   not supply headers, claimed validation results, or post-state facts to the
   other host.
3. A real Rust `/mining/templates` request and `/mining/work` submission
   produce height one, paying an openly known RFC 8032 test identity. The Swift
   host receives the block through actual overlay sync, obtains its content,
   validates and executes it. Their executed tips and template digests agree.
4. The standalone `mixed_node_signer` example signs a nonce-zero transfer of
   17 units plus one unit of fee using the production signing preimage and
   Ed25519 implementation. The transaction is submitted only to Swift. Rust
   must acquire it through transaction announcement/content gossip, report it
   in its pool, and include it through its own real template/work endpoints.
   Swift must synchronize and execute the resulting height-two block.
5. A nonce-one transfer of 23 units plus one fee is submitted only to Rust,
   gossiped to Swift, and mined by Swift at height three. Both hosts must clear
   their pools and agree on exact balances and next-expected nonce two. Before
   each valid submission, an altered-signature version is sent to each host;
   all four controls must be refused with a client error. The valid counterpart
   then succeeds, avoiding a matching-rejection false positive.
6. Swift is stopped while Rust mines height four on branch A. Rust is then
   stopped and Swift restarted at their common height three. Swift mines a
   distinct height four and height five on branch B, paying a different
   recipient. Both hosts then restart and reconnect through native Ivy. Rust
   must replace its previously executed branch A with branch B, rolling back
   A's coinbase credit. No graph facts or admission verdicts are injected.
7. Both hosts' content-backed account reads must agree: the test sender holds
   3,145,688 units with next nonce two, while the second recipient holds
   2,097,192 units with next nonce zero. Both hosts are stopped cleanly and
   restarted again from their durable directories, with Rust bootstrap files
   removed. Their height-five tip and both accounts must still match.

The six mined blocks include two competing height-four blocks; the final
canonical chain has five blocks after genesis. The helper searches up to one million nonces against the actual template
search target, using the production block prefix and hashing implementation.
Python independently checks the returned SHA-256 digest before submission.
Every block uses normal proof-of-work admission; even this short easy schedule
can reject nonce zero after its target adjusts. No private production account is used: the test identity is a public
cryptographic test vector, and its balance comes entirely from this temporary
network. The second recipient is a valid CID with no spending key required.

Transaction fees are one unit: the original Swift daemon defaults to a minimum
relay fee of one. A zero-fee first attempt correctly failed Swift relay policy;
the harness pays the fee rather than disabling that policy. The miner pays the
same test identity on the first three blocks, so the transaction fees return
to that identity and the expected balance loses only the transferred amounts.

## Evidence and limits

`target/mixed-node/report.json` is initialized with `passed:false` before work
starts. A passing report includes all template payloads, submitted transactions, signature-refusal controls,
pool and execution statuses, reorg/account responses, recovered states,
executable hashes, toolchain versions,
and source/lockfile hashes. The harness checks the source snapshot again at
completion; changing sources during the run invalidates the evidence. Logs
include the build and both hosts before and after restart.

The Swift source checkout must remain clean at
`3df9e0a786c7b103a3da88d1c3881efc81706255`; no daemon, Ivy, validation or consensus
source is patched. The only added Rust sources are standalone bootstrap and signing
examples. Each executable is copied into the temporary directory before use,
so concurrent builds cannot replace the binaries exercised by the test. These are complete original Swift and current Rust node processes,
not a sliced Swift model, mock server, or transport-only echo test.

This finite scenario covers root blocks, two signed account transfers, native
transaction gossip, one controlled root reorg, signature-refusal controls,
and durable recovery. It does not establish child-chain hosting parity, WASM
policy execution over the network, reorg transaction resurrection, arbitrary
fork topologies, hostile-peer behavior, retained-state pruning, high load, or
long-run network convergence. Existing differential and formal tests cover
separate pieces and are not implicitly promoted to full-network evidence here.
