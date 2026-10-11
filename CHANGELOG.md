# Changelog

Current development changes are recorded in `changes/unreleased/` and the owning
subsystem documentation. Historical release records have been cleared for the
public Mainframe Sandbox baseline.

## [Unreleased]

### Fixed

- Accept two-character CICS ASSIGN STARTCODE receivers and SEND TEXT WAIT
  completion on the virtual terminal; retain the WRITEQ TS FROM/LENGTH overrun
  rejection with dedicated regressions (#312).
