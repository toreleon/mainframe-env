# CIC-906 representative SAF, audit, and uncertainty validation

`CIC-906.saf-audit-uncertainty-representatives` is a validation-only slice
from `9027d142` (191 typed, 0 legacy, 72 unready application rows). It changes
no command registry, schema, provider behavior, or IBM semantics. The selected
already-executable mutations are `0034 DEFINE COUNTER`, `0140 LINK ACQPROCESS`,
and `0187 SEND` for a mapped APPC conversation. The owner is the existing
`mainframe-env-cics` service tests and their Memory, SQLite, and PostgreSQL
state authorities. This receipt grants only the cells below; it is neither a
191-route campaign nor exact-final-candidate evidence.

| Matrix cell | `0034` counter | `0140` BTS LINK | `0187` mapped APPC SEND |
| --- | --- | --- | --- |
| SAF deny before mutation and typed audit | Memory: `named_counter_saf_audit_precedes_create_and_denial` checks `COUNTER`/Update, absent counter state, and a `host.security.authorize` Deny audit for the issuing run. | Memory: `bts_link_saf_denial_leaves_shared_process_initial_and_audits` checks `BTSPROCESS`/Execute, initial activity with no replay, and the typed Deny audit. The child fetch test checks the analogous `BTSCHILD` path. | Memory: `cic906_appc_send_saf_denial_is_typed_and_precedes_staging` checks the `CONNECTION` denial, unchanged ledger, absent SEND replay, and the typed Deny audit. |
| Audit capacity/failure before mutation | Memory: `cic906_audit_saturation_blocks_counter_bts_link_and_appc_send` sets the real store audit quota to zero; the transaction SAF audit fails with `ResourceExhausted` before a counter row exists. | Same selector; the process stays Initial with no replay. | Same selector; no frame, conversation replay, or outer effect receipt is written. This is a pre-dispatch atomicity check. |
| Post-dispatch uncertainty and owner-fenced reconciliation | Memory: `named_counter_reconciles_unknown_outcome_after_outer_receipt_failure` fails the outer replay insert after one counter mutation, returns `UnknownOutcome`, rejects a different execution owner, and restores the original reply with one counter row. | SQLite and PostgreSQL: `bts_selected_link_reconciles_outer_receipt_after_*_reopen` inject response loss after the outer receipt, reject a different owner after reopen, and replay without a second selected program LINK. PostgreSQL also has `bts_selected_link_reconciles_after_postgres_process_exit` with a new process. The separate `bts_link_post_dispatch_failure_is_fenced_without_redispatch` selector keeps an unresolved program failure unknown. | Memory and SQLite: `cic906_appc_send_receipt_gap_reconciles_on_*` fails the outer replay insert after CONNECT and data frame acknowledgements, returns `UnknownOutcome`, rejects a different owner, and completes the same owner without another transmission. |
| Live cancellation/deadline | No additional counter-specific live clock selector in this slice. | Existing `bts_child_and_link_stop_on_live_cancellation_and_deadline` checks both before LINK dispatch. | `cic906_appc_send_live_cancellation_and_deadline_leave_no_frame` checks a live cancellation probe and finite clock expiry with unchanged ledger and no replay. |
| Restart/backend | Existing counter CAS/reopen selector on SQLite and concurrent CAS selector on PostgreSQL; the outer receipt failure case above is Memory only. | Existing SQLite reopen plus task-owned PostgreSQL 18.6 adapter reopen and separate-process restart selectors. | Existing `conversation_send_wait_sqlite_restarts_without_redispatch` covers staged SEND after SQLite reopen; the new outer receipt failure uses Memory and SQLite in-process recovery. PostgreSQL conversation allocation/reopen is a ledger control, not SEND receipt-gap credit. |

The audit quota selector deliberately stops at the first transaction security
audit. It does not prove a post-dispatch audit-store failure, a multi-record
audit/effect/lifecycle transaction, or every family-specific resource audit
under saturation. The receipt-gap selectors exercise the service-owned replay
and one selected effect each; they do not prove arbitrary program, network,
file, queue, or UOW reconciliation. The remaining 188 typed application rows,
72 unready rows, other options of these three rows, cross-family concurrent
workflows, syncpoint/retention/scale combinations, PostgreSQL SEND receipt-gap,
and licensed CICS differentials remain outside this slice. The CIC-906 parent
and full 0.9 completion gate remain open.

## Source and evidence boundary

The source identities remain the frozen CICS TS 6.x catalog, baseline
`ibm-cics-ts-6x-2026-08-31:api-commands`:

| Catalog row | Review baseline | Pinned topic under `SSJL4D_6.x/reference-applications/` | Manifest SHA-256 |
| --- | --- | --- | --- |
| `0034` | `ibm-cics-ts-6x-application-api-sources-a-2026-09-10` | `commands-api/dfhp4_definecounter.html` | `a0851a951c4efd1ab06d9e90c42e616c937628af0b3df21119be25c4db65cd05` |
| `0140` | `ibm-cics-ts-6x-application-api-sources-b-2026-09-10` | `commands-bts/dfhp4_linkacqprocess.html` | `3b85be529e49d33d514057795bc6de5f48100e2cdde9fb9e434ccddd54dd66f7` |
| `0187` | `ibm-cics-ts-6x-application-api-sources-c-2026-09-10` | `commands-api/dfhp4_sendappc.html` | `b2c2a082b826f1419838b53ae1dcfee56d8642004d900298183d748cd4be0686` |

For this checkout, offline `ibm_docs.py search` found no verified topic and
`read` could not verify the CICS TOC. The three committed `topic_path` locations
were resolved against the bounded retained HTML root and all three files were
absent. No network or browser refresh occurred. Earlier status-ledger source
reviews are historical references; this validation-only edit does not upgrade
their source or licensed credit. `architecture-fast` remains offline-source
blocked where that gate requires a verified corpus.

Focused local checks and their backend identities are recorded in the handoff;
this file is a scope receipt, not a replacement for current-candidate CI or
licensed differential receipts.
