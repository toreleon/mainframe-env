# GitHub Project synchronization

The operational roadmap is tracked in
[mainframe-env Roadmap](https://github.com/users/toreleon/projects/3).

GitHub Project is the coordination view for version progress, ownership,
priority, and release gates. Repository plans, executable tests, CI,
conformance ledgers, and immutable release artifacts remain the technical
authorities. Do not duplicate row-level verdicts, evidence receipts, artifact
hashes, or test counts into Project fields.

## Version mapping

| Version | Milestone | Epic | Implementation PR | State |
|---|---|---|---|---|
| 0.1.1 | `0.1.1` | [#25](https://github.com/toreleon/mainframe-env/issues/25) | — | Local release; closed |
| 0.2.0 | `0.2.0` | [#8](https://github.com/toreleon/mainframe-env/issues/8) | [#1](https://github.com/toreleon/mainframe-env/pull/1) | Published; closed |
| 0.3.0 | `0.3.0` | [#9](https://github.com/toreleon/mainframe-env/issues/9) | [#3](https://github.com/toreleon/mainframe-env/pull/3) | Published; closed |
| 0.4.0 | `0.4.0` | [#10](https://github.com/toreleon/mainframe-env/issues/10) | [#6](https://github.com/toreleon/mainframe-env/pull/6) | Published; closed |
| 0.5.0 | `0.5.0` | [#11](https://github.com/toreleon/mainframe-env/issues/11) | [#4](https://github.com/toreleon/mainframe-env/pull/4) | Published; closed |
| 0.6.0 | `0.6.0` | [#12](https://github.com/toreleon/mainframe-env/issues/12) | [#5](https://github.com/toreleon/mainframe-env/pull/5) | Published; closed |
| 0.7.0 | `0.7.0` | [#13](https://github.com/toreleon/mainframe-env/issues/13) | [#2](https://github.com/toreleon/mainframe-env/pull/2) | Published; closed |
| 0.8.0 | `0.8.0` | [#14](https://github.com/toreleon/mainframe-env/issues/14) | [#30](https://github.com/toreleon/mainframe-env/pull/30) | Tagged `mainframe-env-v0.8.0`; GitHub Release not published |
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

Issue and pull request numbers share one sequence. Epics occupy #8–#25,
pull requests occupy #1–#7 and #26–#31 and #41, and review issues occupy
#32–#40.

## Pull request history

Pull requests are not Project items. The board tracks issues, and a pull
request reaches it through the read-only `Linked pull requests` field on the
epic it closes. That field is populated only by a closing keyword in the pull
request description, so a pull request that closes no issue does not appear on
the board at all. This table is therefore the durable record. Merge commits are
the permanent identity; branches are deleted after merge.

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
| [#41](https://github.com/toreleon/mainframe-env/pull/41) | `docs/consolidate-pr-epic-history` | open | — | — | This history consolidation |

`2926c54` from #29 is the integrated candidate that
[the 0.8.0 status report](status/0.8.0.md) records as the starting point for
`impl/0.8.0`.

Infrastructure and documentation pull requests — #26 through #29 and #41 —
close no epic, so this table is their only record.

### Pull request metadata

Because pull requests are not board items, their `Labels` and `Milestone`
carry the categorization the Project fields would otherwise provide. Every
pull request has exactly one type label and one milestone.

| Label | Meaning | Pull requests |
|---|---|---|
| `type:implementation` | Delivers a version's work packages | #1, #2, #3, #4, #5, #6, #30 |
| `type:release` | Promotes a version to a release commit and tag | #7, #31 |
| `type:ci` | Continuous integration or tooling | #27, #28, #29 |
| `documentation` | Repository documentation only | #26, #41 |

Milestone is the version whose cycle the pull request merged in, so
infrastructure and documentation work is accounted for rather than orphaned.
That is why #26 through #29 carry `0.8.0` despite predating the 0.8.0
implementation: they merged during that cycle.

## Review issues

Rule 3 permits expanding an epic into child issues once a version becomes
active. The 0.8.0 review is the first expansion.

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
Project carries only priority and status.

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

- 0.8.0 was tagged `mainframe-env-v0.8.0` by release
  [#31](https://github.com/toreleon/mainframe-env/pull/31) on 2026-09-04 while
  its milestone still carries nine open review issues, three of which (#32,
  #33, #34) are confirmed defects against 0.8.0 exit-gate claims. No GitHub
  Release is published for 0.8.0, so rule 6 is satisfied in letter: the tag
  exists, the published release does not.
- Epic [#14](https://github.com/toreleon/mainframe-env/issues/14) (0.8.0) was
  closed automatically by the `Closes #14` keyword in implementation pull
  request #30, not by a decision that the release contract was met. Rule 4
  linkage and rule 6 closure are in tension whenever an implementation pull
  request carries the keyword; a release-only keyword would avoid it.
- The licensed z/OS 3.2/JES2 differential stands at 0/16 pending for 0.8.0
  under the approved `pass-with-licensed-differential-pending` disposition. It
  is a hard gate for
  [#23](https://github.com/toreleon/mainframe-env/issues/23) (0.17.0) and must
  clear before 1.0.0.
