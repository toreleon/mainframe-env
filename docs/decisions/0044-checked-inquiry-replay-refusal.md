# ADR 0044: Checked inquiry replay refusal settlement

Status: Implemented infrastructure prerequisite; configured inquiry activation remains unsupported.
Owner: host transport, interpreter original dispatch and physical store maintainers.
Scope: replay-only transport, bounded capture and atomic Completed refusal settlement.
Applies from: mainframe-env mq.programming

## Decision

Keep ordinary and reconciled replay unchanged. A deliberate coordinator protocol
preference may select replay-only transport for the checked integer local-type
inquiry wrapper, including zero selectors. This neither proves normal-local
origin/INQUIRE access nor activates a provider. `HostProvider::replay_retained`
defaults Unsupported without ordinary invoke fallback. Scoped host admission,
canonical budgets, generation/panic checks and actual audit classification remain
the sole host boundary. A changed successful digest becomes actual Unknown before
audit creation rather than disappearing at the old digest-before-audit boundary.

The publicly nameable `CheckedReplayAuditCapture` has private construction at
the actual OriginalDispatch. It binds the original Completed record, complete
current Running execution, invocation, logical observation and same physical
PlatformStore Arc. A synchronous receiver may submit only bounded structural
observations once; it cannot construct Running, choose an audit or mint permission.
There is no Serde factory, global cache/TLS, provider dispatch or journal write
inside the mailbox. Coordinator closure attaches ScopedHost's actual audit and
preserves observations for uncertainty. Submission plus transaction-owned copy
is conservatively bounded to 64MiB before cloning, including the complete captured
invocation. Public invocation collections and strings are rechecked against the
default construction limits; their aggregate capture is additionally limited to
16MiB. Binding contents and generation text remain observations, not permissions.

Success uses the existing nonpublishing physical assertion, exact prior receipt
and original result digest. No receipt/effect/event/audit/clock/counter mutation
or output/source recomputation is performed by this replay step. Known refusal
uses `JournalStore::commit_checked_replay_refusal`: exact full Completed/current
Running/lease/floor/root@1/current-read comparisons and one real scoped refusal
audit with the actual cursor's next existing event/outbox share one physical
lock/writer transaction. Old core/result/provider receipt bytes remain exact.
It cannot represent provider mutations, checkpoint or terminal transition.

Memory extracts the unchanged existing touched-entry journal kernel. SQLite
reuses existing durable codecs and sole row mutation/transaction finish kernel,
with bounded length-before-fetch core/root/provider reads and a writer-locked
contiguous lifecycle keyset check excluding phantoms. No backend lock spans
host/SAF/frame/control callbacks. Non-Success audit metadata must match the real
original capability/resource/invocation/principal/attempt. Root@1 conservatively
refuses row/namespace overlap even under the same root. Legacy unindexed actors
cannot borrow indexed scopes. PostgreSQL and other unimplemented adapters refuse.

The combined 4096-operation budget reserves ten operations for execution/event/
outbox/audit and physical epoch/logical accounting; at most 4086 dependencies
remain. Captured/current/encoded bytes fit 64MiB and narrower configured max-blob
bounds. Known conflicts, quota/encoding/epoch/audit/outbox faults roll back the
whole settlement. Cursor progresses only after known physical success.

Failed settlement, missing/foreign capture or ambiguous acknowledgement retains
the actual closed attempt in its owning coordinator and returns protected Unknown.
It cannot be overwritten by another opt-in replay. No clearing/retry/adoption API
is supplied. A future genuine supervised failure consumer must reconcile current
root/control/cursor and possibly committed audit before consuming it. Scope loss
never authorizes an old terminal-root audit or known unaudited denial. That consumer
and genuine configured inquiry origin are prerequisites for activation, not fake
implemented supervision. Existing legacy failure policy is unchanged.

No canonical/receipt/core/root/index/schema migration, new audit kind/table/journal,
query engine, source numeric projection or provider semantic activation occurs.
Private physical/dispatch fixture tests grant no installed/native/JES/SAF deployment,
crash/recovery/participant/IR/full26 credit. All wider inquiry forms and six-gate
full-call acceptance remain pending; only licensed oracle is human-skipped0/26.
