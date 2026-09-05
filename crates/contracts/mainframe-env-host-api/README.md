# mainframe-env-host-api

Ownership: typed host and CICS requests/results, effect metadata, capability
descriptors, generated official semantic identities, and immutable capability
and subsystem-handler registry snapshots. Non-goals: concrete providers,
database rows, async runtimes, broad application state, or deriving semantic
coverage from generated identities. It depends only on the execution contract.

Invariants: every request/result is bounded; mutations carry effect sequence
and idempotency identity; capability resolution is deterministic; official and
custom semantic namespaces cannot overlap; and no generated identity installs a
handler. Verify with `cargo test -p mainframe-env-host-api` and
`cargo xtask semantic-identities --check`.

## Official handler closure

`OfficialHandlerUnit` selects an exact baseline/subsystem/unit from the existing
generated catalog. `validate` requires every identity in that unit to have an
explicit ready handler in `SubsystemHandlerRegistry`. Missing and unready
handlers fail with their official identity; unknown or empty scopes fail rather
than passing vacuously. Other official units and custom handlers may coexist,
but cannot fill a missing identity or change the selected denominator.

`publish` validates the replacement before delegating to the existing monotonic
`SubsystemHandlerPublisher`. An incomplete replacement cannot disturb the held
or published snapshot. The helper does not install, invoke, or persist handlers,
add another routing authority, or activate a new product profile.

The CICS application unit is `api-commands` in the pinned
`ibm-cics-ts-6x-2026-08-31` baseline. Its 263 identities exclude SPI and FEPI.
Registration closure does **not** prove grammar, option legality, EIB/RESP,
conditions, effects, authorization, recovery, or licensed equivalence. Internal
closure tests contribute no official conformance verdicts.

Focused validation: `cargo test -p mainframe-env-host-api official_handler_unit`.
