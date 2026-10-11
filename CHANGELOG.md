# Changelog

Current development changes are recorded in `changes/unreleased/` and the owning
subsystem documentation. Historical release records have been cleared for the
public Mainframe Sandbox baseline.

## [Unreleased]

### Fixed

- Honor IDCAMS KSDS `RECORDSIZE(avg max)` on the server DEFINE route and use
  `RECORDSIZE(4089 4089)` when omitted on both server and batch KSDS routes.
  Oversized WRITE/REWRITE returns `LENGERR` (22/0). Reject batch NEW DDs without
  DCB/RECFM/LRECL or a usable referback with a typed diagnostic rather than
  inventing Variable/32760 attributes (#273).
  IBM z/OS 3.2 DCB (`xdddcb.htm`) and IDCAMS RECORDSIZE (`da6i2118.htm`) topics
  were unavailable in the offline cache; behavior follows the issue text and
  its authorized continuation policy. The omission default is the longstanding
  IDCAMS default, not independently verified from those cached topics.
