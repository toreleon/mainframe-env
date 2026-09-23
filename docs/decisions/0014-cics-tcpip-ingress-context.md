# ADR-0014: Keep CICS TCP/IP and certificate facts in trusted ingress context

Status: **Proposed for v0.9 development; executable routes pending**
Owner: **CICS and network maintainers**
Scope: **EXTRACT TCPIP and EXTRACT CERTIFICATE task context**
Applies from: **mainframe-env 0.9.0 development**

## Context

The pinned CICS TS 6.x application topics for baseline
`ibm-cics-ts-6x-2026-08-31` are row `0074` `EXTRACT TCPIP`,
`dfhp4_extracttcpip.html` at
`sha256:6a7441511637549ec5246ca013f0b62249047666995e4a2ff570acf04cf389fc`,
and row `0070` `EXTRACT CERTIFICATE`, `dfhp4_extractcertificate.html` at
`sha256:5222642cdc1028d3601dd9f905476ee28c6e435d0e15792f421c6e9ada503bb6`.
Both exact retained raw HTML files were hash verified and parsed with the
repository PlainText parser. The shared argument-value topic at
`sha256:44e85f97788be382d70df61dd7059ba079f26a0da4dff8e40e31751cf3c68e70`
and CVDA overview at
`sha256:81f101e030365400b431ecf68250dfcabc5673e1acbf05010c9285bf590e3b25`
were reviewed the same way. The overview links a numeric CVDA table that is
not in the committed manifest; publication bodies remain outside Git.

The existing online CICS task routes have no admitted socket or TLS handshake
context. Reading process-wide network state would attribute the wrong peer to
a task and would not survive replay or restart. An application-supplied binding
would allow a program to forge client certificate identity.

## Decision

1. A trusted host ingress may immutably bind one `cics-tcpip-context-v1` record
   only after the matching CICS run is admitted. Repeating the same value is
   idempotent; a different value conflicts.
2. The record contains canonical peer and local addresses, DNS names,
   TCPIPSERVICE, port, symbolic source-named authentication/privacy/TLS modes,
   maximum HTTP data length, and optional certificate DER plus fields parsed
   by the trusted TLS ingress. A certificate can be bound only to completed
   client-certificate authentication. The provider bounds the DER envelope
   and all fields; TLS ingress remains responsible for full X.509 validation.
3. The CICS provider validates canonical durable rows on open, rechecks run,
   execution, and principal ownership on access, and deletes the context at
   task release. A task without such a record is a non-TCP/IP application for
   the future EXTRACT commands.
4. The context stores CVDA names, not invented numeric values. The EXTRACT
   commands remain unready until a reviewed numeric mapping, output lifetime,
   condition behavior, and selected compiled routes are implemented.

## Consequences

Network identity is an accepted connection fact owned by the host, never a
value supplied by the COBOL command. Immutable, versioned records preserve
the same facts through SQLite reopen and replay. This decision introduces no
network fetch, browser refresh, licensed oracle credit, or executable-command
readiness by itself.

## Verification

Focused memory/SQLite registration, idempotency, conflict, malformed-envelope,
and corrupt-row reopen tests cover this authority. Module, public API docs,
typed-semantic, documentation, formatting, dependency, and diff gates apply.
