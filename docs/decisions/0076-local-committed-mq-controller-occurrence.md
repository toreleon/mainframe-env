# ADR 0076: Explicit local committed MQ controller occurrence

Status: Proposed
Owner: Mainframe Sandbox runtime
Scope: Local MQ committed PUT1 occurrence and selected Batch/JES controller
Applies from: mainframe-env local composition with this opt-in boundary

This boundary is an explicit local API. It is not an IBM automatic triggering
implementation, an implicit HCONN 0 mapping, or an OD4/options admission extension.
The unavailable pinned MQ 9.4 MQI reference bodies still block those vendor claims.

An authenticated signed local process/trigger graph may additionally sign the
controller launch marker and each complete bounded raw JCL input. The real JCL
parser must produce exactly one always-run IKJEFT01 step selecting the installed
ProgramCall controller. Every signed byte is compared and charged before use.
The original legacy queue-only APIs, codecs and NotFound publication guards stay
unchanged. Genuine Complete/Applied publication remains the separate ADR 0075
participant; this component never writes that publication row.

Only a same-physical-store terminal canonical CoreEffect for a successful,
transaction-free local PUT1 can prepare an occurrence. The original MQ owner
must retain the selected graph, exact attributed replay and unique complete
committed queue message. Pending units, CICS nested effects, unknown or legacy
replay, aliases and native selected stores are refused. Exact original manifest,
queue and replay payloads advance only their versions under the held owner and
the original atomic CAS API. No MQ commit/backout or reconnect result changes.

The atomic occurrence begins as LaunchUnknown, alongside its exact controller
submission binding. Only the real same-store commit acknowledgement releases
that preparation. The original Batch submit, durable JES work admission and
worker path dispatch the compiled installed program. The original Complete/
Applied guard and full generation/artifact/JCL binding also remain checked at
the actual TSO branch. Occupied occurrences never dispatch again; failed or lost
submission acknowledgement retains LaunchUnknown with no automatic recovery.

Retirement borrows the actual selected controller/publication and actual Batch
job mutex. It requires the full real job row, Completed/RC0/one completed step
with ReturnCode 0, the original matching checkpoint, and the real terminal JES
work record. It advances only the occurrence row. A Program return, copied job
snapshot, equal filename or copied root claim supplies none of this authority.

The finite API uses the existing serial trusted ProductServer/store embedding
contract. Raw foreign writers and hostile cross-process atomic custody are not
proved. It does not add a queue consumer, background trigger scan, native MQ
workflow, unknown-outcome settlement, or runtime acceptance credit. Signed graph
resources stay within 256 and original MQ/store/JES/composition limits stay in
force; the launch JCL is bounded at 64 KiB and no global budget is increased.

Positive compiled launch needs the real ADR 0075 publication participant and
the existing JES worker lifecycle. Until that genuine owner is composed and
executed, this draft and its controls remain uncompiled/unrun with zero credit.
