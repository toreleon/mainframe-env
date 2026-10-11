# Changelog

Current development changes are recorded in `changes/unreleased/` and the owning
subsystem documentation. Historical release records have been cleared for the
public Mainframe Sandbox baseline.

## [Unreleased]

### Fixed

- Retain durable recovery for exhausted JES child admission when cancellation fails,
  and preserve terminal children across restart and replay (#225).
