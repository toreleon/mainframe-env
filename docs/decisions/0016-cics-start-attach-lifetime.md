# ADR-0016: Keep START ATTACH work noncancelable and address-bearing

Status: **Proposed for v0.9 development; FROM route pending**
Owner: **CICS, compiler, and runtime maintainers**
Scope: **START ATTACH row 0206**
Applies from: **mainframe-env 0.9.0 development**

## Context

Pinned CICS TS 6.x baseline `ibm-cics-ts-6x-2026-08-31`, catalog
`api-commands:0206`, topic `dfhp4_startattach.html`, retained raw HTML
`sha256:ebda4b40984414871bfa2fb294104dc5aefa81599a99351c94cdb4974f3d4b3a`
defines an immediate local non-terminal task with STARTCODE `U`, null
EIBREQID, and no cancellation. FROM passes a live address for the attached
task's RETRIEVE; CICS does not copy that data through temporary storage. The
exact retained HTML was hash verified and read through the repository
PlainText parser before this implementation.

The existing durable `cics-start-v1` worker already launches facility-less
tasks, but its ordinary START rows can be canceled and copied data is stored
in the interval record. Those properties cannot represent an attached task.

## Decision

1. Append state codes 6 and 7 (`AttachedPending` and `AttachedReady`) to the
   existing interval record codec. Codes 1–5 and their bytes retain their
   original meanings. The worker promotes attached work idempotently; CANCEL
   rejects it in both states.
2. The no-FROM typed route authorizes and resolves a local target before
   scheduling immediate work. It never emits EIBREQID. The launched task
   receives a trusted `U` start-code binding.
3. FROM and LENGTH remain explicitly closed until a durable live-address
   authority can expose the parent's storage to the attached task at
   RETRIEVE time. A copied snapshot would change the pinned IBM behavior.

## Consequences

The no-data route is executable and source-defined, but row 0206 does not
receive whole-command semantic credit. The attached work uses existing
durable worker leases, replay identity, SAF, and installed target authority.
The source's PURGETHRESH discard behavior requires a future TRANCLASS model.

## Verification

Focused memory, SQLite reopen, cancel-denial, malformed state-tag, MCEP v1/v2,
compiler rejection, and selected compiled COBOL launch tests cover this
bounded route. Module/API/typed-semantic/docs/fmt/dependency/diff gates apply.
