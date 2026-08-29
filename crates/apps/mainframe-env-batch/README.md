# mainframe-env-batch

The single 0.1 JCL/JES authority. Bounded JCL is expanded into a typed workflow;
JES owns durable job, step, queue, and spool state; and every executable step is
dispatched through the typed `host.program.invoke` service. The package contains
only the explicitly accepted utility selectors and has no generic-success path.

JES preserves ordered DD inputs for compiler-backed programs. The composed
server translates `SYSIN` and `SYSLIB*` DDs into the same generic source-library
contract used by CLI and future application installation.
