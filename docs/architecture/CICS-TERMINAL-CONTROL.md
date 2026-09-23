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
