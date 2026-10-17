# Explicit native MQ root custody cutoff

- Status: Proposed; external implementation draft, execution evidence pending
- Date: 2026-10-10
- Owner: configured COBOL root driver, selected MQ authority and store adapters
- Scope: one actual synchronous native root and its one selected MQ authority
- Applies from: mainframe-env explicit native MQ root custody profile

## Decision

Install a nonserializable, noncloneable `NativeMqRootManager` through privileged
Rust setup before the exact configured host begins native drive. A retained root
must come from the existing genuine `NativeRootHooks` compiled admission. The
manager retains the actual configured physical host, original control, store and
selected service. Matching identifiers, copied `RootDriverClaim`, a filename,
another router, an ordinary grant and completed core rows cannot construct this
custody. No public application route or ProductServer fallback is added.

`cutoff_live_root` accepts the full original invocation and checks the genuine
retained compiled frame, actual native root membership, exact current execution
and every retained native frame. The first profile refuses preparing, foreign or
multiple root topologies. It makes the actual root and selected service gates
irreversibly revoked and revokes all retained frame gates before asking the same
physical store to capture and fence the old owner. Escaped points, ABI aliases,
HCONN/HOBJ and message handles cannot regain live transport authority. Existing
original CALL, audit, SAF, canonical request/result and physical intent guards
remain necessary; no anonymous writer path is introduced.

## Reentrancy and time

The original cold test's callback runs while the selected MQ service or old frame
may be locked. Its trusted clock also holds its hook mutex during `now_tick`.
The manager does not reacquire those locks or call ordinary `now_tick` or execution
control from that callback. It calls the optional non-callback `custody_tick` on
the same exact configured trusted clock Arc. The clock must supply its current
durable logical tick from its actual owned authority without callback or service
reentry. The default is `Unsupported`; copied or stale caller ticks supply no
fallback. The original test clock can implement this from its existing actual
AtomicU64 source without changing `now_tick`, hook timing, original assertions or
timeouts. Backend floor and original deadline checks still apply.

Revocation uses irreversible atomics and no backend callback. In-flight provider
paths recheck that gate at replay, physical-publication preparation, private
candidate adoption and return. A write that linearizes before the physical fence
is part of the complete preserved unknown state; an old write cannot publish
after that fence. Private old heap state does not become new live authority.
Backend publication and all future ordinary core/provider writers continue to
check the actual retained ownership phase inside the sole physical transaction.

## Storage and preservation

Memory and SQLite atomically capture the exact Open root graph, its entire
actor/event/effect/outbox set, complete configured provider rows and scope rows,
the actual effect-audit rows and all actor/run/scope/registered-row indexes.
Scheduled work, checkpoints and recovery leases remain outside the accepted
synchronous profile and refuse. Index lookup is exact; no truncated global page
or actor count stands in for membership. The typed capture currently reuses the
existing bounded graph container: its field named `closing` contains the actual
captured Open row in this distinct cutoff observation. It is not a Closing gate;
existing terminal publication validation must continue to reject it.

The single transaction writes only the ownership row's next version/Uncertain
phase, a version-one `durable-root-custody-v1` marker binding the exact captured
storage identity and the existing logical clock/epoch. It does not mutate an old
effect, unit, queue payload, CALL row, catalog, audit, terminal event or outbox.
It selects neither commit nor backout. Markers are core owned and anonymous
mutation remains refused. Actual actor, operation and byte ceilings remain
256/4096/64 MiB, additionally narrowed by real row and store quotas. Every extra
captured index/audit and both newly written rows consume that same budget; max+1
refuses as a whole. Unsupported backends refuse without a sequential fallback.

ADR0034 says existing exact outbox delivery remains available for ordinary
Uncertain roots. That policy remains for roots without a custody marker. The new
explicit custody profile additionally freezes old notification delivery while
its unknown graph is held, so the custody's complete old state cannot change
behind a later owning reconciliation. This adds no notification or delivery
credit and changes no notification payload or ordinary Terminal policy.

## Failure and evidence boundary

Failure after volatile revocation keeps old transport fenced and exposes no
custody object or successor. A physically committed but lost acknowledgement
retains the Uncertain row and marker; there is no automatic retry, known-terminal
conversion, old-handle reconstruction or implicit unit disposition. An explicit
owning lost-ack reconciliation contract remains a separate next dependency.
Drop makes no durable decision or cleanup call. Actual process crash/closure
cannot be inferred from a logical SQLite reopen.

The cutoff is a prerequisite, not the original cold reconnect repair. It does
not transfer protected scopes or admit a successor. Versioned independent-root
admission, exact scope CAS, deliberate preserved-unknown-work reconciliation and
complete actual compiled pending PUT/full removed GET oracles remain required.
The historical source05 cold-test input has a handwritten outer `Caller` machine
and an actual compiled `MQCOLD` callee; that outer machine cannot be relabeled a
genuine compiled native root. Its failure and exact source receipt remain
retained. A changed-input original fixture uses the actual compiled `MQNEXT`
root driver and byte-identical nested `MQCOLD` through genuine admission, with
born-owned SQLite manager custody when testing physical reopen. Every original
callback trigger, completion/DONE reply, exact two receipts, unknown outcome,
old-alias refusal and no-second-publication assertion remains unchanged. A
bounded test-only observer inside the same private native program provider
records its actual offer and unchanged returned reply; it grants no authority
and cannot replace the provider or reconstruct a reply from persisted rows.

Physical backend controls and the actual compiled connection cutoff controls
remain separately scoped and unrun in the external draft. They earn no CardDemo,
full native recovery, IBM semantic, licensed or Foundation completion credit.
Original pinned IBM MQ9.4 MQI and COBOL6.5 publication bodies remain unavailable;
changes to commit/backout, cold CONNECT results, HCONN/HOBJ language behavior and
crash disposition require those original sources and their applicable oracle.
