# Changelog

Current development changes are recorded in `changes/unreleased/` and the owning
subsystem documentation. Historical release records have been cleared for the
public Mainframe Sandbox baseline.

## [Unreleased]

### Fixed

- RETRIEVE compatibility requests consume durable START data once and return ENDDATA when exhausted, keeping continuation COMMAREA separate (#197).
