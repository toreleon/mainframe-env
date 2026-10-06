# Subsystem-owned host ABI source libraries

Status: **Frozen for mainframe-env coverage.foundation**
Owner: **host-contract and provider maintainers**
Scope: **subsystem-owned host ABI source libraries**
Applies from: **mainframe-env current subsystem contracts**

The COBOL compiler owns language processing only. It receives exact source
bytes and ordered `SourceLibrary` values through `SourceBundle`; it contains no
DFHAID, DFHBMSCA, SQLCA, or MQ compatibility source and has no implicit
fallback catalog.

CICS owns DFHAID and DFHBMSCA, Db2 owns SQLCA, and MQ owns CMQGMOV, CMQMDV,
CMQODV, CMQPMOV, CMQTML, and CMQV. Each provider exposes one immutable
`mainframe-env.host-abi-source-library@1` definition with subsystem, version,
library name, behavior, exact source bytes, SHA-256-derived identity, license,
and provenance. The checked-in `.cpy` files are repository-authored behavioral
compatibility definitions under Apache-2.0; they do not reproduce or claim to
be vendor source text.

The foundation source package validates all definitions and bounds before it
materializes any files or libraries. Server, CLI, and conformance composition
explicitly order CICS, Db2, then MQ after workload-supplied libraries. Missing,
duplicate, unlicensed, placeholder, oversized, or conflicting input fails
before an immutable source bundle exists. The application-package ABI section
continues to reference content-addressed blobs and can select a retained prior
generation for rollback; the compiler never selects a subsystem generation.

The inventory records nine reached compatibility members and zero compiler
members. Cataloging or relocating these bytes grants zero official semantic
coverage credit.
