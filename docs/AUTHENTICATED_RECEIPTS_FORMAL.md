# Authenticated receipt/value composition

Run `python3.12 tools/formal-authenticated-receipts-check.py` from this repository.
It requires the unchanged `lattice-formal` checkout at
`a9f4c2879b6565ae11c17d35efe14475c5830978`, Lean through `lake`, and the sibling
Rust dependencies. Reports, observations, generated Lean programs and their logs
are written to `target/formal-authenticated-receipts`.

The bridge observes production `AuthenticatedBlockExecution::execute`, starting
from a state header containing only its CID and a real content source. It checks
128 receipt-action cases, including 40 successful transitions, over different
prior receipts, funded/unfunded accounts, duplicate receipts, zero amounts,
different directories and nonces including `u128::MAX`. Every successful result
is fully resolved and compared with the production pure value transition;
rebuilding its receipt trie must produce the same receipt-root CID.

The generated Lean program imports unchanged `applyBlock`, `receiptWrites`,
`Root.batch`, `Root.applyAll` and receipt-key path definitions. It compares value
balances and receipt maps, complete before/after receipt tries, and a partial
transform witness bound to the before root. Full and partial execution must
produce the same observed after root. Lean also proves 168 concrete before/after
`Root.Canon` obligations. These are executable finite observations, not a new
universal refinement theorem.

An independent Python decoder binds 39 observed trie encodings to their actual
DAG-CBOR bytes and SHA-256 CIDs. Receipt key preimages and their hashes are also
checked independently, with collision freedom required only among observed
preimages and encodings. The checker rejects missing encoding-table entries;
the table is not a replacement for a general proof of the serializer or hash.
The model's `Merkle.receipts_commit` and `Keys.receipts_commit_path` theorems
explain the composition and its assumptions; this harness checks the finite
production computations rather than claiming a full Rust instantiation of all
theorems' hypotheses.

The acquisition boundary records eleven actual refusals: removing or corrupting
each content entry read by one successful receipt transition, and supplying a
materialized state whose content does not match its header CID. Nine harness
negative controls cover invented balances, wrong receipt owners, wrong roots,
missing witnesses, changed demand amounts, accepted invalid transitions,
corrupted CBOR, mismatched abstract encodings and an altered acquisition verdict.
The last control mutates the observation; the eleven recorded refusals execute
production acquisition itself.

This closes a receipt-focused portion of the gap between the existing value
model and independent Cashew model checks. It does not prove account/deposit
root composition, full state-root serialization, transaction signatures,
transaction nonce rules, policy execution, block/child admission, parent proof
validity, or arbitrary actions and inputs. Actions here are controlled fixtures
passed to the authenticated state executor, not authenticated signed blocks.
The `u128::MAX` receipt nonce is a state-execution/key boundary case; it makes no
claim that a transaction carrying that nonce is representable in Swift DAG-CBOR.

The report records the formal pin, source/dependency fingerprints, tool versions
and observation hash. It fails if inspected sources change during the run and
starts by replacing stale success with `passed: false`.
