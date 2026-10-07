# mainframe-env-host-api

Ownership: typed host and CICS requests/results, effect metadata, capability
descriptors, generated official semantic identities, and immutable capability
and subsystem-handler registry snapshots. Non-goals: concrete providers,
database rows, async runtimes, broad application state, or deriving semantic
coverage from generated identities. It depends only on the execution contract.

The IMS SSA contract uses reviewed, generated grammar metadata and a bounded
display-code parser. Encoding adapters translate source bytes before parsing;
generic DBD metadata supplies exact field lengths so comparative values remain
binary and cannot be split by connector-shaped data.
Literal `-` null command slots reserve syntax space without selecting an active
command. Null and active slots share the parser bound; raw request bytes still
bind canonical replay. The active-command inventory remains seventeen.

The optional version-1 `ImsGsamFormat` declares application-area F/V/U format,
access method, block bound and control selection in signed database metadata.
`ImsGsamRequest`/`ImsGsamResult` carry U's separate owned length; V retains its
two-byte LL and exact application bytes. This is an owned logical adapter.
Raw PCB and physical RDW interfaces require separate authorities. See
[ADR-0029](../../../docs/decisions/0030-gsam-application-record-formats.md).

The IMS PCB/status contract freezes DB, GSAM, I/O, and alternate mask layouts,
execution contexts, and field applicability. Its generated registry contains
the exact database, system-service, and message status memberships, including
two-byte blank success. Lookup rejects malformed or unknown codes and status/PCB
combinations outside their reviewed contexts. The TM contract validates its
four core statuses against the same registry. These descriptors do not execute
a DL/I call or grant coverage.

The shared `mainframe-env.ims-metadata@1` contract owns versioned DBD, PSB, PCB,
segment, field, index, relationship and sensitivity DTOs plus their bounded
cross-reference validator and domain-separated digest. Packages and providers
consume this one authority rather than translating between private schemas.

`ImsRecoveryRequest` and `ImsRecoveryResult` are additive owned host forms for
logical LOG, basic/symbolic CHKP and XRST in DB-batch CALL context. They use `host.ims.write`, carry
the selected application/package/PSB/database binding and canonical mutation,
and return bounded status, durable sequence, restored application areas and
attempted database-PCB GU statuses. Normal start and named checkpoint selectors
are distinct. Timestamp selection requires an authentic DFS0540I context
authority; LAST requires BMP. Both reject explicitly in this DB-batch route.
Unsupported contexts, command syntax and external transaction operands reject
explicitly. Raw LL/ZZ and AIB framing, other recovery calls and physical log
sizing are not admitted.
The explicit canonical encoder freezes the new named variants without changing
prior IMS request/result bytes. No provider type enters this contract.

Invariants: every request/result is bounded; mutations carry effect sequence
and idempotency identity; capability resolution is deterministic; official and
custom semantic namespaces cannot overlap; and no generated identity installs a
handler. Verify with `cargo test -p mainframe-env-host-api` and
`cargo xtask semantic-identities --check`.

## Documentation

[Documentation portal](../../../docs/README.md) · [Current package map](../../../docs/architecture/PACKAGE-MAP.md) · [Embedding guide](../../../docs/guides/EMBEDDING.md) · [Contribution and verification](../../../CONTRIBUTING.md)
