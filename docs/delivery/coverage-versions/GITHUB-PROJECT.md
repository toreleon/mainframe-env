# GitHub Project synchronization

The operational roadmap is tracked in
[mainframe-env Roadmap](https://github.com/users/toreleon/projects/3).

GitHub Project is the coordination view for version progress, ownership,
priority, and release gates. Repository plans, executable tests, CI,
conformance ledgers, and immutable release artifacts remain the technical
authorities. Do not duplicate row-level verdicts, evidence receipts, artifact
hashes, or test counts into Project fields.

## Version mapping

| Version | Milestone | Epic | Primary implementation/release PR | State |
|---|---|---|---|---|
| 0.1.1 | `0.1.1` | [#25](https://github.com/toreleon/mainframe-env/issues/25) | — | Local release; closed |
| 0.2.0 | `0.2.0` | [#8](https://github.com/toreleon/mainframe-env/issues/8) | [#1](https://github.com/toreleon/mainframe-env/pull/1) | Published; closed |
| 0.3.0 | `0.3.0` | [#9](https://github.com/toreleon/mainframe-env/issues/9) | [#3](https://github.com/toreleon/mainframe-env/pull/3) | Published; closed |
| 0.4.0 | `0.4.0` | [#10](https://github.com/toreleon/mainframe-env/issues/10) | [#6](https://github.com/toreleon/mainframe-env/pull/6) | Published; closed |
| 0.5.0 | `0.5.0` | [#11](https://github.com/toreleon/mainframe-env/issues/11) | [#4](https://github.com/toreleon/mainframe-env/pull/4) | Published; closed |
| 0.6.0 | `0.6.0` | [#12](https://github.com/toreleon/mainframe-env/issues/12) | [#5](https://github.com/toreleon/mainframe-env/pull/5) | Published; closed |
| 0.7.0 | `0.7.0` | [#13](https://github.com/toreleon/mainframe-env/issues/13) | [#2](https://github.com/toreleon/mainframe-env/pull/2) | Published; closed |
| 0.8.0 | `0.8.0` | [#14](https://github.com/toreleon/mainframe-env/issues/14) | [#30](https://github.com/toreleon/mainframe-env/pull/30) | Published; closed |
| 0.8.1 | `0.8.1` | [#42](https://github.com/toreleon/mainframe-env/issues/42) | [#43](https://github.com/toreleon/mainframe-env/pull/43) | Published; closed |
| 0.8.2 | `0.8.2` | — | [#70](https://github.com/toreleon/mainframe-env/pull/70) | Published source bundle; closed |
| 0.8.3 | `0.8.3` | — | — | Pre-0.9 hardening; in development |
| 0.9.0 | `0.9.0` | [#15](https://github.com/toreleon/mainframe-env/issues/15) | — | Planned; open |
| 0.10.0 | `0.10.0` | [#16](https://github.com/toreleon/mainframe-env/issues/16) | — | Planned; open |
| 0.11.0 | `0.11.0` | [#17](https://github.com/toreleon/mainframe-env/issues/17) | — | Planned; open |
| 0.12.0 | `0.12.0` | [#18](https://github.com/toreleon/mainframe-env/issues/18) | — | Planned; open; recovered foundation in review ([export-recovery track](#export-recovery-track)) |
| 0.13.0 | `0.13.0` | [#19](https://github.com/toreleon/mainframe-env/issues/19) | — | Planned; open |
| 0.14.0 | `0.14.0` | [#20](https://github.com/toreleon/mainframe-env/issues/20) | — | Planned; open; recovered foundation in review ([export-recovery track](#export-recovery-track)) |
| 0.15.0 | `0.15.0` | [#21](https://github.com/toreleon/mainframe-env/issues/21) | — | Planned; open; recovered foundation in review ([export-recovery track](#export-recovery-track)) |
| 0.16.0 | `0.16.0` | [#22](https://github.com/toreleon/mainframe-env/issues/22) | — | Planned; open |
| 0.17.0 | `0.17.0` | [#23](https://github.com/toreleon/mainframe-env/issues/23) | — | Planned; open |
| 1.0.0 | `1.0.0` | [#24](https://github.com/toreleon/mainframe-env/issues/24) | — | Planned; open |

The abandoned 0.7.1 proposal is intentionally absent.

Issue and pull request numbers share one repository sequence. The table is the
version-level mapping, not a promise that every infrastructure or repair pull
request is enumerated. Version 0.8.2 was promoted as a patch/source-bundle
release without a separate epic in this mapping.

## Pull request history

Pull requests are not Project items. The board tracks issues, and a pull
request reaches it through the read-only `Linked pull requests` field on the
epic it closes. That field is populated only by a closing keyword in the pull
request description, so a pull request that closes no issue does not appear on
the board at all. This table is a curated release-history view; Git remains the
complete authority. Merge commits are permanent identities, while branches may
be deleted after merge.

| PR | Branch | Merged | Merge commit | Closes | Result |
|---|---|---|---|---|---|
| [#1](https://github.com/toreleon/mainframe-env/pull/1) | `impl/0.2.0` | 2026-09-01 | `3610140` | #8 | 0.2.0 coverage authority |
| [#2](https://github.com/toreleon/mainframe-env/pull/2) | `impl/0.7.0` | 2026-09-01 | `c6c7589` | #13 | 0.7.0 JCL converter and planner |
| [#3](https://github.com/toreleon/mainframe-env/pull/3) | `impl/0.3.0` | 2026-09-01 | `4b82f55` | #9 | 0.3.0 COBOL structure and type system |
| [#4](https://github.com/toreleon/mainframe-env/pull/4) | `impl/0.5.0` | 2026-09-01 | `b4f8fc7` | #11 | 0.5.0 RACF command language and SAF |
| [#5](https://github.com/toreleon/mainframe-env/pull/5) | `impl/0.6.0` | 2026-09-01 | `d8b1e28` | #12 | 0.6.0 dataset, VSAM, and AMS coverage |
| [#6](https://github.com/toreleon/mainframe-env/pull/6) | `impl/0.4.0` | 2026-09-02 | `c7a07a9` | #10 | 0.4.0 COBOL execution semantics |
| [#7](https://github.com/toreleon/mainframe-env/pull/7) | `codex/official-releases-0.2-0.7` | 2026-09-02 | `7eaabd6` | #8–#13 | 0.2.0–0.7.0 release chain |
| [#26](https://github.com/toreleon/mainframe-env/pull/26) | `codex/github-project-sync` | 2026-09-02 | `00db798` | — | Roadmap and release status sync |
| [#27](https://github.com/toreleon/mainframe-env/pull/27) | `codex/ci-budget` | 2026-09-02 | `9ac7935` | — | Reduced hosted runner usage |
| [#28](https://github.com/toreleon/mainframe-env/pull/28) | `codex/ci-cache-trust` | 2026-09-02 | `e381e40` | — | Read-only pull request caches |
| [#29](https://github.com/toreleon/mainframe-env/pull/29) | `codex/ci-gate-tiers` | 2026-09-02 | `2926c54` | — | Certification gates on explicit runs |
| [#30](https://github.com/toreleon/mainframe-env/pull/30) | `impl/0.8.0` | 2026-09-04 | `26cded1` | #14 | 0.8.0 JES2 runtime and real utilities |
| [#31](https://github.com/toreleon/mainframe-env/pull/31) | `release/0.8.0` | 2026-09-04 | `02fcaed` | #14 | 0.8.0 release commit and tag |
| [#41](https://github.com/toreleon/mainframe-env/pull/41) | `docs/consolidate-pr-epic-history` | 2026-09-05 | `c5fd90c` | — | Project and 0.8.0 review history consolidation |
| [#43](https://github.com/toreleon/mainframe-env/pull/43) | `patch/0.8.1` | 2026-09-05 | `7e32999` | #32–#40 | Resolve every 0.8.0 review finding |
| [#44](https://github.com/toreleon/mainframe-env/pull/44) | `release/0.8.1` | 2026-09-05 | `bc5594f` | — | 0.8.1 release commit, tag, and target receipts |
| [#70](https://github.com/toreleon/mainframe-env/pull/70) | `release/bump-0.8.2` | 2026-09-06 | `316e2e1` | — | 0.8.2 release commit and source-bundle tag |

`2926c54` from #29 is the integrated candidate that
[the 0.8.0 status report](status/0.8.0.md) records as the starting point for
`impl/0.8.0`.

Infrastructure and documentation pull requests #26 through #29 and #41 close
no epic; this table preserves their role in the 0.8.0/0.8.1 planning history.

### Pull request metadata

Because pull requests are not board items, their `Labels` and `Milestone`
carry the categorization the Project fields would otherwise provide. Every
pull request has exactly one type label and one milestone.

| Label | Meaning | Pull requests |
|---|---|---|
| `type:implementation` | Delivers a version's work packages | #1, #2, #3, #4, #5, #6, #30, #43 |
| `type:release` | Promotes a version to a release commit and tag | #7, #31, #44, #70 |
| `type:ci` | Continuous integration or tooling | #27, #28, #29 |
| `documentation` | Repository documentation only | #26, #41 |

Milestone is the version whose cycle the pull request merged in, so
infrastructure and documentation work is accounted for rather than orphaned.
That is why #26 through #29 carry `0.8.0` despite predating the 0.8.0
implementation: they merged during that cycle.

## Review issues

Rule 3 permits expanding an epic into child issues once a version becomes
active. The 0.8.0 review is the first expansion.

The [pre-0.9.0 deep review](../../reviews/PRE-0.9.0-DEEP-REVIEW.md) expands
roadmap epic [#15](https://github.com/toreleon/mainframe-env/issues/15) into 28
independently closable findings: [#101](https://github.com/toreleon/mainframe-env/issues/101)
through [#128](https://github.com/toreleon/mainframe-env/issues/128). The review
index is the authoritative R-ID-to-issue mapping and retains the detailed
evidence and closure criteria.

| Issue | Priority | Finding |
|---|---|---|
| [#40](https://github.com/toreleon/mainframe-env/issues/40) | P0 | Review tracker; child of epic #14 |
| [#32](https://github.com/toreleon/mainframe-env/issues/32) | P0 | Abend state is lost across warm restart |
| [#33](https://github.com/toreleon/mainframe-env/issues/33) | P0 | Warm restart duplicates committed `DISP=MOD` appends |
| [#34](https://github.com/toreleon/mainframe-env/issues/34) | P0 | Utility record framing silently corrupts data |
| [#35](https://github.com/toreleon/mainframe-env/issues/35) | P1 | Datasets are created before their allocation lock is held |
| [#36](https://github.com/toreleon/mainframe-env/issues/36) | P1 | DISP abnormal-termination default does not follow the JCL rule |
| [#37](https://github.com/toreleon/mainframe-env/issues/37) | P1 | `normalize_records` pads fixed records with ASCII, not EBCDIC |
| [#38](https://github.com/toreleon/mainframe-env/issues/38) | P1 | `dispose_dds` failure masks the original abend |
| [#39](https://github.com/toreleon/mainframe-env/issues/39) | P2 | Minor findings: gate scope, spool rollback, dead type, hygiene |

Findings and reproductions stay in [the 0.8.0 review](review/0.8.0.md); the
Project carries only priority and status. They moved to the `0.8.1` milestone
when patch epic [#42](https://github.com/toreleon/mainframe-env/issues/42)
entered implementation, and pull request #43 closed them after its complete
hosted gate passed.

## Plan-track issues

The goal-aligned plan work of 2026-09-24 to 2026-09-26 runs beside the version ladder: it corrects
behaviour that released versions already claim and adds independent evidence,
rather than delivering a new version's surface. Its issues therefore sit
outside the version epics. None carries a milestone yet; the column below
names the version line or epic whose behaviour each issue corrects, so a
milestone can be chosen when the work is scheduled. Priority is the Project's
`Priority` field.

| Issue | Priority | Finding | Roadmap relation | Issue state | Pull request state |
|---|---|---|---|---|---|
| [#237](https://github.com/toreleon/mainframe-env/issues/237) | P1 | SORT honours SORT FIELDS keys and INCLUDE/OMIT, or fails closed | 0.8 JES2 and utilities (#14) | Closed | [#248](https://github.com/toreleon/mainframe-env/pull/248) merged |
| [#246](https://github.com/toreleon/mainframe-env/issues/246) | P1 | Utility handlers declare their accepted operands and fail closed | 0.8 JES2 and utilities (#14) | Closed | [#250](https://github.com/toreleon/mainframe-env/pull/250) merged |
| [#249](https://github.com/toreleon/mainframe-env/issues/249) | P1 | Empty SORTOUT generation makes the next COBOL step abend U0999 | 0.6 datasets (#12) | Closed | [#256](https://github.com/toreleon/mainframe-env/pull/256) merged |
| [#252](https://github.com/toreleon/mainframe-env/issues/252) | P1 | An abended COBOL step loses its DISPLAY output | 0.8 JES2 (#14) | Closed | [#257](https://github.com/toreleon/mainframe-env/pull/257) merged |
| [#253](https://github.com/toreleon/mainframe-env/issues/253) | P1 | Four checks already failing on main 7765d48b | Repository health | Closed | [#258](https://github.com/toreleon/mainframe-env/pull/258) merged |
| [#244](https://github.com/toreleon/mainframe-env/issues/244) | P2 | README status matches the facts | Documentation | Closed | [#247](https://github.com/toreleon/mainframe-env/pull/247) merged |
| [#245](https://github.com/toreleon/mainframe-env/issues/245) | P1 | Per-assertion oracle provenance (ADR-0024) | 0.8 evidence; prerequisite for 0.17 (#23) | Open | [#255](https://github.com/toreleon/mainframe-env/pull/255) closed unmerged; its commits reached main through [#269](https://github.com/toreleon/mainframe-env/pull/269) |
| [#251](https://github.com/toreleon/mainframe-env/issues/251) | P0 | MOVE of a quoted numeric literal to PIC 9 must zero-fill | 0.4 COBOL execution (#10) | Open | [#248](https://github.com/toreleon/mainframe-env/pull/248) merged |
| [#254](https://github.com/toreleon/mainframe-env/issues/254) | P0 | WRITE record-name without FROM writes the record area | 0.4 COBOL execution (#10) | Open | [#248](https://github.com/toreleon/mainframe-env/pull/248) merged |
| [#260](https://github.com/toreleon/mainframe-env/issues/260) | P1 | ADR-0025 licence and provenance policy (Proposed; needs counsel) | Gates licensed evidence for 0.10–0.15, 0.17 (#23) and 1.0 (#24) | Open | [#263](https://github.com/toreleon/mainframe-env/pull/263) open |
| [#261](https://github.com/toreleon/mainframe-env/issues/261) | P1 | Reproducible run bundle for CardDemo READACCT (ADR-0026) | modernize-ai artifact boundary; no version epic | Open | [#282](https://github.com/toreleon/mainframe-env/pull/282) open |
| [#262](https://github.com/toreleon/mainframe-env/issues/262) | P2 | Independent expected outputs for CardDemo base-batch | 0.8 evidence; toward 0.17 (#23) | Open | [#269](https://github.com/toreleon/mainframe-env/pull/269) merged; [#288](https://github.com/toreleon/mainframe-env/pull/288) open |
| [#259](https://github.com/toreleon/mainframe-env/issues/259) | P2 | Semantic differential campaign against GnuCOBOL | 0.4 COBOL execution (#10); development checking only | Open | [#293](https://github.com/toreleon/mainframe-env/pull/293) open |
| [#229](https://github.com/toreleon/mainframe-env/issues/229) | P1 | Numeric-edited MOVE keeps insertion commas inside suppression | 0.4 COBOL execution (#10) | Closed | [#272](https://github.com/toreleon/mainframe-env/pull/272) merged |
| [#231](https://github.com/toreleon/mainframe-env/issues/231) | P1 | CR/DB sign suffix rendering | 0.4 COBOL execution (#10) | Open | [#272](https://github.com/toreleon/mainframe-env/pull/272) merged |
| [#230](https://github.com/toreleon/mainframe-env/issues/230) | P1 | Floating currency `$` in numeric-edited pictures | 0.4 COBOL execution (#10) | Closed | [#283](https://github.com/toreleon/mainframe-env/pull/283) merged; its regression [#366](https://github.com/toreleon/mainframe-env/issues/366) is fixed in open [#370](https://github.com/toreleon/mainframe-env/pull/370), and [#369](https://github.com/toreleon/mainframe-env/issues/369) remains |
| [#287](https://github.com/toreleon/mainframe-env/issues/287) | P1 | Zero value in an all-Z picture prints `.00` instead of spaces | 0.4 COBOL execution (#10) | Closed | [#290](https://github.com/toreleon/mainframe-env/pull/290) merged |
| [#264](https://github.com/toreleon/mainframe-env/issues/264) | P1 | Scaled binary items with VALUE act as zero; TRUNC(STD) | 0.4 COBOL execution (#10) | Open | [#281](https://github.com/toreleon/mainframe-env/pull/281) open |
| [#265](https://github.com/toreleon/mainframe-env/issues/265) | P2 | DIVIDE … ROUNDED and REMAINDER | 0.4 COBOL execution (#10) | Open | [#285](https://github.com/toreleon/mainframe-env/pull/285) open |
| [#286](https://github.com/toreleon/mainframe-env/issues/286) | P1 | Numeric overflow abends instead of truncating (DISPLAY, COMP-3) | 0.4 COBOL execution (#10) | Open | [#291](https://github.com/toreleon/mainframe-env/pull/291) open |
| [#277](https://github.com/toreleon/mainframe-env/issues/277) | P1 | DISPLAY drops a literal that starts with a data name | 0.4 COBOL execution (#10) | Closed | [#280](https://github.com/toreleon/mainframe-env/pull/280) merged |
| [#270](https://github.com/toreleon/mainframe-env/issues/270) | P1 | WRITE to RECORD VARYING … DEPENDING ON writes the full record | 0.4 COBOL execution (#10) | Open | [#275](https://github.com/toreleon/mainframe-env/pull/275) open |
| [#266](https://github.com/toreleon/mainframe-env/issues/266) | P1 | EXEC PARM is not passed to the COBOL main program | 0.8 JES2 (#14) | Open | [#278](https://github.com/toreleon/mainframe-env/pull/278) open |
| [#267](https://github.com/toreleon/mainframe-env/issues/267) | P1 | DCB=(*.ddname) referback is ignored | 0.7 JCL (#13) | Closed | [#274](https://github.com/toreleon/mainframe-env/pull/274) merged |
| [#268](https://github.com/toreleon/mainframe-env/issues/268) | P2 | Tooling tests fail on main | Repository health | Closed | [#284](https://github.com/toreleon/mainframe-env/pull/284) merged |
| [#271](https://github.com/toreleon/mainframe-env/issues/271) | P2 | Confirm `/` and `0` insertion inside zero suppression against IBM | 0.4 COBOL execution (#10); needs an IBM source | Open | — |
| [#273](https://github.com/toreleon/mainframe-env/issues/273) | P1 | New datasets without DCB default silently to Variable/32760 | 0.6 datasets (#12) and 0.8 JES2 (#14); may need an ADR | Open | — |

Pull request and issue states above were read from GitHub on 2026-09-29, after
the merge session that day. Thirteen plan-track pull requests merged into main
(#247, #248, #250, #256–#258, #269, #272, #274, #280, #283, #284 and #290).
#255 did not merge: it was closed on 2026-09-29 because its commits had
already reached main through #269, so its work is incorporated, not separately
merged. Nine remain open: #263, #275, #278, #281, #282, #285, #288, #291 and
#293 target main or a fix branch, and #288's corrected base-batch receipt is
still pending review.

Remaining stacks, from main: #281 → #285 and #281 → #291. Every other open
plan-track pull request now targets main. Issues #271 and #273 have no pull
request.

## Export-recovery track

The 2026-09-22 Mac Codex lanes for 0.12.0, 0.14.0 and 0.15.0 were committed
locally but never pushed. On 2026-09-29 their exported sessions were replayed
onto `main`, one reviewed slice per pull request. Each chain below merges in
order from `main`. The foundations are syntax, contract and in-memory runtime
work. They do not complete any version: every plan stays **Proposed**, and no
slice earns execution, conformance, differential or licensed credit.

| Version | Tracking | Pull requests, in merge order | Waiting on |
|---|---|---|---|
| 0.12.0 Db2 (#18) | [#344](https://github.com/toreleon/mainframe-env/issues/344) | [#345](https://github.com/toreleon/mainframe-env/pull/345) catalog → [#351](https://github.com/toreleon/mainframe-env/pull/351) lexer → [#355](https://github.com/toreleon/mainframe-env/pull/355) AST → [#356](https://github.com/toreleon/mainframe-env/pull/356) transaction → [#357](https://github.com/toreleon/mainframe-env/pull/357) host references → [#358](https://github.com/toreleon/mainframe-env/pull/358) dynamic SQL → [#363](https://github.com/toreleon/mainframe-env/pull/363) expressions → [#365](https://github.com/toreleon/mainframe-env/pull/365) cursor → [#368](https://github.com/toreleon/mainframe-env/pull/368) SELECT → [#372](https://github.com/toreleon/mainframe-env/pull/372) CREATE TABLE → [#374](https://github.com/toreleon/mainframe-env/pull/374) type compatibility | [#350](https://github.com/toreleon/mainframe-env/issues/350) re-pin [#354](https://github.com/toreleon/mainframe-env/pull/354), after [#340](https://github.com/toreleon/mainframe-env/pull/340). Then the catalog is regenerated and float, decfloat and Boolean constants can be un-fenced. |
| 0.14.0 IMS (#20) | [#347](https://github.com/toreleon/mainframe-env/issues/347) | [#349](https://github.com/toreleon/mainframe-env/pull/349) call catalog → [#352](https://github.com/toreleon/mainframe-env/pull/352) SSA → [#359](https://github.com/toreleon/mainframe-env/pull/359) sources → [#362](https://github.com/toreleon/mainframe-env/pull/362) TM contracts → [#364](https://github.com/toreleon/mainframe-env/pull/364) PCB status → [#367](https://github.com/toreleon/mainframe-env/pull/367) metadata → [#371](https://github.com/toreleon/mainframe-env/pull/371) database engine → [#373](https://github.com/toreleon/mainframe-env/pull/373) TM runtime | Review only. Package publication, host routing and TM/database integration were never finished in the lost lane. |
| 0.15.0 MQ (#21) | [#21](https://github.com/toreleon/mainframe-env/issues/21) | [#340](https://github.com/toreleon/mainframe-env/pull/340) call registry and [#337](https://github.com/toreleon/mainframe-env/issues/337) re-pin → [#341](https://github.com/toreleon/mainframe-env/pull/341) syncpoint context → [#343](https://github.com/toreleon/mainframe-env/pull/343) MQI structures → [#348](https://github.com/toreleon/mainframe-env/pull/348) licensed harness (0/26 credit); and [#341](https://github.com/toreleon/mainframe-env/pull/341) → [#346](https://github.com/toreleon/mainframe-env/pull/346) object lifecycle | Review only |

Defects found while recovering and reviewing, each with a fail-first
regression test:

| Issue | Finding | Roadmap relation | Pull request |
|---|---|---|---|
| [#335](https://github.com/toreleon/mainframe-env/issues/335), [#336](https://github.com/toreleon/mainframe-env/issues/336) | Module ceilings exceeded on `main`; string dispatch in `program.rs` | Repository health | [#339](https://github.com/toreleon/mainframe-env/pull/339) |
| [#337](https://github.com/toreleon/mainframe-env/issues/337) | MQINQ topic republished; pinned bytes unavailable | 0.15 MQ (#21) | [#340](https://github.com/toreleon/mainframe-env/pull/340) |
| [#342](https://github.com/toreleon/mainframe-env/issues/342) | MQ object names folded to upper case and trimmed | 0.15 MQ (#21) | [#346](https://github.com/toreleon/mainframe-env/pull/346) |
| [#350](https://github.com/toreleon/mainframe-env/issues/350) | 25 Db2 13 SQL-reference topics republished | 0.12 Db2 (#18) | [#354](https://github.com/toreleon/mainframe-env/pull/354) |
| [#360](https://github.com/toreleon/mainframe-env/issues/360) | Documentation manifest stale on `main` | Repository health | [#361](https://github.com/toreleon/mainframe-env/pull/361) |
| [#366](https://github.com/toreleon/mainframe-env/issues/366) | Floating `$` pictures fail IR verification (regression from #283) | 0.4 COBOL execution (#10) | [#370](https://github.com/toreleon/mainframe-env/pull/370) |
| [#369](https://github.com/toreleon/mainframe-env/issues/369) | Floating strings with embedded `B`, `0` or `/` edit wrongly | 0.4 COBOL execution (#10) | [#376](https://github.com/toreleon/mainframe-env/pull/376), on [#370](https://github.com/toreleon/mainframe-env/pull/370) |
| [#375](https://github.com/toreleon/mainframe-env/issues/375) | ON SIZE ERROR never raised for numeric-edited receivers; floating `+`/`-` capacity one too high | 0.4 COBOL execution (#10) | [#377](https://github.com/toreleon/mainframe-env/pull/377), on [#376](https://github.com/toreleon/mainframe-env/pull/376) |

The v0.9 rows 0027, 0093 and 0114 stay unready. Their source blockers are
recorded on [#297](https://github.com/toreleon/mainframe-env/issues/297).

## Operating rules

1. Keep one epic issue and one milestone per product version.
2. Use `Status`, `Subsystem`, `Gate`, and `Priority` for portfolio-level
   coordination. Avoid adding fields without a recurring planning question.
3. Expand an epic into child issues only when that version becomes active or
   a work package can be assigned and reviewed independently.
4. Link implementation and release pull requests to their epic with a closing
   keyword in the pull request description. Do not add pull requests to the
   board as items; they surface through the epic's `Linked pull requests`
   field. Pull request checks and merge state determine implementation
   completion.
5. Give every pull request one type label and one milestone. Labels and
   milestones are the pull request's categorization because it is not a board
   item.
6. Close the epic and milestone only after the release contract is satisfied.
   A local tag is not represented as a published GitHub Release.
7. Update this mapping when versions are added, removed, or renumbered, and
   when a pull request merges or a version changes state.

## Open deviations

Recorded so the history stays accurate rather than tidy. These are statements
of fact, not proposals.

- 0.8.0 published on 2026-09-04 with nine open review issues, three of which
  (#32, #33, #34) were confirmed defects against claims the release itself
  makes. The defects were disclosed in the GitHub Release known-limitations
  section rather than left implicit. On 2026-09-05, the findings moved to the
  0.8.1 patch milestone so the original milestone and corrective release are
  distinguishable. Pull request #43 resolved and closed all nine findings on
  2026-09-05.
- Epic [#14](https://github.com/toreleon/mainframe-env/issues/14) (0.8.0) was
  closed automatically by the `Closes #14` keyword in implementation pull
  request #30, not by a decision that the release contract was met. Rule 4
  linkage and rule 6 closure are in tension whenever an implementation pull
  request carries the keyword; a release-only keyword would avoid it.
- The licensed z/OS 3.2/JES2 differential remains at 0/16 pending for the 0.8
  line, including 0.8.2, under the approved
  `pass-with-licensed-differential-pending` disposition;
  Hercules, local, and model outputs receive no licensed-equivalence credit.
  The licensed differential is a hard gate for
  [#23](https://github.com/toreleon/mainframe-env/issues/23) (0.17.0) and must
  clear before 1.0.0.
- The version table names `0.8.2` and `0.8.3` milestones, but neither exists
  on GitHub; `VERSION` on main is `0.8.3` and the newest tag is
  `mainframe-env-v0.8.2`. Milestone `0.8.0` is still open with no open issues
  although its version is published and its epic
  [#14](https://github.com/toreleon/mainframe-env/issues/14) is closed.
- The plan-track pull requests (#247, #248, #250, #255–#258, #263, #269,
  #272, #274, #275, #278, #280–#285, #288, #290, #291 and #293) carry no type
  label and no milestone, contrary to rule 5, and their issues carry no
  milestone.
- As of 2026-09-29, issues #245, #251, #254 and #231 are still open although
  the pull requests listed for them have merged (#269, #248 and #272), because
  those pull requests did not use a closing keyword for them. Whether each is
  resolved is an owner decision; this page does not close them.
- The export-recovery pull requests (#339–#377 in the track above) also carry
  no type label or milestone, contrary to rule 5. Most target another
  recovery branch rather than `main`. GitHub applies a closing keyword only
  when its pull request merges into `main`, so each stacked pull request must
  be retargeted to `main` before it merges, or its issue closed by hand.
- Independent checking in #262 and #259 found that the recorded 0.8 CardDemo
  base-batch receipt had captured product output that GnuCOBOL disagrees with
  ([#229](https://github.com/toreleon/mainframe-env/issues/229),
  [#266](https://github.com/toreleon/mainframe-env/issues/266),
  [#267](https://github.com/toreleon/mainframe-env/issues/267),
  [#287](https://github.com/toreleon/mainframe-env/issues/287)). The corrected
  receipt is in pull request #288, which is still open and unmerged as of
  2026-09-29; the evidence it carries is pending, not complete. GnuCOBOL is a development
  reference, not IBM authority, and earns no licensed credit.
