# CardDemo-full 0.1.1 Program Status

Current phase: **CD.A — corpus and application packaging**
Current issue: **CD-011 — installed CICS program routing PASS**
Next issue: **CD-012 — CICS screens and conditions**
Current product: **0.1.0-alpha.0**
Target product: **0.1.1**
CardDemo source: clean `59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e`

## Source identity

- mainframe-env base commit: `36162d7d333797b8161092dbedb45220a1fb82c2`
- current deterministic dirty-tree identity:
  `sha256:d4ed524bc073f3cd74ab050cfbefbcb26fc68c35bfc2eeff7b7ed0b7aa0d0331`
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
- Completion commit identity:
  `4342e1b55797adf22c4f55eaf6567cea362a3a80`.
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

### CD-007 — file, linkage, and CALL semantics

- State: **PASS**
- Required commit subject: `Implement CardDemo file and program calls`
- Completion commit identity:
  `91eeac38c62cca69670c91eb1adb5c7d90a8a177`.
- Evidence: `conformance/0.1.1/evidence/issues/CD-007.json`
- Evidence digest:
  `sha256:9a093cf504ddf55d5242da5f48bb2e1783cb3c48f64790305954480402260597`
- Acceptance: 54 lowered SELECT definitions retain DD assignment,
  organization, access mode, record/relative key, and FILE STATUS. The corpus
  has 31 indexed and 23 sequential bindings; focused relative-key coverage is
  explicit even though the pinned program corpus reaches none.
- Dataset effects consume explicit `cobol.dd.<SELECT>` authority or the lowered
  ASSIGN name, carry random keys, mutate READ INTO records, serialize WRITE
  FROM bytes, and update success, EOF, or provider condition status.
- CALL USING sends bounded ordered `mainframe-env.cobol.call@1` values and
  accepts only exact-arity `mainframe-env.cobol.call-result@1` responses before
  mutating caller storage. The selected AB-to-XY roundtrip proves by-reference
  return behavior.
- The exact corpus contract covers 185 linkage items, 63 calls with 228 USING
  operands, 53 OPEN, 50 CLOSE, 34 READ, and 117 WRITE statements. CBSTM03A,
  CBSTM03B, and CSUTLDTC are present; the contract digest is
  `sha256:0cafcb6178b6c027f22fd0182bf6e7c5f74a2af0238f10476566afa4823e2c6e`.

### CD-008 — typed embedded host operands

- State: **PASS**
- Required commit subject: `Lower CardDemo embedded host operations`
- Completion commit identity:
  `b203b9d7e73047af8b74af5d919280cad97316f3`.
- Evidence: `conformance/0.1.1/evidence/issues/CD-008.json`
- Evidence digest:
  `sha256:5c6d97d6499842d32d7063c64f2179917fd435321999bbd63e2abaac92fbb27b`
- Acceptance: CICS literals normalize while storage reads dereference bytes and
  INTO/RESP/RESP2 stay typed mutation destinations. SQL/DLI use a versioned
  family/opcode/name/mode/value envelope; MQ uses named ordered by-reference
  lists. No raw command string is provider authority.
- The exact corpus contract covers 240 CICS, 20 SQL, 26 DLI, and 22 MQ
  operations, 673 typed operands, 232 destinations, every reached opcode, and
  four executable ABI oracles. Its shape digest is
  `sha256:ceb16597bc10703bb52cdf03a40c082d7aa57cdb66314350b98ed289e147d76e`.

### CD-009 — generic content-addressed application packages

- State: **PASS**
- Required commit subject: `Add generic mainframe application packages`
- Completion commit identity:
  `6a8c7ccec048d3e9e2fd5a3f47608962debe8829`.
- Evidence: `conformance/0.1.1/evidence/issues/CD-009.json`
- Evidence digest:
  `sha256:aebf2c87b6891e6018ee13652cfcc30609d5a2635634730338d52aba8c48c317`
- A generic production kernel owns canonical manifests and atomic staging,
  commit, readiness, restart, and idempotent replay. The server composes it
  without CardDemo-specific branches.
- The owned package identity is
  `sha256:e3f0674a667fe38d3b4aa823f73bf83d7ef1bc8af1f9813c18978761aee34860`;
  source, resource, program, data, profile, and migration cohorts are exact.
- Partial, corrupt, orphaned, incompatible, and conflicting installs fail
  before readiness; staged state remains explicitly not-ready.

### CD-010 — BMS and CSD resource catalogs

- State: **PASS**
- Required commit subject: `Install BMS and CICS resource catalogs`
- Completion commit identity:
  `36162d7d333797b8161092dbedb45220a1fb82c2`.
- Evidence: `conformance/0.1.1/evidence/issues/CD-010.json`
- Evidence digest:
  `sha256:c36fa5b51c47af8a2f764736654941b6c2069a56893d7d87b047eaffce580ed3`
- Generic parsers retain BMS continuation/macro/field metadata and CSD resource
  properties. The pinned result is 21 mapsets/maps, 1,164 fields, 25
  transactions, 26 programs, eight files, and one TDQUEUE.
- All transaction references validate except the explicitly unresolved
  `CDV1 -> COCRDSEC` edge; no object or fallback is synthesized. The exact
  resource digest is
  `sha256:246382391f8d8e6f002c903590c841ec3c2c9357ffa07d5e70be41a9116db29c`.

### CD-011 — installed program routing and CICS frames

- State: **PASS**
- Required commit subject: `Route installed CICS programs and transfers`
- Completion commit identity: recorded when CD-012 starts from this issue commit.
- Evidence: `conformance/0.1.1/evidence/issues/CD-011.json`
- Evidence digest:
  `sha256:0f60d3c84f1ccdf34ab6f8a8d88919d48f6d7d08af0de90abfaad06f4f1ba669`
- All 26 installed program definitions resolve immutable exact/latest
  generations. XCTL replaces at constant depth, LINK creates a child, and
  RETURN restores caller COMMAREA bytes.
- The complete invocation context propagates unchanged. The catalog digest is
  `sha256:fddc6f9b6e14e8a26f097f63b34ed020c06768beff65da19f34e0b9f43e2ee3f`.

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
- `cargo test -p mainframe-env-compiler -p mainframe-env-interpreter -p mainframe-env-conformance --locked`
  — **0**; 27 compiler, 6 interpreter, and 28 conformance tests passed.
- Workspace check and affected-package Clippy with `-D warnings` — **0**.
- `CARDEMO_CORPUS_DIR=<redacted-local-checkout> cargo xtask carddemo-file-call --check`
  — **0**; the exact 44-program file/call contract passed.
- `env -u CARDEMO_CORPUS_DIR cargo xtask carddemo-file-call --check` — **1**;
  expected missing-environment failure.
- Ordinary conformance and cumulative CD-001–CD-007 gates — **0**.
- `cargo test -p mainframe-env-compiler -p mainframe-env-interpreter -p mainframe-env-conformance --locked`
  — **0**; 27 compiler, 6 interpreter, and 32 conformance tests passed.
- Workspace check and affected-package Clippy with `-D warnings` — **0**.
- `CARDEMO_CORPUS_DIR=<redacted-local-checkout> cargo xtask carddemo-host --check`
  — **0**; exact CICS/SQL/DLI/MQ operand and opcode receipt passed.
- `env -u CARDEMO_CORPUS_DIR cargo xtask carddemo-host --check` — **1**;
  expected missing-environment failure.
- Ordinary conformance and cumulative CD-001–CD-008 gates — **0**.

## Implementation status

- Issues passed: **11 / 27**
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

Neither decision blocks CD-012. No CD-011 blocker remains.

## Next smallest executable step

Implement BMS SEND/RECEIVE field serialization, AID/cursor/attribute behavior,
and RESP/RESP2/HANDLE/NOHANDLE condition semantics over installed maps.
