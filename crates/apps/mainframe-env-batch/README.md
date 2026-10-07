# mainframe-env-batch

The owned JCL/JES authority. Bounded JCL is expanded into a typed workflow;
JES owns durable job, step, queue, and spool state; and every executable step is
dispatched through the typed `host.program.invoke` service. The package contains
only the explicitly accepted utility selectors and has no generic-success path.

JES preserves ordered DD inputs for compiler-backed programs. The composed
server translates `SYSIN` and `SYSLIB*` DDs into the same generic source-library
contract used by CLI and future application installation.

Application TSO and IMS behavior is installed through complete, package-bound
`mainframe-env.batch-controller-registry@1` generations. The runtime parses an
exact typed selector and executes a generic plan; it contains no CardDemo
program, database, segment, or DD identity. Generation validation and selector
conflict checks finish before atomic publication, and retained identity-equal
generations support application-package rollback without partial selection.

Common utilities, nested Db2 TSO programs, and COBOL system services are
selected from the generated `mainframe-env.common-program-catalog@1` registry.
JES and the composed server dispatch typed enums only; unsupported dispositions
remain catalog data and application programs fall through to `ProgramService`.

The 0.7 converter uses the shared exact-byte source closure and diagnostic
contracts. Its lossless Rowan-style JCL tree owns fixed columns, continuations,
in-stream data, and CNTL data. One generated catalog supplies all 20 JCL, 13
JES2 JECL, and 204 parameter identities to parsing, validation, planning,
documentation, and conformance closure checks.

INCLUDE and procedure traversal share one bounded dependency/cycle utility.
JCLLIB order, SET/EXPORT timing, procedure defaults and invocation overrides,
DD/EXEC overrides, nested invocation chains, and backward DD references retain
their exact definition, use, invocation, and override source ranges. Expansion
only produces converter input; it performs no scheduling, allocation mutation,
utility execution, or JES success simulation.

## Documentation

[Documentation portal](../../../docs/README.md) · [Current package map](../../../docs/architecture/PACKAGE-MAP.md) · [Embedding guide](../../../docs/guides/EMBEDDING.md) · [Contribution and verification](../../../CONTRIBUTING.md)
