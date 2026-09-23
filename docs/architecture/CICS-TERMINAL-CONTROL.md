# CICS terminal control in the v0.9 typed route

Status: **Implemented incrementally**
Owner: **CICS provider maintainers**
Scope: **typed terminal-control routes and durable terminal state**
Applies from: **mainframe-env 0.9.0 development**

The typed `SEND PARTNSET` route selects a registered 8775 partition set for
the current task, or resets the task to the base partition when its operand is
omitted. `CicsService::register_partition_sets` admits immutable, bounded,
nonoverlapping partition geometry. Selection checks that the definitions fit
the current terminal. The selected name is durable in the provider store and
is removed at task end. Retried mutations use an atomic state and receipt
write; a new request with the same idempotency key and different content is
rejected. The FACILITY resource `CICS.TERMINAL.PARTNSET.<name>` is checked
before changing selection. A base reset uses the suffix `BASE`.

`SEND PARTNSET` uses the typed COBOL path and MCEP v2 operation tag 90 and
operand tag 192. No existing MCEP v1 tag changed. RESP, RESP2, NOHANDLE,
EIBFN, and conditions flow through the common typed CICS response path.

The selected IBM source is CICS TS 6.x baseline
`ibm-cics-ts-6x-application-api-sources-c-2026-09-10`, catalog row
`ibm-cics-ts-6x-2026-08-31:api-commands:0191`, topic
`SSJL4D_6.x/reference-applications/commands-api/dfhp4_sendpartnset.html`,
SHA-256
`6f2af661ff00fa3559fee90e704c927966d7e03e655f94536bba116aa1142256`.
The committed topic SHA was checked against
`/Users/tore/Library/Caches/mainframe-env/ibm-docs-archive/raw/html/sha256/6f/6f2af661ff00fa3559fee90e704c927966d7e03e655f94536bba116aa1142256.html`;
the matching HTML was parsed locally with `ibm_docs.py` `PlainText`.
The pinned source says an immediately following RECEIVE is invalid until a
SEND MAP, SEND TEXT, or SEND CONTROL intervenes. This route records the
selection and the RECEIVE paths enforce that sequencing. The local
definition API bounds names to eight alphanumeric characters and geometry
to the configured terminal size; those bounds are implementation contracts.

During offline review, `ibm_docs.py read` succeeded for the exact topic.
`search` returned the requested row but reported other uncached topics in its
catalog scope. Those topics were not candidates for this command. No network
or browser refresh was used.

## RECEIVE PARTN

`CicsService::submit_partition_input` accepts an AID, partition name, raw
data, and cursor position from an authenticated terminal session. The active
partition set must contain that name. `RECEIVE PARTN` is rejected until a SEND
MAP or SEND TEXT has followed `SEND PARTNSET`; submitted input alone does not
satisfy that sequencing rule. The receive route consumes one pending input,
returns its partition name, actual LENGTH, AID and cursor position, applies
first-receive uppercase translation and later ASIS, and truncates INTO with
LENGERR 22 while reporting the original length. SET returns a 12-byte prefix
followed by data through the interpreter's pointer output contract. Session,
partition receive state, and replay receipt update atomically in the provider
store after FACILITY authorization. The MCEP v2 tags are operation 86, option
ASIS 124, and output PARTN 248.

Source: IBM CICS TS 6.x baseline
`ibm-cics-ts-6x-application-api-sources-b-2026-09-10`, catalog row
`ibm-cics-ts-6x-2026-08-31:api-commands:0164`, topic
`SSJL4D_6.x/reference-applications/commands-api/dfhp4_receivepartn.html`,
raw HTML SHA-256
`55f42adddc8b4aba65cd474531ffaca3a51d76716927a5432f9d49505d4a30f3`.
The committed hash matched its exact raw archive file, and the repository's
`ibm_docs.py` `PlainText` parser read the local HTML. No refresh was used.

## SEND CONTROL, SEND PAGE, and PURGE MESSAGE

`SEND CONTROL` applies ERASE, ERASEAUP, FRSET, FREEKB, ALARM, cursor,
partition activation, printer width, print, and form feed controls to the
durable terminal state. `ACCUM` stages controls in a bounded logical message;
the terminal screen changes when `SEND PAGE` completes it. REQID must remain
consistent across accumulated sends. A PAGING message queues its completed
page in durable temporary output; NOAUTOPAGE keeps the displayed screen.
`SEND PAGE` validates trailer framing, completes the message, and handles
RETAIN, RELEASE, and SET. The local SET contract returns a 12-byte prefix and
the bounded page image with RETPAGE 32. `PURGE MESSAGE` discards the pending
logical message without erasing the displayed screen. All three use FACILITY
authorization before durable state mutation and atomic replay receipts.
Unknown local LDCs return INVLDC 41; 3650-only FMHPARM returns INVREQ 16 for
the local 3270/8775 terminal model. `CicsService::terminal_control_snapshot`
exposes the resulting cursor, keyboard, partition, printer, page, and paging
state. The new MCEP v2 operation tags are 88 and 89. Device-control flag tags
occupy 125–145; the seven value operands occupy 193–199.

The selected IBM sources are CICS TS 6.x baseline
`ibm-cics-ts-6x-application-api-sources-c-2026-09-10`:

| Catalog row | Topic | Raw HTML SHA-256 |
| --- | --- | --- |
| `ibm-cics-ts-6x-2026-08-31:api-commands:0188` | `SSJL4D_6.x/reference-applications/commands-api/dfhp4_sendcontrol.html` | `4de8c65be1c657057a00898a83816bfb61cdc5bc8113e1b4def1dcc057d3345a` |
| `ibm-cics-ts-6x-2026-08-31:api-commands:0190` | `SSJL4D_6.x/reference-applications/commands-api/dfhp4_sendpage.html` | `ea6908816e60520e9de1ced1cd312b4e99745dc4a2592a06468f1b7166a43254` |

The affected existing PURGE MESSAGE source is baseline
`ibm-cics-ts-6x-application-api-sources-b-2026-09-10`, catalog row
`ibm-cics-ts-6x-2026-08-31:api-commands:0148`, topic
`SSJL4D_6.x/reference-applications/commands-api/dfhp4_purgemessage.html`, raw
HTML SHA-256
`5e742d2abc92990142252e6bb6ac5f037252e06f14b769fac0d0aa24c24ba1fa`.
Each committed SHA matched its exact raw archive file and the repository
`PlainText` parser read that local HTML. No browser or network refresh occurred.
