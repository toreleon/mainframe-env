# mainframe-env-interpreter

Ownership: the deterministic, bounded Core-MIR reference machine. Non-goals:
COBOL syntax, concrete providers, async scheduling, stores, native/JIT/Wasm, or
ambient clock/input. Allowed dependencies are owned IR, execution, host, and
diagnostic contracts.

Invariants: one bounded quantum returns one logical drive action; host effects
are ordered and explicit; storage, output, frames, and steps are bounded.
Verify with `cargo test -p mainframe-env-interpreter`.

COBOL entry arguments bind in `PROCEDURE DIVISION USING` order from the
`entry_formals_v1` IR config attribute. An empty list leaves declared LINKAGE
storage unbound. Historical payloads without this attribute reject inbound
arguments, except batch entry with no LINKAGE roots, where the PARM is ignored.
