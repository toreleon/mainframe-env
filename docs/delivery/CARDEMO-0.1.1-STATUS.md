# CardDemo-full 0.1.1 Program Status

Current phase: **CD.A — corpus and application packaging**
Current issue: **CD-002 — acceptance passed; focused completion commit in progress**
Next issue: **CD-003 — complete source closures and compatibility copybooks**
Current product: **0.1.0-alpha.0**
Target product: **0.1.1**
CardDemo source: clean `59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e`

## Source identity

- mainframe-env base commit: `8d2f1a6476cee17032f277d41bb026cb1d7bdaf3`
- current deterministic dirty-tree identity:
  `sha256:60615424bc816493e7a0c95f024e68502005224936cfcc45c6308d7e92c98dd0`
- dirty-tree algorithm: SHA-256 of the sorted `sha256sum` records for every
  repository file except `.git/`, `target/`, and the two persistent CardDemo
  status ledgers (which carry the identity).
- preparation inputs and implementation changes remain local; no version,
  remote, tag, publication, or deployment action has occurred.

## Completed implementation issues

### CD-001 — pinned corpus gate

- State: **PASS**
- Required commit subject: `Gate the pinned CardDemo application corpus`
- Completion commit identity:
  `8d2f1a6476cee17032f277d41bb026cb1d7bdaf3`.
- Evidence: `conformance/0.1.1/evidence/issues/CD-001.json`
- Evidence digest:
  `sha256:b97d476b03e315b400dfa9f850c10672cc1d25289ee179c62e6f0f04f0c6c97b`
- Acceptance: `CARDEMO_CORPUS_DIR` is consulted only by the explicit CardDemo
  corpus gate; the exact repository, commit, tree, cleanliness, Apache-2.0
  license, 329-file canonical content identity, two runtime archives, two
  embedded runtime metadata files, and 33 declared file-count cohorts pass.
- Negative controls: missing environment, dirty/missing tracked input, commit
  drift, license drift, content drift, file-count drift, and relative-path
  escape all fail closed with stable redacted codes.

### CD-002 — comment-aware fixed source and COPY directives

- State: **PASS**
- Required commit subject: `Parse CardDemo fixed source and COPY directives`
- Completion commit identity: the focused commit containing this ledger and the
  `CardDemo-Issue: CD-002=pass` trailer; its hash is recorded on the next
  continuation to avoid self-reference.
- Evidence: `conformance/0.1.1/evidence/issues/CD-002.json`
- Evidence digest:
  `sha256:7adea86b25b9b625226ff3c50ecbc65d2beac8e3a4a4900cffb154dcf5b549a6`
- Acceptance: exact fixed columns, comments, continuations, quoted COPY names,
  multiline multi-pair COPY REPLACING, expansion bounds, and repository-relative
  source/directive origins pass. The corpus-backed gate preprocesses all 44
  programs against all 62 application/BMS copybooks twice with 346 identical
  expansions.
- Negative controls: comment/sequence-area/string false positives, missing and
  circular copybooks, malformed directives, invalid continuations, copy-depth,
  token, and expanded-byte limits fail explicitly.

## Commands and exit codes

- `cargo test -p mainframe-env-conformance --locked` — **0**; 19 passed,
  including all focused corpus controls.
- `cargo test -p xtask --locked` — **0**; 5 passed.
- `cargo clippy -p mainframe-env-conformance -p xtask --all-targets --locked -- -D warnings`
  — **0**.
- `env -u CARDEMO_CORPUS_DIR cargo xtask carddemo-corpus --check` — **1**;
  expected missing-environment failure.
- `CARDEMO_CORPUS_DIR=<redacted-local-checkout> cargo xtask carddemo-corpus --check`
  — **0**; exact pinned receipt passed.
- `env -u CARDEMO_CORPUS_DIR cargo xtask conformance --check` — **0**;
  versions, architecture, profiles, schemas, inventory, and existing evidence
  pass without the CardDemo environment.
- `cargo fmt --all -- --check` — **0**.
- `git diff --check` — **0**.
- `cargo test -p mainframe-env-compiler -p mainframe-env-conformance --locked`
  — **0**; 16 compiler and 19 conformance tests passed.
- `cargo check --workspace --all-targets --locked` — **0**.
- `cargo clippy -p mainframe-env-compiler -p mainframe-env-conformance -p xtask --all-targets --locked -- -D warnings`
  — **0**.
- `CARDEMO_CORPUS_DIR=<redacted-local-checkout> cargo xtask carddemo-source --check`
  — **0**; 44 programs, 62 copybooks, 346 expansions, and 44 deterministic
  replays passed.
- `env -u CARDEMO_CORPUS_DIR cargo xtask carddemo-source --check` — **1**;
  expected missing-environment failure.
- `cargo run -p mainframe-env-cli -- compile <redacted-corpus>/app/cbl/COSGN00C.cbl --format fixed`
  — **1**; expected CD-003 closure failure is now the genuine
  `CopyNotFound("COCOM01Y")`, not license prose.
- Final format, JSON, redaction, diff, ordinary conformance, and pinned corpus
  checks — **0**.

## Implementation status

- Issues passed: **2 / 27**
- Journeys passed: **0 / 20**
- Installed CardDemo resources: **0**
- Executed CardDemo application journeys: **0**
- Overall implementation: **IN PROGRESS**

## Open decisions and blockers

1. The pinned CSD defines `CDV1 -> COCRDSEC`, but the repository and runtime
   archive contain no source or object. Source or an owner-approved
   disabled/retired correction is required before the full-profile gate.
2. The repository is still `0.1.0-alpha.0`. A 0.1.1 release cannot be cut until
   0.1.0 is finalized or an explicit version-line correction is accepted.

Neither decision blocks CD-003. No CD-002 blocker remains.

## Next smallest executable step

Commit CD-002 locally with its exact subject and evidence trailers. Then record
that commit hash and begin CD-003 by moving the already proven corpus source
closure into generic CLI/compiler/installer inputs and replacing the nine
comment-only conformance placeholders with owned compatibility contracts.
