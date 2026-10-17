# 0066. Borrowed current CICS program task custody

Status: proposed; infrastructure prerequisite, not MQ application or subsystem semantic acceptance.
Owner: CICS runtime and privileged compiled-program embedding maintainers
Scope: explicit per-effect borrowed custody of an already registered and synchronized current root Run
Applies from: mainframe-env optional trusted Rust dispatch embedding; ordinary CICS command paths remain unchanged

Terminal execution and LINK entry snapshots retain observations, not the live task. A copied invocation, selector, generation, session or artifact cannot replace custody of the actual Run. The explicit nonconstructible current-program loan uses the original CommandLease to remove and own that Run and retain its same-thread exclusive command claim throughout one non-CICS external dispatch callback.

Only the exact synchronized logical-level-one root with explicit local context and exact session is supported. The API performs no registration, synchronization, copied restoration, enrollment or root relabeling. Changed actor, nested/outer origin, parent, session, expiry, cancellation, ABEND, unavailable task or active claim refuses. HRTB callback borrowing prevents escape; the loan has no public constructor, Clone, Debug, serialization, Send or Sync.

Task custody is distinct from compiled/catalog, coordinator Intent, protected RootDriverClaim, provider/frame and MQ authority. A clock offer is not a clock producer: the embedding supplies actual coordinator control. This trusted synchronous boundary does not assert atomic control observation against arbitrary external writers.

Unknown callback outcome marks the session uncertain while the real command holds the Run, before restoration releases its claim. Failed restoration is fenced while the command remains owned, before original Drop can retry. Panic fences before command Drop restores. Known callback results disarm only after actual restoration succeeds. No native queue action, commit, backout, disconnect, recovery permission or COBOL completion follows from a loan exit.

Hold the loan for one non-CICS external effect, never the whole machine drive. Ordinary CICS commands correctly refuse while Run is removed; no reentrancy bypass is introduced. A first compiled frame must be genuinely bound before its first MQ operation. A terminal clone cannot synthesize that binding.

Three unrun source controls cover actual held Run/claim, competing entry/task cleanup, changed actor/scope, unknown/panic and the real pre-restoration fence boundary. Protected CICS MQ root admission, CODATE01 workflow, implicit HCONN, native MQOD/UOW and licensed result comparison remain separate work. Missing IBM MQ 9.4 bodies or licensed captures remain separate evidence limitations.
