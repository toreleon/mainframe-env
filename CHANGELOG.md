# Changelog

Current development changes are recorded in `changes/unreleased/` and the owning
subsystem documentation. Historical release records have been cleared for the
public Mainframe Sandbox baseline.

## [Unreleased]

### Fixed

- Make `SET TO`, `SET UP BY`, and `SET DOWN BY` work with `INDEXED BY` names;
  preserve table-bound index displacements and the starting index for serial `SEARCH` (#232).
