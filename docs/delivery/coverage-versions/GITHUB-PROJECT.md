# GitHub Project synchronization

The operational roadmap is tracked in
[mainframe-env Roadmap](https://github.com/users/toreleon/projects/3).

GitHub Project is the coordination view for version progress, ownership,
priority, and release gates. Repository plans, executable tests, CI,
conformance ledgers, and immutable release artifacts remain the technical
authorities. Do not duplicate row-level verdicts, evidence receipts, artifact
hashes, or test counts into Project fields.

## Version mapping

| Version | Milestone | Epic | State |
|---|---|---|---|
| 0.1.1 | `0.1.1` | [#25](https://github.com/toreleon/mainframe-env/issues/25) | Local release; closed |
| 0.2.0 | `0.2.0` | [#8](https://github.com/toreleon/mainframe-env/issues/8) | Published; closed |
| 0.3.0 | `0.3.0` | [#9](https://github.com/toreleon/mainframe-env/issues/9) | Published; closed |
| 0.4.0 | `0.4.0` | [#10](https://github.com/toreleon/mainframe-env/issues/10) | Published; closed |
| 0.5.0 | `0.5.0` | [#11](https://github.com/toreleon/mainframe-env/issues/11) | Published; closed |
| 0.6.0 | `0.6.0` | [#12](https://github.com/toreleon/mainframe-env/issues/12) | Published; closed |
| 0.7.0 | `0.7.0` | [#13](https://github.com/toreleon/mainframe-env/issues/13) | Published; closed |
| 0.8.0 | `0.8.0` | [#14](https://github.com/toreleon/mainframe-env/issues/14) | Planned; open |
| 0.9.0 | `0.9.0` | [#15](https://github.com/toreleon/mainframe-env/issues/15) | Planned; open |
| 0.10.0 | `0.10.0` | [#16](https://github.com/toreleon/mainframe-env/issues/16) | Planned; open |
| 0.11.0 | `0.11.0` | [#17](https://github.com/toreleon/mainframe-env/issues/17) | Planned; open |
| 0.12.0 | `0.12.0` | [#18](https://github.com/toreleon/mainframe-env/issues/18) | Planned; open |
| 0.13.0 | `0.13.0` | [#19](https://github.com/toreleon/mainframe-env/issues/19) | Planned; open |
| 0.14.0 | `0.14.0` | [#20](https://github.com/toreleon/mainframe-env/issues/20) | Planned; open |
| 0.15.0 | `0.15.0` | [#21](https://github.com/toreleon/mainframe-env/issues/21) | Planned; open |
| 0.16.0 | `0.16.0` | [#22](https://github.com/toreleon/mainframe-env/issues/22) | Planned; open |
| 0.17.0 | `0.17.0` | [#23](https://github.com/toreleon/mainframe-env/issues/23) | Planned; open |
| 1.0.0 | `1.0.0` | [#24](https://github.com/toreleon/mainframe-env/issues/24) | Planned; open |

The abandoned 0.7.1 proposal is intentionally absent. Historical pull
requests #1 through #7 are retained in the Project as completed implementation
and release records.

## Operating rules

1. Keep one epic issue and one milestone per product version.
2. Use `Status`, `Subsystem`, `Gate`, and `Priority` for portfolio-level
   coordination. Avoid adding fields without a recurring planning question.
3. Expand an epic into child issues only when that version becomes active or
   a work package can be assigned and reviewed independently.
4. Link implementation pull requests to their epic. Pull request checks and
   merge state determine implementation completion.
5. Close the epic and milestone only after the release contract is satisfied.
   A local tag is not represented as a published GitHub Release.
6. Update this mapping when versions are added, removed, or renumbered.
