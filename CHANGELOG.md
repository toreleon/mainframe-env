# Changelog

Current development changes are recorded in `changes/unreleased/` and the owning
subsystem documentation. Historical release records have been cleared for the
public Mainframe Sandbox baseline.

## [Unreleased]

### Fixed

- Commit installed CALL dataset cursors atomically with retained instance state and the replay
  receipt; reject divergent legacy cursor recovery records instead of restoring stale cursors
  (#189).
