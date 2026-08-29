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
