# mainframe-env-interpreter

Ownership: the deterministic, bounded Core-MIR reference machine. Non-goals:
COBOL syntax, concrete providers, async scheduling, stores, native/JIT/Wasm, or
ambient clock/input. Allowed dependencies are owned IR, execution, host, and
diagnostic contracts.

Invariants: one bounded quantum returns one logical drive action; host effects
are ordered and explicit; storage, output, frames, and steps are bounded.
Verify with `cargo test -p mainframe-env-interpreter`.
