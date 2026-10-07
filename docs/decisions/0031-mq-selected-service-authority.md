# One selected MQ service authority

Status: **Proposed**
Owner: **MQ provider and execution/store maintainers**
Scope: **private service composition, not public MQI acceptance**
Applies from: **mainframe-env current subsystem contracts**

## Decision

The existing `MqService` holds one mutex-protected discriminated authority:
legacy `DurableState` or rich catalog/delivery/rows/marker/replay. Strict private
selection uses the existing single-snapshot reader. It never creates missing
state, silently imports, changes recovery fences or applies cold backout. No
legacy queue map lives beside a selected rich delivery kernel.

The selected opener accepts one `Arc<dyn PlatformStore>` and derives its provider
store view by trait upcasting that same allocation. It cannot pair separate core
and provider backends. Authorization and the trusted replay clock are mandatory
constructor inputs, not optional testing permissions. Selection itself grants
no resource authorization, host attestation, coordinator permission or readiness.
The exact trusted generation/fence remains a deployment/recovery input, never
application envelope data. The existing bounded reader and codecs remain the
only durable-state interpretation authorities.

Historical public constructors keep their existing migration and policy paths,
wrapping only a legacy authority. Their checked legacy guard keeps the sole mutex
held while existing algorithms borrow that state. A selected instance cannot
obtain this guard even while its stored variant is legacy: old install/execute/
replay/persistence paths are sequential and must not bypass selected publication.
The old `mq_providers` registration produces no providers for a selected instance.
It retains existing registrations for historical instances. A separate typed MQI
registration requires its actual participant, dispatch and recovery proofs.

## Compatibility and proof boundary

No persisted schema, canonical domain, source pin, dependency or public legacy
constructor signature changes. Legacy import/reader rejection of rich markers
remains fail-closed; old already-open writers retain their manifest CAS fence.
Selected open is read-only, including refusal of missing, mixed, corrupt or
identity-mismatched state. The same physical SQLite store can close/reopen and
select its retained authority without volatile state reconstruction as durable
ownership. Opaque handles, host process leases and UOW owners are not minted by
this opener.

The pinned IBM MQ 9.4 baseline `ibm-mq-9.4-mqi-2026-08-31` and catalog rows
`0001`, `0007`, `0008`, `0009`, `0015`, `0020`, `0021` retain existing context,
handle, syncpoint and delivery semantics. Offline `MQCONNX` row `0009`, topic
`SSFKSJ_9.4.0/refdev/q101770_.html`, corroborates that opening stored authority
does not establish application connection/task/process handle ownership. This
composition introduces no IBM wire or reason constant and grants no official
or licensed coverage.

## Remaining acceptance

Actual selected operations must bind the original effect, real host lifecycle,
same-store core intent, separately persisted UOW ownership and exact typed SAF
resource decisions. They must compose row/replay/audit publication before state
adoption, preserve uncertainty and replay through the shared coordinator, and
advance the persisted registry fence before restarted exposure. Selection alone
does not accept MQ as a participant or execute any of the 26 MQI calls. Those
proofs, ABI registration, CardDemo and licensed differentials remain required.
