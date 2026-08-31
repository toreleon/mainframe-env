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

The bounded owned definitions for reached DFHAID, DFHBMSCA, SQLCA, and MQ
copybooks are documented in
[`docs/compatibility/CARDDEMO-COPYBOOKS.md`](../../../docs/compatibility/CARDDEMO-COPYBOOKS.md).

The semantic data model uses unique qualified identities while retaining simple
COBOL names for resolution. Recursive groups, sibling REDEFINES, levels
66/77/78/88, OCCURS ranges/dependencies/indexes, FILE/LINKAGE roots, subscripts,
reference modification, and reached display/binary/packed/edited bytes are
bounded explicitly.
