# Changelog

Current development changes are recorded in `changes/unreleased/` and the owning
subsystem documentation. Historical release records have been cleared for the
public Mainframe Sandbox baseline.

## [Unreleased]

### Fixed

- Dataset reads expose incomplete input through a `more` flag; batch steps and
  other whole-input consumers fail explicitly instead of using a truncated
  record prefix (#186).
