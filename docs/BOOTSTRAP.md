# Exact-revision sibling bootstrap

`scripts/platform-bootstrap.py` is standalone Python 3.11+ tooling for an explicit, complete
17-repository 0xLattice revision manifest. It does not publish repositories,
invent pins, choose branch HEADs, commit source, or replace existing work.

The manifest schema is:

- `format`: `lattice-platform-revisions-v1`
- `organization`: exactly `0xLattice`
- `repositories`: one record per supported sibling directory, containing
  `directory`, `repository` (`0xLattice/<actual repository name>`) and `commit`
  (the actual full lowercase 40-character commit ID).

The supported directory names are the 17 names in `REPOSITORIES` at the top of
the script. Every directory and remote mapping must be unique. Branch names,
tags, abbreviated commits, arbitrary URLs, embedded credentials, different
organizations and incomplete manifests fail before any Git fetch. The checked-in `platform-revisions.json` contains the actual published checkpoint
revisions. Its lattice-rs entry pins the source commit preceding the manifest-only
publication update, avoiding a self-referential hash.

```sh
python3.12 scripts/platform-bootstrap.py platform-revisions.json --destination ./rust-platform \
  --check --report ./bootstrap-result.json
```

Git uses the normal configured credential helper, SSH configuration or URL
rewrites. The tool accepts no token argument, disables interactive terminal
credential prompts, invokes subprocesses through argument arrays, and does not
print or save Git/compiler stdout or stderr. Errors and reports contain only
operation categories, known repository names and supplied public commit IDs.
Configure authentication before running it. Failed commands deliberately have
generic diagnostics; inspect your credential configuration or rerun the relevant
command locally when detailed diagnostics are needed.

For a new destination, every exact revision is fetched and checked out detached
under a temporary sibling staging directory. The origin mapping, actual commit,
clean worktree and Cargo path dependencies are validated before installation.
Symlinks and submodules are unsupported and refused. Path dependencies in normal,
workspace, target, build/dev, patch and replacement tables must resolve to Cargo
manifests inside the captured sibling layout. Cargo manifests and lock files are
required in every repository.

An existing destination is accepted only when it contains exactly all 17
directories, each already at its recorded commit with the correct organization
origin and no modified, untracked or ignored files. Partial directories, unrelated
files and dirty checkouts fail closed. There is no reset, clean, checkout-over,
force option or automatic repair. Existing sources are copied through their Git
objects into isolated temporary clones before any requested build; the original
checkout files are left intact and checked again afterwards.

`--check` runs `cargo check --locked --workspace --all-targets` for each repository
in the isolated staging layout, with one fresh target directory outside source.
Changes made to source/lock files by a build fail the final clean-state check.
Only after successful validation is a new complete source tree installed. A
failed fetch or build removes the temporary clones and leaves no partial
destination. `--report` exclusively creates a new file and refuses to overwrite
an old report; failures never leave a stale success claim.

This verifies exact source revisions and native/default-feature compilation. It
does not run test suites, Swift/formal checks, optional GPU/browser features,
or prove reproducible binary bytes. Toolchains, registries, native dependencies,
Git authentication configuration and environment remain external prerequisites.
Cargo build scripts execute repository code; this tool is not a sandbox. The
manifest's commits must actually be fetchable with the caller's configured
credentials before any remote bootstrap can succeed.

## Local validation

```sh
python3.12 scripts/test_platform_bootstrap.py
```

Nine tests use only disposable local Git fixtures and an isolated Git URL rewrite.
Git permits only the file protocol and Cargo is offline in the test environment.
They verify all 17 exact checkouts and locked Cargo checks, unchanged clean
existing files, dirty/ignored refusal, wrong organization/commit refusal,
unavailable revision cleanup, manifest/path dependency validation, suppression
of credential-bearing diagnostics and exclusive failure reports. No GitHub push,
repository creation or actual remote network access occurs in these tests.

## Continuous integration

The platform workflow first checks the bootstrap tools with local fixtures. Its
Linux, macOS and Windows jobs then fetch the complete reviewed sibling manifest
and run formatting, strict all-target Clippy, and native all-target tests in all
17 repositories. `platform-ci-manifest.py` replaces only the workflow repository's
pin with the actual checked-out commit, so a new change cannot silently test the
older checkpoint instead. Sibling pins remain exact; reports record the full set.
Tracked edits, a foreign origin and a source subdirectory are rejected. A failed
repository does not prevent checks of the remaining independent repositories;
the overall result still fails if any required check fails.

All 17 platform source repositories are public, so the workflow fetches exact
commits without a cross-repository credential. Checkout credentials are not
persisted. The check runner refuses known token environment variables, and build
outputs live outside source. Fork pull requests use the same public dependency
pins; there is no privileged `pull_request_target` build path.

Actual green remote runs must be verified before claiming CI coverage. Local bootstrap and
native tests do not establish Windows/Linux behavior. Optional GPU hardware,
WASM/browser targets, Swift or formal oracle regeneration, and daemon scenario
scripts remain separate checks.

`python3.12 scripts/test_platform_ci.py` exercises current-commit substitution,
wrong-origin/dirty-source refusal, credential exclusion, all 51 fixture checks,
and a real formatting failure that must leave a failed report.
