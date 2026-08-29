# CardDemo-full 0.1.1 Program Status

Current phase: **CD.A — corpus and application packaging**
Current issue: **CD-006 — exact COBOL core execution semantics PASS**
Next issue: **CD-007 — file, linkage, and CALL semantics**
Current product: **0.1.0-alpha.0**
Target product: **0.1.1**
CardDemo source: clean `59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e`

## Source identity

- mainframe-env base commit: `aaec5b737e0b90b98b5ad83ad129c504bfe72c37`
- current deterministic dirty-tree identity:
  `sha256:0b4fa17d9a8a659163cb9da26a66f2d1b598333e7224e7e1eabb0abf095a1978`
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
- Completion commit identity:
  `852aa654547a6df68c876e06b6f63f42559922b0`.
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

### CD-003 — complete source closures and owned compatibility copybooks

- State: **PASS**
- Required commit subject: `Load complete CardDemo source closures`
- Completion commit identity:
  `11d32398021b2240a95f757eec152a7607cba3a0`.
- Evidence: `conformance/0.1.1/evidence/issues/CD-003.json`
- Evidence digest:
  `sha256:b3326477beeb8c0f393ecb4393d057f821728d460fba5754f6cf5d03da53eb14`
- Acceptance: explicit library order is part of additive source identity while
  legacy one-file identities remain unchanged. Compiler, repeatable CLI
  libraries, the composed JES `SYSIN`/`SYSLIB*` route, and the public future
  installer input all use the same bounded contract.
- The corpus gate derives 44 unique program closures, 62 application/BMS
  copybooks, nine owned compatibility definitions, seven ordered libraries,
  and 346 expansions with no placeholder or native fallback.
- Missing, duplicate, ambiguous, invalid, symbolic-link, and unassigned inputs
  fail before artifact publication. The real login now reaches the expected
  CD-004 semantic `DuplicateName` failure.

### CD-004 — hierarchical data scopes and exact layouts

- State: **PASS**
- Required commit subject: `Implement CardDemo COBOL data layouts`
- Completion commit identity:
  `8697e1d1fbe2001415c560eb227325bd2e64bb2e`.
- Evidence: `conformance/0.1.1/evidence/issues/CD-004.json`
- Evidence digest:
  `sha256:728eb45dd6cf917e16f25da43bf3b29b5d102da94ef359d7f73ec886de865bc8`
- Acceptance: all 44 closures produce semantic models with 15,296 qualified
  layouts, 89 duplicate-name sets resolved by hierarchy, 808 redefinitions,
  23 variable OCCURS entries, 1,160 condition names, 54 file records, and 185
  linkage items.
- The exact length-delimited layout digest is
  `sha256:0b7f24c44302ac0c62ec306e2d79d9f672f7bdf55a1d01a2b67451dd17fe4460`;
  maximum bounded program storage is 176,272 bytes.
- OF/IN qualification, group extents, levels 66/77/78/88, conditions,
  REDEFINES, OCCURS DEPENDING ON, indexes, subscripts, reference modification,
  sections, and reached display/edited/binary/packed bytes have focused tests.
- The full login now clears semantics and stops at CD-005 HIR
  `UnknownStatement`.

### CD-005 — typed structured COBOL control flow

- State: **PASS**
- Required commit subject: `Build structured CardDemo COBOL control flow`
- Completion commit identity:
  `aaec5b737e0b90b98b5ad83ad129c504bfe72c37`.
- Evidence: `conformance/0.1.1/evidence/issues/CD-005.json`
- Evidence digest:
  `sha256:759e61a9a56883c64b632916b33e17cd627fd326270a5bf0eb7bc68a135937b6`
- Acceptance: all 44 closures now produce HIR with 11,117 typed statements,
  14,317 control nodes, and 18,983 edges. Nested IF/EVALUATE/SEARCH and
  inline PERFORM scopes retain parents, branches, explicit or implicit ends,
  and 1,205 true, 1,207 false, and 44 loop edges.
- Paragraph PERFORM/THRU and GO TO/NEXT SENTENCE resolve 1,201 call,
  1,201 return, and 201 transfer edges. EXIT, GOBACK, STOP RUN, labels,
  branch markers, and generic END-* terminators keep distinct identities.
- Unknown, unmatched, unterminated, and missing-target input fails HIR. The
  pinned corpus's one duplicate paragraph becomes an unsupported recovered
  node; multiline structured control remains unsupported until CD-006 consumes
  its graph, so neither condition can publish partial behavior.
- The exact structured-control digest is
  `sha256:f0a73e52901638049da34143b8456768c0b7fb8df541ba57496cf71d8de143cb`.
  The full login reaches HIR completion and stops at the typed CD-006
  `StructuredControl` boundary.

### CD-006 — exact COBOL core execution semantics

- State: **PASS**
- Required commit subject: `Execute CardDemo COBOL core semantics`
- Completion commit identity: recorded when CD-007 starts from this issue
  commit.
- Evidence: `conformance/0.1.1/evidence/issues/CD-006.json`
- Evidence digest:
  `sha256:30752d833bc42b004b693fd5f562a4caf7fc9bd129bde076cc28daeba2efc130`
- Acceptance: executable IR now carries picture, digits, scale, sign, element,
  occurrence, parent, and condition metadata over one shared program-storage
  base, preserving group, child, REDEFINES, rename, subscript, and reference
  overlap.
- Checked decimal execution covers 1,111 display, 32 edited, 118 packed, and
  984 binary layouts; 1,244 signed and 226 scaled items use exact scale,
  sign/overpunch, precedence, truncation, rounding, and overflow behavior.
- MOVE/group MOVE, STRING, UNSTRING, INSPECT, INITIALIZE, figuratives,
  conditions, 1-based subscripts, reference modification, and all 12 reached
  intrinsic functions have exact positive and typed negative controls.
- Multiline IF/EVALUATE/PERFORM graphs now lower and execute, including branch
  convergence, VARYING loops, and schema-2 checkpointed loop state with
  schema-1 read compatibility. The login closure produces a legal artifact
  with no diagnostics.
- The exact core-shape digest is
  `sha256:b8272836a041f06233c5c948b3bbf8153e2d102314c6fcd8bedc81a2642d0b06`;
  two exact executable oracles produce
  `sha256:173a2d4b676bff2baf13a4fa0aaa8183a29751cf4b93d36fa8f1353ccdedd278`.

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
- `cargo test -p mainframe-env-source -p mainframe-env-compiler -p mainframe-env-cli -p mainframe-env-server -p mainframe-env-conformance -p xtask --locked`
  — **0**; 6 source, 18 compiler, 3 CLI, 10 server, 19 conformance, and
  5 xtask tests passed.
- `cargo check --workspace --all-targets --locked` — **0**.
- Affected-package Clippy with `-D warnings` — **0**.
- `CARDEMO_CORPUS_DIR=<redacted-local-checkout> cargo xtask carddemo-closure --check`
  — **0**; exact CD-003 closure receipt passed.
- `env -u CARDEMO_CORPUS_DIR cargo xtask carddemo-closure --check` — **1**;
  expected missing-environment failure.
- Full-library CLI login compilation — **1**; expected next-phase semantic
  `DuplicateName`, proving the source closure is complete.
- JES fixed `SYSIN` plus `SYSLIB` selected-route test — **0**; exact `HELLO`
  execution output.
- `cargo check --workspace --all-targets --locked` — **0**.
- Affected compiler/conformance/CLI/server/xtask tests — **0**; 58 passed.
- Affected-package Clippy with `-D warnings` — **0**.
- `CARDEMO_CORPUS_DIR=<redacted-local-checkout> cargo xtask carddemo-layout --check`
  — **0**; exact CD-004 layout receipt passed.
- `env -u CARDEMO_CORPUS_DIR cargo xtask carddemo-layout --check` — **1**;
  expected missing-environment failure.
- Full-library CLI login compilation — **1**; expected next-phase HIR
  `UnknownStatement`.
- Final format, JSON, redaction, diff, ordinary conformance, and cumulative
  CD-001–CD-004 gates — **0**.
- `cargo test -p mainframe-env-compiler -p mainframe-env-conformance -p mainframe-env-cli -p mainframe-env-server -p xtask --locked`
  — **0**; 27 compiler, 19 conformance, 3 CLI, 10 server, and 5 xtask tests
  passed.
- `cargo check --workspace --all-targets --locked` and affected-package Clippy
  with `-D warnings` — **0**.
- `CARDEMO_CORPUS_DIR=<redacted-local-checkout> cargo xtask carddemo-control --check`
  — **0**; all 44 HIR models and the exact CD-005 receipt passed.
- `env -u CARDEMO_CORPUS_DIR cargo xtask carddemo-control --check` — **1**;
  expected missing-environment failure.
- Full-library CLI login inspection — **0**; HIR completion reaches the typed
  CD-006 `StructuredControl` boundary without artifact publication.
- Ordinary conformance and cumulative CD-001–CD-005 gates — **0**.
- `cargo test -p mainframe-env-compiler -p mainframe-env-interpreter -p mainframe-env-conformance --locked`
  — **0**; 27 compiler, 6 interpreter, and 26 conformance tests passed.
- Workspace check and affected-package Clippy with `-D warnings` — **0**.
- `CARDEMO_CORPUS_DIR=<redacted-local-checkout> cargo xtask carddemo-core --check`
  — **0**; 44 corpus shapes and two exact executable oracles passed.
- `env -u CARDEMO_CORPUS_DIR cargo xtask carddemo-core --check` — **1**;
  expected missing-environment failure.
- Full-library CLI login compilation — **0**; a legal artifact was emitted with
  no diagnostics after structured CFG and core semantics lowering.
- Ordinary conformance and cumulative CD-001–CD-006 gates — **0**.

## Implementation status

- Issues passed: **6 / 27**
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

Neither decision blocks CD-007. No CD-006 blocker remains.

## Next smallest executable step

Implement typed SELECT/FD bindings, FILE STATUS and AT END/INVALID KEY updates,
LINKAGE storage, and by-reference CALL mutation. Start with the pinned
CBSTM03A-to-CBSTM03B and CSUTLDTC selected routes before generalizing the host
program and dataset effects.
