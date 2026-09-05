# Unknown-outcome propagation (#49)

The installed-call, installed-batch and inline COBOL adapters preserve the typed
`UnknownOutcome` category. `ExecutionProblem::has_unknown_outcome` recognizes
both the category and the legacy public flag. No caller must parse a message.

The interpreter treats an unknown host effect as in-doubt work, not a COBOL
CALL/ACCEPT exception, file declarative, or subsystem condition. Known provider
rejections remain handleable using ordinary language rules. A secondary cursor
write/delete failure or journal result/terminal failure cannot turn an already
uncertain business effect into a definite failure.

The regression provider commits actual provider state before returning unknown.
Tests compile and install nested programs, pass through the real host router,
interpreter and journal, and reopen SQLite to compare durable business data and
exact reconciliation records (execution, run unit, sequence, key, request and
result digests). Separate tests cover known rejection and failure of cursor,
effect-result, and terminal-event persistence.

A result-journal failure can leave only the previously durable `Intent` record.
The caller still receives `UnknownOutcome`; recovery must inspect that intent by
its effect key and obtain authoritative outcome evidence before retrying. This
change does not magically persist information when the store cannot accept it.
The existing `unknown_effects` listing contains explicitly recorded unknown
outcomes, not every abandoned intent. Do not interpret an empty listing as proof
that no in-doubt intent exists after a persistence failure.

Direct installed COBOL calls retain their existing non-journaled inner driver;
a durable outer batch/inline driver records uncertainty for its child call.
The tests do not claim general crash replay/deduplication equivalence. Stable
nested-call identity and crash-gap replay are the separate obligation in #55;
canonical persistent encoding is #57. No automatic retry or licensed IBM
conformance credit is introduced here.
