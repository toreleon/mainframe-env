# Private MQ host lifecycle directory

Status: **Proposed**
Owner: **MQ contract and provider maintainers**
Scope: **volatile trusted-service composition, not public route acceptance**
Applies from: **mainframe-env current subsystem contracts**

## Decision

Keep process topology and numeric handle ownership in a bounded private host
directory. Opaque process/frame leases are minted by the already-admitted host
service, never reconstructed from an MQ envelope, arbitrary string hash or
equal principals. A process pins principal identity and decoded host context;
each root processing unit gets non-reused task/thread identities. An exact
repeated invocation gets its existing lease; changed identity cannot silently
rebind a live frame. The directory retains bounded invocation snapshots and
rechecks live cancellation, finite deadlines and exact identity on lookup.

An explicitly admitted CICS child preserves its parent's task ownership. The
child must have passed the actual CICS host loan/frame authority first; parent
fields alone are not attestation. The directory checks parent/run/principal,
context/session, cancellation probe, provider generations and non-widening
limits. IMS or batch subtasks cannot borrow CICS task inheritance. Ending a
nested frame retires no parent handles; the final frame ends the processing
unit. An IMS coordinator's already-resolved syncpoint advances its volatile
owner epoch and retires old nonshared/unassociated handles. Epoch exhaustion
does not mutate either directory or registry.

Shared connections survive processing-unit end, but not process end. The
registry's new host-only `end_process` transition retires every matching
volatile handle, including shared/in-use and default/unassociated forms,
without touching another environment, host or process. It is not application
MQDISC, a SAF permit or a durable transaction decision. Service composition
must hold one authority lock and use its same registry through the existing
pub/sub reclamation guard. Process retirement requires all frames to be gone.

## Sources and compatibility

Baseline `ibm-mq-9.4-mqi-2026-08-31`, catalog rows `0008` MQCONN, `0009` MQCONNX
and `0012` MQDISC. Pinned topics below `SSFKSJ_9.4.0/refdev/` are `q101760_`,
`q101770_` and `q101800_`, reviewed with offline hash-verified search/read from
the retained archive. MQCONNX distinguishes task/thread and process sharing;
MQDISC's source-specific behavior remains separate from host termination.
Existing registry/canonical token identities, default-MQDISC behavior, owned
cold restart, source pins and public provider registrations remain unchanged.
Finite directory capacities are product guards, not IBM numeric constants.

## Remaining acceptance

The directory is volatile and not yet called by public dispatch. It attests no
arbitrary Invocation or origin binding, gives no coordinator privileges, and
cannot supply a durable UOW owner or recovery fence. The selected service must
bind its real host authority, original effect, actual registry, SAF/security
context and separately persisted UOW identity before advertising MQI. Directory
counters are unique only within one host OS process; resetting them on restart
does not establish durable identity freshness or prevent cross-process ABA.
Before exposing restarted dispatch, the service must advance its durably
retained handle-registry epoch under its recovery authority. That service
transition remains unimplemented here. Leases are not serialized. Kernel tests
grant no official call coverage or licensed differential credit.
