# mainframe-env-batch

The single 0.1 JCL/JES authority. Bounded JCL is expanded into a typed workflow;
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
