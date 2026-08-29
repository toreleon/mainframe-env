# CardDemo-full 0.1.1 Program Status

Current phase: **CD.A — corpus and application packaging**
Current issue: **CD-001 — acceptance passed; focused completion commit in progress**
Next issue: **CD-002 — fixed-column comment-aware COPY preprocessing**
Current product: **0.1.0-alpha.0**
Target product: **0.1.1**
CardDemo source: clean `59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e`

## Source identity

- mainframe-env base commit: `3dd0de69c087315f7ec62b9176d046afbb3ea3b8`
- current deterministic dirty-tree identity:
  `sha256:7ec5e166f775700c83f968f8759990e1bbf04da77177fbedbb961511e34d4adf`
- dirty-tree algorithm: SHA-256 of the sorted `sha256sum` records for every
  repository file except `.git/`, `target/`, and the two persistent CardDemo
  status ledgers (which carry the identity).
- preparation inputs and implementation changes remain local; no version,
  remote, tag, publication, or deployment action has occurred.

## Completed implementation issues

### CD-001 — pinned corpus gate

- State: **PASS**
- Required commit subject: `Gate the pinned CardDemo application corpus`
- Completion commit identity: the focused commit containing this ledger and the
  `CardDemo-Issue: CD-001=pass` trailer; its hash is recorded on the next
  continuation to avoid self-reference.
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

## Implementation status

- Issues passed: **1 / 27**
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

Neither decision blocks CD-002. No CD-001 blocker remains.

## Next smallest executable step

Commit CD-001 locally with its exact subject and evidence trailers. Then record
that commit hash and begin CD-002 with a focused regression from the pinned
`COSGN00C.cbl` source proving that license prose containing “copy of” is not a
COPY directive.
