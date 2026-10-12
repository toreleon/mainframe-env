# 0067. Borrowed durable first-dispatch Intent

Status: proposed; infrastructure prerequisite, not protected native-root or MQ acceptance.
Owner: interpreter coordinator and privileged Rust dispatch embedding maintainers
Scope: opt-in wrapping of the actual first keyed durable HostCall dispatch
Applies from: mainframe-env optional execute_resumable_with_dispatch_control entry

The original coordinator retains lifecycle, before-dispatch Intent, execution controls, scoped host invocation and result/audit persistence. Copied EffectRecords or matching IDs cannot mint its borrowed loan. The opt-in entry creates the nonconstructible proof from the actual JournalCursor/store, exact actor/effect, retained Intent and control after original persistence and pre-dispatch control checks.

The proof checks exact Running execution and canonical Intent identity, including actor, sequence/key, request digest, capability, resource, audit key and recovery state. It is not a storage lock or protected RootDriverClaim, and asserts no exclusivity against independent writers. Compilation, CICS task, installed frame, MQ provider/UOW and terminal disposition must be independently admitted by their actual owners.

The hook cannot return a fabricated host result: the coordinator retains its own original ScopedHostService call and audited result. At most one physical dispatch occurs. An omitted dispatch keeps the original unresolved Intent; repeated callback attempts never invoke a second host call. Failed/panicked wrapping after dispatch or a changed current execution/Intent conservatively produces UnknownOutcome with the actual audit and no resolution age or automatic retry.

Legacy entries pass the private default hook option. Native-root/child local profiles and RootDriverClaim remain unchanged. Existing durable replay identity/result checks execute without reminting this first-dispatch proof. Grant, generation, clock, cancellation, deadline, quantum, canonical-byte, lifecycle and store quotas are unchanged.

A privileged CICS embedding can borrow its actual task inside the per-effect callback while this Intent loan exists. It cannot hold the task across an entire machine drive. Initial compiled frame binding and protected CICS MQ root remain local implementation prerequisites; copied terminal/LINK observations are insufficient.

Three unrun source controls cover actual two-occurrence result/audit identity, changed actor, omitted/repeated/failed/panicked wrapping and refusal to repeat retained uncertainty. There are no new MQ return codes, COBOL argument semantics, implicit settlement, anonymous fallback, application tokens or licensed execution evidence.
