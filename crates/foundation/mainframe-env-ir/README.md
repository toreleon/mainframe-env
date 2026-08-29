# mainframe-env-ir

Ownership: generic typed-ID IR, operation catalogs, verification, canonical
text, and the owned binary envelope. Non-goals: COBOL ASTs, provider access,
execution scheduling, or infrastructure codecs. Allowed dependencies are the
source/diagnostic foundation and SHA-256 implementation.

Invariants: all arenas and strings are bounded; unknown operations and invalid
references fail verification; binary readers authenticate and bound payloads
before allocation. Verify with `cargo test -p mainframe-env-ir`.
