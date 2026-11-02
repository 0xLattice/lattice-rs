# Production Keys / ChildCommit observations checked by Lean

Run:

```sh
python3 tools/formal-keys-child-check.py
```

The runner exports observations from the actual Rust primitives and validation
functions, imports the unchanged pinned Lean `KeysProofs` and
`ChildCommitProofs`, and executes comparison checks. It does not edit the
formal repository. Outputs and logs are in `target/formal-keys-child/`.

## Correspondence

- **136 grammar inputs:** all single ASCII bytes, empty strings, separator and
  whitespace cases, non-ASCII text and combining characters. Production
  `deterministic_atom`, `directory_atom` and `account_atom` are compared with
  `Keys.VisibleAscii`, `ValidDirectory` and `ValidAccount`.
- **320 key inputs:** production `DepositKey::to_string`,
  `ReceiptKey::to_string` and `ReceiptKey::storage_key` are compared with
  `depositText`, `receiptText` and `receiptPath`. Amounts include zero and
  u64 maximum; nonces include values above u64 and u128 maximum. JSON exports
  these integers as decimal strings, and Lean receives exact natural numbers.
- **168 CID inputs:** canonical production content IDs and variants with
  uppercase characters, whitespace, omitted multibase prefix, padding,
  nonzero trailing padding bits and a different codec. Production
  `standard_cid` must agree with whether `Keys.cidDecode` succeeds.
- **96 proof-of-work prefixes:** actual `Block::pow_prefix`, not a test copy of
  its formatting, must equal the model's exact character/byte sequence. Cases
  cover version/height limits, signed timestamp limits, optional parent,
  absent versus explicitly empty recipient, and canonical recipient text.
- **350 carrier inputs:** production `valid_child_commitments` is compared
  with `ChildCommit.commitsValid`. The inputs include ordinary actions,
  commitment writes and deletes, malformed separator counts, stale entering
  state, absent or wrong child CID, malformed directories, sparse child links,
  and an unloaded child index. Duplicate keys occur across two transaction
  bodies. Production `execute_block` determines whether the state update
  succeeds; that outcome is compared with `KeysDistinct` under the controlled
  state prerequisites below.

The exporter constructs child indexes using `Header::new`, so their CIDs bind
those exact entries. Children themselves remain CID-only references; the
commitment predicate does not fetch or execute child blocks. The observed
`childrenWellFormed` flag is computed by the production block method. The
formal model intentionally takes that flag as input rather than redefining
CID, encryption and directory validation inside `ChildCommit`.

## Necessary assumptions and boundaries

`ChildCommit.Valid` includes only distinct general-action keys and the child
commitment predicate. It is not complete block validity. For the state check,
every action has a nonempty key, a present old value, and that old value equals
the input map's value. There are no account/swap actions, signatures, policies,
coinbase rewards or other competing error conditions. Therefore duplicate
keys are the remaining state-transition conflict, including duplicates across
bodies. The test does not equate arbitrary transition success with
`KeysDistinct`, nor claim that grammar-invalid actions pass complete admission.

`commitsValid` assumes an available child index. Rust returns a content error
when a commitment needs an unloaded index. The adapter explicitly checks this
extra acquisition boundary: no commitment succeeds without reading the index;
a needed unavailable index must yield an error. It never relabels missing
content as the formal model's `false` verdict.

Lean's key texts are Unicode character lists; Rust strings are UTF-8. The
bridge maps Unicode scalar values exactly for text comparisons. The grammar
only admits visible ASCII. Proof-of-work byte comparisons are restricted to
ASCII fields plus their modeled NUL separator and recipient tag; this makes
one modeled character equal one actual byte. The prefix cases satisfy the
link/target/separator prerequisites of `Keys.Header.Valid`; empty recipient is
included to test encoding separation, even though full production block
admission requires a standard recipient CID.

`receiptPath` abstracts SHA-256 as `H`. The bridge supplies a finite table of
observed receipt-preimage/digest pairs, refuses unknown table inputs, and
independently verifies every exported digest with Python SHA-256 over UTF-8.
It does not implement SHA-256 in Lean, establish collision resistance, or prove
the no-collision hypotheses in `receiptPath_injOn` and
`receiptPath_separated` for arbitrary inputs. Receipt text injectivity also
requires the relevant directory/account grammar assumptions; arbitrary
slash-containing strings in the encoding corpus are not promoted to valid
state keys. The corpus compares actual encodings on such inputs only.

The imported theorems prove properties of their models. Executing these finite
comparisons does not prove the Rust implementations refine the models for all
inputs. `ChildCommit` proof composition, securing-work verification, per-location
credit ratcheting and parent-run binding are outside this particular bridge.
Other platform bridges cover separate concerns and are not substituted here.

## Evidence lifecycle and negative controls

The report is atomically initialized with `passed:false` before pin checks or
builds, and becomes `passed:true` only after all checks pass. The runner records
Cargo.lock, production source/manifests, exporter/checker/runner, unchanged
formal model/toolchain hashes and compiler versions. It rechecks source hashes
and the clean pinned formal revision at completion, rejecting concurrent
changes. The pin is `a9f4c2879b6565ae11c17d35efe14475c5830978`.

Seven deliberately altered production observations must be rejected by the
same Lean comparison: grammar result, receipt path, CID acceptance, prefix
byte, commitment verdict, duplicate-key state success, and missing-index
success. Each negative file asserts rejection; a Lean syntax/elaboration
failure is a harness failure. An eighth altered receipt-preimage control must
fail the independent SHA-256 check. The report describes finite conformance
and its explicit representation boundaries, not a whole-platform proof.
