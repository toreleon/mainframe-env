# Changelog

All notable changes to mainframe-env are documented here.

## [Unreleased]

## [0.1.1] - 2026-08-31

### Added

- Accepted the bounded mainframe-env 0.1 greenfield product contract.
- Added the deterministic ME.V0 scope, oracle, profile, package, and evidence entry pack.
- Added Rust 2024 workspace, release-version authorities, and architecture/profile checks.
- Certified the generic COBOL, CICS, Db2, IMS, MQ, dataset, JES, RACF, restart,
  backup/restore, security, and overload capabilities with the complete
  CardDemo corpus. CardDemo remains conformance data and tooling, not a shipped
  application feature.
- Added owned, hash-pinned `COCRDSEC` demo source for the upstream `CDV1`
  orphan with an explicit no-card-data correction contract.

### Changed

- Promoted the product and all workspace crates to 0.1.1.
- Corrected corpus-tooling `CARDEMO` spellings to `CARDDEMO`. The typo in the
  pinned upstream FTP JCL remains only as an explicit compatibility alias.

## [0.1.0-alpha.0] - Unreleased

Initial development identity. This version is not published and makes no production-readiness claim.
