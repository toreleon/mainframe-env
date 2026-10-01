# Coverage and conformance — Coverage authority progress

Subsystem: **coverage**
Phase: **foundation**
Target release: **0.2.0**

Status: **Complete implementation candidate; not released**

The accepted source is the immutable local `mainframe-env-v0.1.1` tag at
`44f3081eb2fdf22d09e1a97725f5a4163431ca70` (tree
`3d504ece02f1c09e124606ded695b00ba984d104`). The accepted CardDemo
certification candidate remains
`857115b907ce7098c965a51117a079048ea8182e` with evidence digest
`sha256:cc7fbcd0867e0654c9f3cd937967c6776413759612ceb652eae2be7083f0b17d`.
Neither historical identity is rewritten by this implementation line.

The implementation branch began from controller-prepared commit
`1afbf402c56c085a46cc3599c91b1b77002b1e16`. Its pre-edit inventory was 315
tracked files, 24 workspace packages, 22 library test targets, the accepted
260-test floor, 29 Db2 H1/H3 scan lines, and three batch H1/H3 scan lines.

## Current work package

CV-201 is complete. All six official PDFs and the three primary IBM HTML
snapshots reproduced the research roadmap digests; the RACROUTE supporting
snapshot is separately pinned. The normalized catalogs contain nine baselines
and 1,506 mandatory rows. IBM publication bytes are not stored in the
repository.

CV-202 is complete. The independent six-gate coverage row, immutable evidence
record, denominator-bound snapshot, and append-only store contracts are frozen.
The initial ledger has 1,506 rows and zero semantic numerators. CV-203 is
complete: all official semantic IDs are generated, while the explicit handler
registry remains empty until typed handlers are installed. CV-204 is
complete with signed, reference-validated, atomically selected package
generations and retained rollback.
CV-205 is complete: the Db2 production H1/H3 scan is 29 to zero, and the
pinned Db2 plus authorization routes pass through selected package catalog
data. CV-206 is complete: the batch production H1/H3 scan is three to zero,
and pinned Db2, IMS, and MQ authorization batch paths pass through exact typed
controllers decoded from the selected signed package. CV-207 is complete: all
nine exact compatibility members moved from compiler ownership to versioned,
licensed CICS, Db2, and MQ source libraries with zero coverage credit. CV-208
is complete: all utility/system-service selection and official/custom route
registration is generated, and deterministic scans prove production
application hardcode and application string-dispatch counts are zero. CV-209
is complete: machine and Markdown ledgers agree, five non-destructive
migrations retain rollback, 299 workspace tests and explicit PostgreSQL 18
controls pass, CardDemo remains 20/20, Zowe CLI 8.36.0 passes live, and local
unpublished 0.2 release artifacts reproduce on candidate
`sha256:cf745ecbfd6a0d8dac3e824174dd1c2a184e8c563fa1004d27809aec9827c6cc`.

The independent PR review repair is complete. Seven findings are closed with
focused regressions: non-destructive Db2 install/upgrade/rollback and primary
key integrity; server-owned HMAC-verified V2 package publication; exact
controller artifact binding plus durable restart/rollback; aggregate and nested
package bounds; and clean-checkout, explicit-target release verification with
full-history/tag CI checkout. The canonical repair receipt is
`conformance/0.2/evidence/review-repair.json`.

The follow-up PR review repair is also complete. Ten additional findings are
closed with focused regressions for exact raw Db2 bytes and defaults, compiled
Draft 2020-12 artifact validation, empty controller generations, signed
program identity enforcement, fully signed Db2 definitions, referentially safe
rollback, allocation-safe package text preflight, and conservative retained
package/controller bounds. The round-1 receipt remains unchanged and is now
validated against completion commit `77119bff9b1bef16ee28e94a2ea9ccdcccc22d6b`;
the distinct follow-up receipt is
`conformance/0.2/evidence/review-repair-round-2.json`.

## Work package ledger

| Work package | State | Evidence |
|---|---|---|
| CV-201 | pass | `conformance/0.2/evidence/work-packages/CV-201.json` |
| CV-202 | pass | `conformance/0.2/evidence/work-packages/CV-202.json` |
| CV-203 | pass | `conformance/0.2/evidence/work-packages/CV-203.json` |
| CV-204 | pass | `conformance/0.2/evidence/work-packages/CV-204.json` |
| CV-205 | pass | `conformance/0.2/evidence/work-packages/CV-205.json` |
| CV-206 | pass | `conformance/0.2/evidence/work-packages/CV-206.json` |
| CV-207 | pass | `conformance/0.2/evidence/work-packages/CV-207.json` |
| CV-208 | pass | `conformance/0.2/evidence/work-packages/CV-208.json` |
| CV-209 | pass | `conformance/0.2/evidence/work-packages/CV-209.json` |

The machine ledger at
`conformance/0.2/evidence/program-status.json` is authoritative for command
receipts, dirty-tree identity, blockers, and the next executable step.

## Open decisions and blockers

None. The z/OSMF 189-row denominator intentionally remains a frozen
heading-level inventory; endpoint normalization creates a new baseline in the
0.11 workstream instead of silently rewriting this denominator.

## Corrections

**2026-09-07 — the CV-201 source-reproduction sentence above is wrong about the
HTML pins, and every pin has since been replaced.** Under "Current work
package" this record says "All six official PDFs and the three primary IBM HTML
snapshots reproduced the research roadmap digests". The six PDF digests did
reproduce. The three HTML digests did not and could not: they were captures of
the rendered DOM, which carries a fresh `lit$<random>$` nonce and an Adobe
`eto_<hex>` nonce on every load, so no retrieval mode we can now identify would
return those bytes twice. The same is true of the separately pinned RACROUTE
supporting snapshot. The original sentence is left standing as the dated record
of what was believed then.

The baselines are no longer read from PDFs at all. Every one of the nine now
pins a manifest of IBM Documentation topics under
`conformance/0.2/manifests/`, fetched from the content endpoint
(`?parsebody=true&lang=en`), which is byte-stable across repeated fetches; the
865 catalog rows that carried `pdf-page:N;outline:TITLE` now carry
`topic:PATH;topic-id:SLUG;heading:TITLE`. The 1,506-row denominator, every
per-unit denominator, and the generated identity set
(`sha256:b659a6e1...`) are unchanged by that migration — locators are not part
of the identity digest. Coverage claims did not move.

**2026-09-07 — "byte-stable across repeated fetches", written one commit above,
holds for eight of the nine baselines and not for db2.** Three consecutive full
re-reads of the 832 Db2 SQL Reference topics reported 1, 1 and 6 topics changed,
naming different topics each time. Every served body was smaller than its pin
and carried an earlier Last Updated date than the pinned 2026-09-03, so this is
a stale edge revision still in circulation rather than IBM editing anything: the
entire difference in `db2z_sql_createview.html` is the date line plus two
`&nbsp;` entities in a cross-reference title. It is a review decision, not a
coverage question — the 174 Db2 rows all resolve, the denominator is untouched,
and no numerator moves either way. `docs/research/publication-source-probe.md`
records the diagnosis in full.

**2026-09-08 — the Db2 run counts one correction above are superseded, and the
cause is no longer open.** That correction says "three consecutive full re-reads
... reported 1, 1 and 6 topics changed". There were four, and they reported
**1, 7, 1 and 6**; the 7-topic run is the one the diagnosis was written from and
it is the run the correction omits. Two documents in this tree giving different
counts for one investigation is the drift these records exist to catch, so the
figure is restated here rather than edited above: the sentence above stands as
what was believed on 2026-09-07.

What the correction above could not yet say is why. It reads as an open review
decision; it is closed. Each of the seven topics the 7-topic run named was
re-read six times and **42 of 42 reads reproduced the pinned sha256**, and twelve
concurrent reads of `db2z_sql_createview` returned the pinned digest as well, so
neither repetition nor concurrency is the trigger. Three signs settle it as an
older build still in circulation rather than as republication: no topic was ever
named by two runs, every stale body was smaller than its pin, and every stale
body carried an earlier `Last Updated` — 2026-01-07 through 2026-05-19 against a
pinned 2026-09-03 — where republication moves that date forward.

One part of the earlier account did not survive re-measurement and should not be
carried forward either. "A second read recovers the pin" is one of two rules, not
the rule: `db2z_sql_createview` served the 2026-01-07 build, 58,675 bytes against
a pinned 58,685, on **13 consecutive reads**. The date is the arm that always
fires. Commit 16829f8 encodes exactly that — a mismatch is read twice, a re-read
that reproduces the pin settles the topic, and otherwise the served date against
the pinned one decides — and a full re-read afterwards reported db2 832/832
topics, 2 changed, 2 `stale-read`, 0 unexplained, exit 0.

Nothing here is a re-pin and nothing here moves a number this document owns. The
174 Db2 row locators still resolve exact — re-run against IBM on 2026-09-08,
`sql-statements` 158/158 and `sql-pl-statements` 16/16, `toc_matches_pin=True`,
`unresolved=0`, exit 0, with 20 matched on the served `h1` and 154 on a
table-of-contents label — the `sql-statements` 158 and `sql-pl-statements` 16
denominators are untouched, and no numerator moves. The measurement itself is
publication-derived and is not in the repository: it was written to
`$TMPDIR/cobolgrammar/db2-staleness-finding.md` and carries zero coverage credit.
