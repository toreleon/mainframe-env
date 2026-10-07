# Volatile task-root MQI ABI aliases

Status: **Proposed**
Owner: **MQ host/interpreter maintainers**
Scope: **connection alias translation, not executable handle or terminal authority**
Applies from: **mainframe-env current subsystem contracts**

## Decision

An embedding may deliberately supply one `Arc<MqMqiAbiScope>` through the
admitted `MqMqiProgramFrame`. Independently admitted SAME TASK frames must receive
that exact allocation; equal contexts, bindings or integers are not proof of
sharing. Construction is privileged volatile storage, not admission to a root,
SAF, handle registry, UOW or recovery. The selected provider still validates
the original effect and opaque live token against its sole registry and core.

The bounded slots are allocated before frame binding. Each CONN/CONNX reserves
one slot and burns a positive PIC S9(9) BINARY alias before dispatch. Abort,
retirement and reuse never rewind that scope's high-water value. Capacity is an
owned product guard, not an IBM maximum; a full scope refuses a connection call
before dispatch even if its eventual provider response might reuse a token.
Reservations are not observable executable mappings. Known valid replies reuse
an existing exact token's alias or adopt the reserved slot. Successful DISC
retires the alias across frames, without overwriting undefined z/OS Hconn bytes.
CMIT/BACK preserve aliases and use the current unit supplied by the admitted
frame; the alias table neither selects nor decides a UOW.

Result encoding and allocation precede final frame/structure/storage checks.
After all external observations, the machine compares every captured argument's
layout, view and complete bytes. It then takes the alias-table lock, rechecks
the reservation and planned token/retirement and commits caller bytes plus the
preallocated slot. No callback, allocation, fallible storage lookup or provider
operation occurs between that final check and commit. Concurrent stale plans
fail before caller bytes change.

Unusable results, postdispatch refusal, late storage/profile change, uncertainty
and dropped pending calls fence all aliases in the shared scope. Pending-call
leases arm only after a machine effect is allocated; undispatched refusal and
unused-frame drop do not fence the root. These guards have no provider cleanup,
durable decision, terminal callback, retry or fabricated execution evidence.
Known reviewed failed replies preserve undefined Hconn bytes and release only
their unused reservation. Alias and caller-byte changes require usable known
writeback, not merely provider success.

## Compatibility and source boundary

The default frame port returns no scope, preserving older per-machine
connection-only embeddings. Future native point/property forwarding must require
the deliberate shared allocation. The initial finite ABI supports ASCII-compatible
CONN/CONNX and normal big-endian signed four-byte MQLONG fields, rejecting native
binary declarations and misaligned actual binary views before dispatch.

This scope has no Serde, executable checkpoint, snapshot or reconstruction path.
A fresh cold scope contains no mappings, even if integers or context look equal.
Numeric uniqueness is within an allocation, not a claim of global uniqueness
across roots. Cold/root provenance and live-token validation remain independent
host/provider obligations. Old DTOs, canonical domains, storage versions and
source/status projections are unchanged.

Offline hash-verified baseline `ibm-mq-9.4-mqi-2026-08-31` supplies CONN row0008
q101760 (SHA fa0cdd2c5e19326dfb91e5ad0b921fd47a1a3a918682e13c4ff5e36c2ba40347,
lines113–160), CONNX row0007 q101770 (SHA
41e9da41eb766141814ba1b2c3dc9c649450d1ab6cc64d574165f239ea8ed633,
lines1–55), and DISC row0012 q101800 (SHA
8e33bfec37f7fb467b9f206e8d2f03dc84a18bebf230068582dd84b7d4375e36,
lines19–28 and91–105). The source task scope excludes subtasks; SAME TASK sharing
is an owned host-lifecycle choice, never inferred from arbitrary CALL nesting.
Source review is zero execution credit; no browser/network refresh was performed.

Installed root ownership/terminal publication, native OPEN/PUT/GET/properties,
HOBJ/HMSG alias families, source-port forwarding and genuine compiled provider
verticals remain integration work. Private fixture replies test the adapter and
must not be reported as installed-host or licensed acceptance. All applicable
nonlicensed full mq.programming gates remain required; only the unavailable licensed
oracle is human-skipped0/26.
