# mainframe-env-compiler

Ownership: bounded COBOL source decoding, lossless syntax, typed semantic/HIR,
Core-MIR lowering, legality, and the public compiler service. Non-goals: async
I/O, provider execution, native code generation, or persistence. Allowed
dependencies are owned compiler foundations/contracts plus private Rowan syntax
storage.

Invariants: recovery nodes never publish; every executable statement lowers to
a registered Core-MIR operation; source, layout, and provenance bounds are
enforced. Fixed-format preprocessing is column-aware, comments are never
directive authority, continuations retain exact decoded-source origins, and
COPY expansion records both included bytes and directive origins. Verify with
`cargo test -p mainframe-env-compiler`.

Host compatibility source is an explicit input, never compiler-owned data.
CICS, Db2, and MQ expose versioned source libraries that the server, CLI, or
conformance composer orders into `SourceBundle`; missing ABI members fail COPY
resolution. The ownership contract is documented in
[`docs/architecture/HOST-ABI-SOURCE-LIBRARIES.md`](../../../docs/architecture/HOST-ABI-SOURCE-LIBRARIES.md).

The semantic data model uses unique qualified identities while retaining simple
COBOL names for resolution. Recursive groups, sibling REDEFINES, levels
66/77/78/88, OCCURS ranges/dependencies/indexes, FILE/LINKAGE roots, subscripts,
reference modification, and reached display/binary/packed/edited bytes are
bounded explicitly. It also owns canonical USAGE/data-class identity,
NATIONAL/UTF-8/DBCS and pointer/object/floating layouts, LP-sensitive sizes,
SYNCHRONIZED alignment, inherited group usage, non-allocating TYPEDEF templates,
TYPE instance expansion, dynamic/unbounded metadata, and source provenance.
Dynamic and unbounded layouts remain analyzable but block executable publication
until their runtime storage semantics are implemented by the 0.4 profile.

The generated Enterprise COBOL catalog also owns all 82 intrinsic-function
identities/signatures and 28 special-register identities. Semantic analysis
records typed calls/references with source provenance, validates overloads,
argument classes, arity, homogeneous/variadic rules, format-literal context,
special-register operands and receiving restrictions, and infers result type and
fixed length. Recognized functions without an explicit interpreter route remain
analyzable but block executable publication.
