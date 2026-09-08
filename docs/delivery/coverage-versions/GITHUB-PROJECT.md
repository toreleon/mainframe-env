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
| 0.9.0 | `0.9.0` | [#15](https://github.com/toreleon/mainframe-env/issues/15) | — | Planned; open |
| 0.10.0 | `0.10.0` | [#16](https://github.com/toreleon/mainframe-env/issues/16) | — | Planned; open |
| 0.11.0 | `0.11.0` | [#17](https://github.com/toreleon/mainframe-env/issues/17) | — | Planned; open |
| 0.12.0 | `0.12.0` | [#18](https://github.com/toreleon/mainframe-env/issues/18) | — | Planned; open |
| 0.13.0 | `0.13.0` | [#19](https://github.com/toreleon/mainframe-env/issues/19) | — | Planned; open |
| 0.14.0 | `0.14.0` | [#20](https://github.com/toreleon/mainframe-env/issues/20) | — | Planned; open |
| 0.15.0 | `0.15.0` | [#21](https://github.com/toreleon/mainframe-env/issues/21) | — | Planned; open |
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
