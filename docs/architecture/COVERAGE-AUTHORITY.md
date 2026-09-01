# Official coverage authority

Status: **Frozen for mainframe-env 0.2.0**

## Authority boundary

The reviewed source receipt index under `conformance/0.2/catalogs/` is the
authority for a baseline's publication identity, source digest, normalized
rows, and immutable denominator. IBM publication bytes are not repository
assets. A source update creates a new baseline and catalog digest; it never
rewrites an accepted denominator.

`mainframe-env-coverage` owns the typed coverage row, immutable evidence, and
append-only snapshot contracts. It has no parser, handler, provider, route, or
application dependency and is outside the production profile. Repository
tooling validates its durable JSON projections.

## Independent gates

Every row declares one or more applicable gates from this ordered vocabulary:

1. `recognized`
2. `validated`
3. `executed`
4. `conditioned`
5. `recovered`
6. `differential`

Gate results do not imply one another. Execution does not imply recognition
evidence, a conditioned pass does not imply recovery, and an ordinary test
environment cannot produce a differential pass. A differential pass requires a
pinned licensed IBM environment receipt.

`complete` is a derived projection only. It is true exactly when the latest
retained evidence for every applicable gate passes. A failed observation may be
followed by a later passing observation, but both immutable records remain in
the row history. Evidence sequence reuse, replacement, or reordering fails
closed.

## Ledger and denominator rules

A coverage snapshot binds a baseline ID, catalog digest, denominator, and
monotonic generation. The first accepted snapshot anchors the digest and
denominator for that store. Later generations may append evidence and change
derived gate states, but cannot change the anchor. Publishing a snapshot whose
evidence has not first been appended to the evidence store fails.

For gate `g`, the denominator is the number of mandatory rows to which `g`
applies and the numerator is the number whose latest evidence for `g` passes.
There is no weighting or rounding. The checked-in 0.2 generation contains all
1,506 normalized rows, no semantic evidence, zero complete rows, and zero
numerators for every gate.

## Generation boundary

Catalog extraction and code generation establish identities and exhaustive
registration only. They never create a coverage evidence record and therefore
cannot increment `recognized`, `validated`, `executed`, `conditioned`,
`recovered`, or `differential`. Public selected-route evidence remains required
for semantic credit.
