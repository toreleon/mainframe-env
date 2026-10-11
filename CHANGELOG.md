# Changelog

Current development changes are recorded in `changes/unreleased/` and the owning
subsystem documentation. Historical release records have been cleared for the
public Mainframe Sandbox baseline.

## [Unreleased]

### Fixed

- SEND MAP and SEND TEXT now persist valued CURSOR and FREEKB keyboard state
  with their screen and replay receipt (#210). Bare SEND MAP CURSOR continues
  to succeed and selects the first symbolic field with length -1 when available;
  IC/default cursor positioning remains unmodelled.
