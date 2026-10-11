# Changelog

Current development changes are recorded in `changes/unreleased/` and the owning
subsystem documentation. Historical release records have been cleared for the
public Mainframe Sandbox baseline.

## [Unreleased]

### Fixed

- Apply every INSPECT TALLYING and REPLACING phrase in scan order, consuming matched
  characters once and rejecting malformed trailing phrases (#192).
