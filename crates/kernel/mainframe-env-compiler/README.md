# mainframe-env-compiler

Ownership: bounded COBOL source decoding, lossless syntax, typed semantic/HIR,
Core-MIR lowering, legality, and the public compiler service. Non-goals: async
I/O, provider execution, native code generation, or persistence. Allowed
dependencies are owned compiler foundations/contracts plus private Rowan syntax
storage.

Invariants: recovery nodes never publish; every executable statement lowers to
a registered Core-MIR operation; source, layout, and provenance bounds are
enforced. Verify with `cargo test -p mainframe-env-compiler`.
