# ADR-0015: Shared BTS lifecycle authority

Status: **Proposed for the incremental CIC-904.bts-lifecycle slice**
Owner: **CICS provider and execution maintainers**
Scope: **shared BTS process/activity state for 23 lifecycle application rows**
Applies from: **mainframe-env 0.9.0 development**

## Context

The 23 assigned BTS controls create, acquire, inspect, run, suspend, reset, and
remove process or activity state. Existing event-control rows persist event
pools, but have no process tree or lifecycle owner. The pinned CICS TS 6.x
ACQUIRE, DEFINE PROCESS, and DEFINE ACTIVITY topics make acquisition UOW scoped
and defer definition commitment to a successful syncpoint. Sibling BTS command
families need the same process and activity identity.

## Decision

`mainframe-env-cics::bts_lifecycle` owns one versioned CAS process row per
process-type/name pair. Each row contains the bounded root and descendant
activity tree, monotonically increasing process and activation epochs,
checkpoint references, and exact mutation replay. A separate versioned index
resolves each opaque 52-character activity ID to its process without an
unbounded scan. A per-run-unit acquisition row retains an epoch tombstone
across syncpoints so stale UOW owners cannot recreate an old lease identity.

DEFINE PROCESS writes the pending process, root index, and defining UOW
acquisition atomically. A successful syncpoint publishes the definition and
releases the acquisition atomically; rollback removes the pending process and
root index in the same transaction. The existing CICS UOW coordinator remains
the syncpoint owner. BTS supplies no universal prepare, automatic
compensation, or automatic redispatch after an unknown outcome. The local
participant declaration is `BTS_PARTICIPANT`.

The process row schema and namespace are version 1. Readers reject unknown
fields, invalid relationships, cycles, stale checkpoint epochs, malformed
identities, and excess state. There is no prior BTS process row to migrate.
Rollback to a build without this feature requires stopped admission and a
compatible backup; it must not silently ignore live process/checkpoint rows.
Retention must preserve pending definitions, acquisition tombstones, replay
records, and checkpoint references until their owning UOW and effect windows
close. The initial implementation does not prune those rows automatically.

## Consequences

Every BTS lifecycle and sibling handler uses this authority for process or
activity state. Existing event rows retain only their event-pool content and
will bind to the lifecycle identity when the public command routes land.
The store's atomic provider mutation works on Memory, SQLite, and PostgreSQL
through one contract. No command becomes executable merely by installing the
authority; typed registration follows selected-route tests and acceptance
gates. Licensed differential credit remains pending.

## Sources

IBM CICS TS 6.x baseline `ibm-cics-ts-6x-application-api-sources-a-2026-09-10`,
catalog rows `0002`/`0003` (`dfhp4_acquire.html`, SHA-256
`646a460e9ba3dd34548c5d73939a9a102538fda186d9e1e6b0721c20a12e927a`),
`0032` (`dfhp4_defineactivity.html`, SHA-256
`85c07bc78fc04f6a2496240292a0766033eff1b4f4f0c2ab5daff19402667260`),
and `0037` (`dfhp4_defineprocess.html`, SHA-256
`a31702315bb8d6cb0ac499858593eed0e2d4c2e3b65145474e95a39ecbd71f56`).
The manifest pins and external raw HTML matched and were parsed offline with
`ibm_docs.py`. The full assigned source map is in the 0.9.0 status.
