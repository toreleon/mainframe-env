# Engineering reviews

Review work by subsystem and contract boundary. Review guidance is a decision
input; current validation requires a run against the candidate being changed.
Execution receipts and historical acceptance snapshots remain outside Git.

- [Subsystem engineering review](SUBSYSTEM-REVIEW.md): security, durability,
  execution, storage, provider, and tooling risks.
- [CICS command boundaries](CICS-COMMAND-BOUNDARIES.md): PROGRAM, RETRIEVE,
  and PURGE MESSAGE promotion requirements.
- [Current subsystem progress](../delivery/IMPLEMENTATION-STATUS.md): owning
  phases and work packages.

Record repairs with focused regressions and a change report. Update the owning
subsystem status when the implementation changes; a broad test pass alone does
not close a specific finding.
