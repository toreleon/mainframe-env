# Roadmap and contribution audit — 2026-10-11

Status: **Source-qualified planning and tracking snapshot**
Owner: **maintainers through the linked canonical issues**
Scope: **59 existing open issues, 54 roadmap packages and active Foundation work**
Applies from: **this audit snapshot; implementation claims retain their producers**

The review uses main `f2d89a7bdc049055e700b784e31a54d6d57d7ad2` and the open
Foundation PR #393 snapshot `f7a1c964445a00cd9f056b2ddbcf1722e8d31832`.
Its description retains earlier producing-source attribution; it is not a test
at the PR tip. The active Foundation chat was read and its current progress
inspected: at the initial inspection, local schema commit `9627543c` and
integration of child repairs were in progress. The parent subsequently pushed
`164b289ce4367dc0da937c4d06959aa2f15bc651`, which includes that schema commit
and the child repairs. Runtime results remain bound to their earlier producers;
no runtime acceptance at this later head is claimed by this audit.
That checkout and its edits were preserved. Later source/results
must update their owning records rather than inherit this snapshot's identity.

The earlier issue audit reviewed 83 issues and closed 24 with resolution evidence:
20 fixes on main and four branch-scoped results (#394–#397) awaiting Foundation
integration. This review creates nine missing bounded tracking items, taking
its issue baseline from 59 to 68. It does not close additional implementation
issues, rerun runtime acceptance or certify Foundation.

## Findings and corrections

| Finding at audit baseline | Correction / remaining boundary |
|---|---|
| 56 of 59 issues unassigned; active work also lives in chats/drafts | Canonical issue claims and scoped ownership; do not infer availability from an empty assignee |
| Eight issues unlabelled; most had no usable flow state | Fill missing area/impact labels and use one flow label; triage is not proof that work is unstarted |
| 50 issues lacked milestones | Milestones are optional historical groupings; subsystem phase, owner and dependency define delivery |
| Epics #15–#24 linked removed coverage-version plans/prompts | Replace with registered plan/status/prompt authorities; retain old titles as lookup aliases |
| Foundation local TODOs and original journey failures lacked bounded tickets | #405 parent plus seven distinct slices, reusing existing defects and acceptance trackers |
| 54 roadmap packages lacked consistent slice-level claims | Keep their existing nine epics; claim a bounded child with dependencies and acceptance before starting |
| Branch verification and main delivery could be confused | Record delivery separately; #405 tracks branch-only closures #394–#397 through main integration |
| No repository issue forms or PR template | Add defect, bounded-work and decision forms plus a PR evidence/coordination template |
| Board statuses differed from contribution/delivery wording | Define flow and delivery states in one workflow and reference it from the guides |
| Review/check protections and current board adoption unverified | Keep #228 open; branch-protection read returned HTTP 403, so no configuration is claimed |

The forms and workflow guide review; they do not install automatic overlap
rejection, required reviewers, protected branches or project automation.
Actual board access, owner confirmation and protection configuration remain
explicit adoption work under #228.

### Subsequent coordination reconciliation

The authorized follow-up inspects Project #3's 137 items, 17 fields and two
views against 68 open issues and two open PRs. Twelve open issues are absent
from the board, and fifty closed issues retain an active flow column. All 68
issue bodies now contain a current coordination record; each has one flow and
priority label and an area classification. Original reports, checklists,
states and assignees remain intact. Eight native parent links connect
#406–#412 to #405 and #413 to #16.

The active Foundation claim is visible on its reserved children. Preparation
and investigation are not promoted to current-tip runtime acceptance. #412
waits on its stated batch/data-volume prerequisites; the settled #399 decision
is retained. #332/#333 retain their assignees and a required handoff boundary
with #405 before the final integrated gate. Closed #173 is no longer presented
as an active issue blocker for #180; current retained-source/review readiness
still needs triage.

Board migration is prepared for 155 unique items, with flow and delivery
separate. `viewerCanUpdate` is true, but actual field and item mutations are
both denied by the integration credential. No board configuration is claimed.
Branch-protection read remains denied and the accessible ruleset list is empty.
The [project guide](subsystems/GITHUB-PROJECT.md) records the migration, manual
maintenance and unresolved access/automation/administrative obligations. A
subsequent read contains #413 after the explicit parent links, bringing the
board to 138 items; existing content IDs remain preserved and deduplicated.

## Roadmap coverage and implementation state

The [remaining-work inventory](subsystems/REMAINING-WORK.md) contains each
package's detailed implementation and remaining acceptance. Every one of its
54 unique IDs maps to an existing epic below. These are parent trackers, not
54 unclaimed implementation tasks. Missing package-specific evidence means
Unknown, not no code. Completed recovery or normalization slices do not close
broader runtime, application or licensed requirements.

| Owning phase | All six work-package IDs | Canonical epic | Recorded boundary |
|---|---|---|---|
| `cics.application-api` | CIC-901, CIC-902, CIC-903, CIC-904, CIC-905, CIC-906 | [#15](https://github.com/toreleon/mainframe-env/issues/15) | Bounded execution; family acceptance incomplete |
| `cics.system-api` | SPI-1001, SPI-1002, SPI-1003, SPI-1004, SPI-1005, SPI-1006 | [#16](https://github.com/toreleon/mainframe-env/issues/16) | Private preparation; three joins and all public acceptance pending |
| `zosmf.rest` | ZMF-1101, ZMF-1102, ZMF-1103, ZMF-1104, ZMF-1105, ZMF-1106 | [#17](https://github.com/toreleon/mainframe-env/issues/17) | 1101 normalization complete; 1102–1106 portfolio acceptance pending |
| `db2.core` | DB2-1201, DB2-1202, DB2-1203, DB2-1204, DB2-1205, DB2-1206 | [#18](https://github.com/toreleon/mainframe-env/issues/18) | Bounded syntax/values; generic execution and host acceptance incomplete |
| `db2.programming` | DB2-1301, DB2-1302, DB2-1303, DB2-1304, DB2-1305, DB2-1306 | [#19](https://github.com/toreleon/mainframe-env/issues/19) | Full-scope packages pending on core acceptance |
| `ims.programming` | IMS-1401, IMS-1402, IMS-1403, IMS-1404, IMS-1405, IMS-1406 | [#20](https://github.com/toreleon/mainframe-env/issues/20) | Selected DB/TM/recovery exists; complete contexts and application integration pending |
| `mq.programming` | MQ-1501, MQ-1502, MQ-1503, MQ-1504, MQ-1505, MQ-1506 | [#21](https://github.com/toreleon/mainframe-env/issues/21) | Selected/private/installed flows exist; 26 complete-call gates pending |
| `integration.transactions` | INT-1601, INT-1602, INT-1603, INT-1604, INT-1605, INT-1606 | [#22](https://github.com/toreleon/mainframe-env/issues/22) | Early participant boundary exists; mixed-resource acceptance incomplete |
| `certification.licensed` | CER-1701, CER-1702, CER-1703, CER-1704, CER-1705, CER-1706 | [#23](https://github.com/toreleon/mainframe-env/issues/23) | Harness preparation exists; authentic environments/campaigns pending |

Stable release promotion remains separate under #24 (REL-1001–REL-1005).
The profile track #300 has PT-1–PT-3 implemented and PT-4–PT-7 pending.
GenApp gaps #308–#312 and #315/#316/#321 stay owned by their actual language,
CICS, Db2 or JCL authorities. Do not grow application-specific shadow engines
on the profile track.

## Newly bounded work

These tickets describe existing remaining scope and drafts, not new passes or
an instruction to start a second implementation. Seven children are reserved
within the active Foundation work until an explicit split/handoff is recorded.
No new contributor assignment or finish date is inferred by this audit.

| Slice | Canonical issue | Actual state / boundary |
|---|---|---|
| `FOU-TRACK` | [#405](https://github.com/toreleon/mainframe-env/issues/405) | Active parent; preserves existing claims and required delivery to main |
| `FOU-READ-01` | [#406](https://github.com/toreleon/mainframe-env/issues/406) | Observations exist; durable journal/restart completion still under review |
| `FOU-COMPOSITION-01` | [#407](https://github.com/toreleon/mainframe-env/issues/407) | Components/draft exist; original typed publication/readiness acceptance pending |
| `FOU-IMS-01` | [#408](https://github.com/toreleon/mainframe-env/issues/408) | Kernels/recovery exist; typed storage bridge/application acceptance pending |
| `FOU-ONLINE-01` | [#409](https://github.com/toreleon/mainframe-env/issues/409) | 403 remains; PF5/BMS draft lacks original runtime acceptance |
| `FOU-BATCH-01` | [#410](https://github.com/toreleon/mainframe-env/issues/410) | Layout/CCSID failure; actual produced attributes and changed-candidate acceptance pending |
| `FOU-SDK-01` | [#411](https://github.com/toreleon/mainframe-env/issues/411) | 00-submit exceeded original inclusive allowance; responsible subphase unknown |
| `FOU-PERF-01` | [#412](https://github.com/toreleon/mainframe-env/issues/412) | Baseline measurement deliverable; performance acceptance remains a separate decision |
| `SPI-1001-JOINS` | [#413](https://github.com/toreleon/mainframe-env/issues/413) | Private source preparation candidate; claim/cache identities still require confirmation |

#405 reuses #303/#332/#333 for workspace/integration acceptance, #273 for
generic dataset attributes, #186 for truncation, #245/#262 for provenance and
independent batch expectations, #261 for run-bundle boundaries and #260 for the
licence decision. Its local implementation queue stays separate from unavailable
original MQI source, the exact `r_dump.html` architecture topic, licensed
campaigns and Docker capacity. Missing inputs block only dependent steps.

#412 tracks implementation of the settled #399 baseline-measurement decision. It does not
invent a batch/memory/image target or complete #300/PT-5 budget acceptance.

## Parallel boundaries and recommended order

| Work lane | Ownership / integration rule | Safe next action |
|---|---|---|
| Foundation runtime, stores, package composition, online/batch, SDK, IMS bridge | Existing #393 claim; child repairs consume its branch prerequisites | Continue existing reviewed drafts; record any handoff in #405 and the child |
| SPI-1001 private source joins | #413 candidate, main-targeted; no system-source path changes in audited #393 diff | Confirm current cache identities and claim; preserve public fences and coordinate shared generated files |
| Tooling skip-output defect | Existing #353; audited `tools/run_tooling_tests.py` absent from #393 diff | Agree exact remaining colored-output scope and recheck current overlap before claiming |
| GenApp/profile or broader roadmap features | Existing gap/epic; shared compiler/runtime changes can overlap Foundation | Inspect actual shared authority first; preparation does not authorize duplicate runtime work |
| Board/process adoption | #228; independent documentation and authorized project configuration | Review this workflow, confirm owners, and verify actual board/protection access |

Integrate this workflow/documentation PR before dependent implementation PRs
when practical. Independent source-preparation work can target main; Foundation
children target its recorded parent. Regenerate the shared docs manifest from
both sides after integration. Final #393 acceptance and merge remain separate;
this audit neither approves nor merges that candidate.

## Existing open queue: remaining obligation for every issue

These 59 rows retain the previous source-qualified issue-audit dispositions.
They are a triage inventory, not fresh runtime findings. Unassigned items need
claim confirmation; existing #332/#333 assignees remain unchanged. The current
canonical issue and owning progress take precedence over this dated snapshot.

| Existing issue | Remaining implementation, decision or acceptance |
|---|---|
| [#353](https://github.com/toreleon/mainframe-env/issues/353) | Mutation anchor passes 16 tests and tooling discovery passes 4 tests with pinned Python 3.12.13; the runner still parses an uncolored OK(skipped=N) line and does not sanitize Python 3.14 ANSI output. |
| [#333](https://github.com/toreleon/mainframe-env/issues/333) | All 16 queued PRs merged and focused blockers #329/#330/#331 now close; #332 final integrated workspace acceptance remains outstanding. No merge/conflict work should be repeated. |
| [#332](https://github.com/toreleon/mainframe-env/issues/332) | Required metadata fixtures were reconciled and propagated; eight-package evidence exists. Required final integrated workspace build/test acceptance is not established. |
| [#321](https://github.com/toreleon/mainframe-env/issues/321) | Intrinsic argument resolution for EIBTASKN has no completed repair or verified GenApp intake result. |
| [#316](https://github.com/toreleon/mainframe-env/issues/316) | SIGN LEADING SEPARATE requires byte/storage and DISPLAY correction with IBM-backed regression evidence. |
| [#315](https://github.com/toreleon/mainframe-env/issues/315) | Space-separated intrinsic arguments and table/subscript ambiguity were explicitly excluded from #320; IBM-backed parser decision and paired tests remain. |
| [#312](https://github.com/toreleon/mainframe-env/issues/312) | Three GenApp option contracts require separate sourced disposition: STARTCODE receiver, SEND TEXT WAIT, and WRITEQ LENGTH beyond FROM. |
| [#311](https://github.com/toreleon/mainframe-env/issues/311) | Pinned ASKTIME supports the current packed-decimal check; remaining work is the explicit GenApp source-correction/profile decision, not a product-bug claim. |
| [#310](https://github.com/toreleon/mainframe-env/issues/310) | Runtime BMS maps exist; symbolic-map DSECT copybook generation for COPY SSMAP is still missing. |
| [#309](https://github.com/toreleon/mainframe-env/issues/309) | GenApp SQLCA include requires a product-supplied precompiler declaration; parser/SQLCA value models alone do not supply it. |
| [#308](https://github.com/toreleon/mainframe-env/issues/308) | Recognized deferred JOB/EXEC/DD parameters do not yet have the complete per-parameter planning/capability policy and intake evidence. |
| [#303](https://github.com/toreleon/mainframe-env/issues/303) | Scoped lint repairs and affected-package checks exist; exact-candidate strict whole-workspace/all-targets/all-features acceptance for this issue is not established. |
| [#300](https://github.com/toreleon/mainframe-env/issues/300) | PT-1 document, PT-2 intake and PT-3 initial report are delivered; profile journeys, budgets, crash/resume and customer-shape work PT-4 through PT-7 remain. |
| [#298](https://github.com/toreleon/mainframe-env/issues/298) | GET64 SET lifetime and additional cross-CCSID conversion remain explicitly unsupported; #326 fixes task-channel GET SET, not this scope. |
| [#297](https://github.com/toreleon/mainframe-env/issues/297) | Three application rows retain source/registry gaps: CICSMESSAGE callable disposition, GETNEXT TIMER termination/token contract and ISSUE COPY control characters. |
| [#296](https://github.com/toreleon/mainframe-env/issues/296) | BTS/container option combinations and system-event inventory retain explicit fences and have no accepted licensed differential disposition. |
| [#295](https://github.com/toreleon/mainframe-env/issues/295) | PUT64 PREPEND needs an explicit CICS TS level in the profile and tests on both sides of the version gate. |
| [#273](https://github.com/toreleon/mainframe-env/issues/273) | Some DD/record-format repairs exist; no complete verified OPEN-time DCB/IDCAMS RECORDSIZE disposition closes both allocation paths. |
| [#271](https://github.com/toreleon/mainframe-env/issues/271) | Slash/zero insertion inside suppression still needs a pinned IBM semantic disposition; GnuCOBOL behavior alone does not settle it. |
| [#262](https://github.com/toreleon/mainframe-env/issues/262) | Independent-reference promotion is partial (historical 4/90); posting/report derivations and current accepted batch evidence remain outstanding. |
| [#261](https://github.com/toreleon/mainframe-env/issues/261) | #282 delivered a first READACCT bundle; typed effect trace, RunnerResultManifest, actual replay/diff/export boundary and three-way comparison remain incomplete. |
| [#260](https://github.com/toreleon/mainframe-env/issues/260) | ADR-0025 is Proposed; license/provenance governance acceptance is a separate unfinished decision. |
| [#259](https://github.com/toreleon/mainframe-env/issues/259) | #293 delivered grammar generator/runner/minimizer and bounded campaigns; declared INSPECT/level-88/file-I/O scope and PR/nightly CI wiring remain incomplete. |
| [#245](https://github.com/toreleon/mainframe-env/issues/245) | Initial batch provenance policy exists; per-assertion expected-source coverage across all conformance and independence requirements remain broader than the merged increment. |
| [#236](https://github.com/toreleon/mainframe-env/issues/236) | Db2 utility non-goal expansion needs a named data-movement scope and decision before implementation. |
| [#235](https://github.com/toreleon/mainframe-env/issues/235) | DFSORT/ICETOOL expansion needs the named workload profile required by the proposal. |
| [#234](https://github.com/toreleon/mainframe-env/issues/234) | Batch scheduling/network layer proposal has no accepted implementation scope or completed gate. |
| [#232](https://github.com/toreleon/mainframe-env/issues/232) | INDEXED BY names need runtime materialization and SET TO/UP/DOWN, subscript and SEARCH regressions. |
| [#228](https://github.com/toreleon/mainframe-env/issues/228) | Board migration partly applied; workflow repair/current verification and ownership remain. Latest recorded Project access was denied, so old snapshots are not proof of completion. |
| [#225](https://github.com/toreleon/mainframe-env/issues/225) | Normal exhaustion now cancels the unadmitted child, but failed-terminalization recovery, restart and cancellation/admission-race acceptance are not established. |
| [#224](https://github.com/toreleon/mainframe-env/issues/224) | Current compiler admits compact PIC X(6) FORMATTIME while runtime validation still requires FormatTextOutput(8); compiled artifact admission remains inconsistent. |
| [#214](https://github.com/toreleon/mainframe-env/issues/214) | Transfer/frame hardening exists; retained browse/update holds and monotonically owned nested mutation keys across same-task transfer still need the specific proof. |
| [#210](https://github.com/toreleon/mainframe-env/issues/210) | SEND CONTROL has terminal snapshots, but SEND MAP/TEXT cursor and keyboard modeling remains explicitly unmodeled in current routing documentation. |
| [#200](https://github.com/toreleon/mainframe-env/issues/200) | Host-call failure diagnostics still need the issuing statement position rather than the advanced program counter. |
| [#198](https://github.com/toreleon/mainframe-env/issues/198) | Durable ACCUM/page/PURGE infrastructure exists; full multi-page/restart/error/TSIOERR selected-route evidence remains unestablished. |
| [#197](https://github.com/toreleon/mainframe-env/issues/197) | Typed START/RETRIEVE queue consumption exists; empty-argument compatibility retrieval still returns run.retrieve and needs separation from COMMAREA/continuation semantics. |
| [#192](https://github.com/toreleon/mainframe-env/issues/192) | Current inspect_op still interprets the first REPLACING/TALLYING phrase; complete multi-phrase behavior or rejection remains missing. |
| [#190](https://github.com/toreleon/mainframe-env/issues/190) | Known non-success installed CALL instance disposition remains a durable-protocol decision; unknown-outcome preservation must remain. |
| [#189](https://github.com/toreleon/mainframe-env/issues/189) | One atomic cursor authority with the installed CALL replay receipt and interrupted-call recovery has not been established. |
| [#188](https://github.com/toreleon/mainframe-env/issues/188) | cics_numeric_value for typed READ/REWRITE still has no bare numeric-literal branch; other WRITE/browse options do not close it. |
| [#186](https://github.com/toreleon/mainframe-env/issues/186) | Dataset Records lacks a truncation signal and callers retain bounded reads; explicit failure/pagination and >4096 record proof remain required. |
| [#180](https://github.com/toreleon/mainframe-env/issues/180) | The compiler-only bare 3270 SEND compatibility descriptor remains; row-level typed registration is not proof that this exact form was migrated and its descriptor deleted. |
| [#178](https://github.com/toreleon/mainframe-env/issues/178) | CLI/JES product compile options still need a supported SQL INCLUDE expansion interface/default and explicit SQLCA policy. |
| [#160](https://github.com/toreleon/mainframe-env/issues/160) | Owned IBM hexadecimal COMP-1/COMP-2 encoding, raw bytes, aliases and an explicit alternate IEEE profile remain pending. |
| [#159](https://github.com/toreleon/mainframe-env/issues/159) | Expression/receiver-aware division precision/scale and exact candidate workspace evidence remain pending. |
| [#158](https://github.com/toreleon/mainframe-env/issues/158) | IBM ARITH(COMPAT) intermediate precision separated from declaration limits and exact candidate acceptance remain pending. |
| [#157](https://github.com/toreleon/mainframe-env/issues/157) | Federated Hercules/KICKS profile, license matrix, reproducible runtime and comparative report have not met the research decision gate. |
| [#130](https://github.com/toreleon/mainframe-env/issues/130) | Bounded typed-HIR/host-effect slices merged, but the aggregate explicitly remains open for later dynamic/per-family, frontend and exact-candidate acceptance. |
| [#129](https://github.com/toreleon/mainframe-env/issues/129) | Rule candidate/pilot tooling exists; this issue requires the distinct reviewed HTML-example extraction/wrapper/scenario/mutation pipeline and its two-example acceptance. |
| [#24](https://github.com/toreleon/mainframe-env/issues/24) | Stable release freeze, reproducible/signature artifacts, install/rollback, independent approval and authorized publication remain incomplete. |
| [#23](https://github.com/toreleon/mainframe-env/issues/23) | Licensed IBM certification campaigns, cross-resource/scale/recovery matrix and unchanged-source release rehearsal remain unaccepted. |
| [#22](https://github.com/toreleon/mainframe-env/issues/22) | Common coordinator building blocks exist; complete mixed-resource failure, recovery, compatibility and differential matrix remains unaccepted. |
| [#21](https://github.com/toreleon/mainframe-env/issues/21) | MQ catalog/kernel waves merged; full source-backed programming, security/recovery/scale and licensed acceptance remain incomplete. |
| [#20](https://github.com/toreleon/mainframe-env/issues/20) | IMS recovered and nonlicensed kernel/contract slices merged; complete metadata publication, host integration, application journeys and acceptance remain incomplete. |
| [#19](https://github.com/toreleon/mainframe-env/issues/19) | Full Db2 SQL/SQL PL, advanced data/query, routines/bind/distribution and differential scope remains incomplete. |
| [#18](https://github.com/toreleon/mainframe-env/issues/18) | Db2 syntax and bounded scalar/binder models exist; generic relational execution, host ABI/SQLCA and complete acceptance remain incomplete. |
| [#17](https://github.com/toreleon/mainframe-env/issues/17) | Complete generated z/OSMF REST portfolio and protocol/security/failure/scale acceptance remain incomplete. |
| [#16](https://github.com/toreleon/mainframe-env/issues/16) | Private SPI/FEPI preparation is partial with three SPI form/source joins; public execution acceptance remains 0/269 and 0/39. |
| [#15](https://github.com/toreleon/mainframe-env/issues/15) | 260 typed registrations and bounded local proofs are partial command/option coverage; three rows, broader obligations and aggregate acceptance remain incomplete. |

## Adoption and maintenance

Follow the [coordination workflow](WORK-COORDINATION.md) and
[CONTRIBUTING](../../CONTRIBUTING.md). At each intake/handoff, confirm the issue,
owner, source, changed contracts, dependent PRs, PR base and next acceptance step.
Track unrun, failed and externally blocked checks separately. Update the owning
progress record first and regenerate navigation; do not maintain a second
independent runtime-completion counter in the board or this audit.

Checklists and issue labels are review aids. Required runtime, dependency,
architecture, licensed and final-candidate gates retain their existing limits.
All execution receipts remain external with their original source identities.
