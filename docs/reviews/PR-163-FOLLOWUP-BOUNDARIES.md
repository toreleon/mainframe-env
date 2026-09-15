# PR #163 follow-up boundaries

This note records the corrective coverage boundary for review findings #196, #197, and #198.

## PROGRAM operands (#196)

The typed LINK/XCTL PROGRAM resolver now distinguishes literal resource names from data-area operands. Literal names remain 1-8 characters and accept the CICS resource-name set `A-Z`, `0-9`, `$`, `@`, and `#`. A data-area PROGRAM operand must be exactly eight bytes and alphabetic/alphanumeric. Provider admission uses the same resource-name character set.

This closes the validator mismatch without broadening deferred LINK/XCTL options.

## RETRIEVE (#197)

RETRIEVE is not promoted to typed/runtime equivalence by this follow-up. The current compatibility binding `cics.retrieve` is continuation COMMAREA state, not an expired START record. Until the runtime owns a durable scheduled START-record queue and can atomically consume an eligible record with replay state, RETRIEVE remains a legacy compatibility route and receives no typed or whole-command coverage credit.

Required promotion gate:

- separate START payload authority from terminal continuation COMMAREA;
- durable identity, due/expiry state, ownership, and restart semantics;
- atomic RETRIEVE consume + replay/idempotency update;
- tests for no record, one record, repeated RETRIEVE, crash/retry, concurrent consumption, and coexistence with pseudo-conversation state.

## PURGE MESSAGE (#198)

The typed PURGE MESSAGE route remains intentionally partial. The current runtime can exercise only the empty full-BMS logical-message state. This row must not be interpreted as whole-command behavioral equivalence until durable BMS accumulation/page state makes non-empty messages reachable and the source-defined TSIOERR/error behavior is implemented and tested.

Required whole-command promotion gate:

- durable terminal-owned accumulated logical-message/page state;
- SEND/ACCUM transitions that produce non-empty state;
- PURGE disposal for one and multiple pages;
- restart/replay coverage and TSIOERR/error cases;
- coverage accounting that distinguishes typed-route existence from whole-command equivalence.

These boundaries are correctness requirements, not roadmap ordering commitments.
