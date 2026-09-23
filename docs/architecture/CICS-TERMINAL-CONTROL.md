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
selection and leaves that sequencing to the relevant receive path. The local
definition API bounds names to eight alphanumeric characters and geometry
to the configured terminal size; those bounds are implementation contracts.

During offline review, `ibm_docs.py read` succeeded for the exact topic.
`search` returned the requested row but reported other uncached topics in its
catalog scope. Those topics were not candidates for this command. No network
or browser refresh was used.
