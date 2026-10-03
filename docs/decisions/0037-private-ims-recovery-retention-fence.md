# ADR-0037: Private IMS recovery retention fence

Status: **Proposed**
Owner: **IMS provider and core-server retention maintainers**
Scope: **IMS-1405.private-recovery-retention-fence, existing unowned core dependency fence**
Applies from: **mainframe-env 0.14.0 development**

Selected application recovery checks the exact canonical core effect before
serving private recovery replay. Private `ims-recovery-v1-` aggregates retain
checkpoint, LOG and backout references but lack accepted retention age and
original core owner attribution. Ordinary `ims-v1-replay` pruning can therefore
leave canonical effects apparently reclaimable while private replay still needs
them. Hashed recovery addresses, session termination and sequence counters are
not owner or expiry authority.

The user-authorized leaf adds an IMS-owned opaque presence predicate: the
existing prefix query requests at most one row. Any reserved-prefix row,
including malformed payloads, unknown versions and future subnamespaces, sets
the existing unowned dependency fence. Query failure propagates; it never
certifies absence. Absence is only a successful empty query. The server ORs this
answer with existing dependencies between the existing provider epoch reads;
the existing store transaction rechecks that exact epoch before archive/delete.
This is a conservative host safety rule, not an IBM retention lifetime.

The four affected existing families are `resolved-effects`, `terminal-work`,
`lifecycle-events` and `terminal-executions`. Any private row blocks those
families globally, including unrelated owners, indefinitely. Forecasts report
protection, not reclaimable capacity. Ordinary provider replay can still be
archived with its full codec and existing owner/digest/CAS proofs. No private
cleanup, age, graph decoder, target, coordinator, store, lease, participant or
TM/raw CALL extension is introduced. `audit` and `delivered-outbox` do not gain
this fence. Legacy `ims-state`, ordinary session/checkpoint/session-index/undo
and generic undo namespaces outside the reserved prefix gain no new protection.

The payloads, sixteen targets/order, SQL migrations, canonical encodings,
watermarks and idempotency lifetime remain compatible. Existing older
maintenance binaries ignore this new inventory predicate: drain them before
retention against retained private graphs. Rollback must keep maintenance
disabled or use a compatible fence. Never strip protected rows to regain space.
Private expiry still requires an accepted IMS/store contract for attribution,
age, retry/checkpoint/LOG horizon and archive-before-CAS replacement/deletion.

The bounded proof composes existing SQLite integrity/VACUUM INTO backup with
drained signed selection, real database/checkpoint/undo rows, private LOG,
canonical journals and immutable artifact copying/verified reads. Separate
processes must execute seed, backup and restore observations. This proves only
that selected local composition; it does not certify the v0.16 mixed graph or
PostgreSQL, licensed execution, participant atomicity, parent IMS-1405 or v0.14.

Pinned reference baseline `ibm-ims-15.6-recovery-utilities-2026-09-11`:
`SSEPH2_15.6.0/com.ibm.ims156.doc.apr/ims_logcall.htm` SHA-256
`5106e045b54bca8564d5fc91065ca0dff7b58da4f25d8e09fd11b07d2aada5ca`
(plain lines 95–118: code/data and LOG limits); symbolic CHKP topic
`ims_symbolicchkpcall.htm` SHA-256
`87ede5820b177ea850473dda852c424a7b4a8f5a93dd06c68a0b95c11a4093b5`
(63–85: saved areas, commit/restart, position loss); XRST topic
`ims_xrstcall.htm` SHA-256
`aff46320869b8910ab9916011c8d722a5e044f9e33970d2996e0501204046cb6`
(100–153: prior execution and required log records).
These selected retained topic-path files were absent; matching SHA archive
bytes were verified and parsed locally using ibm_docs.py, then actual search/read.
Catalog `ibm-ims-15.6-dli-2026-08-31/dli-call-families` :0010,
:0023 and :0016/:0025 identify LOG/symbolic CHKP/XRST. No denominator or
official coverage credit changes. Exact source and command receipts stay external.
