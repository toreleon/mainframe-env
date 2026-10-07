# CIC-906 bounded CICS validation slice

This slice extends the R-24 assurance inventory at the `3f749203` base with
CICS-specific source and plan inputs. The application registry remains **175
typed, 0 legacy, 88 unready**. It changes no CICS command, option, response, or
transaction semantics, and earns no IBM or licensed differential credit.

| Boundary | New validation | Scope and limit |
| --- | --- | --- |
| COBOL `EXEC CICS` source grammar and option decoder | `cics_source_parser` fuzz target wraps each bounded command body in a valid free-format program; three seeds include a valid syncpoint, a file form, and a duplicate/unknown option. The compiler property rejects generated unknown top-level options. | Exercises source lexing, clause parsing, descriptor admission, and typed resolution for free-format source. No complete 263-command grammar claim. |
| Canonical typed MCEP v1/v2 decoder | `cics_plan_decoder` fuzz target decodes under explicit byte/count/name/literal limits and checks accepted plans re-encode and decode; v2 accepted bytes must already be canonical. Seeds include v1/v2 syncpoint and a truncated count. A property varies file identities, checks both versions, trailing bytes, and the encoded-byte bound. | Does not establish all option/operation tag combinations or runtime dispatch behavior. |
| Owner-fenced replay, cancellation, and syncpoint | Two Loom models enumerate a stale owner versus restart claim and cancellation versus commit under a version/epoch fence, using the shared `EffectState`. Focused existing provider tests exercise real CICS replay, cancellation, syncpoint, and SQLite reopen paths. | The Loom code is a contract model, not a proof of the provider implementation. SQLite reopen is local durability evidence, not a separate-process crash or PostgreSQL result. The separately integrated [PostgreSQL durable-profile slice](CIC-906-POSTGRES-VALIDATION.md) adds bounded adapter, read-version, and checkpoint-retention selectors. |

The registered fuzz smoke budget is 256 runs per target with 4,096-byte inputs;
the run script copies seeds outside Git. These tests are diagnostic validation
of the stated boundary only. The following CIC-906 matrix cells remain open:

- All 263 source forms and every accepted option's semantic effect; fixed and
  variable source formats; complete v1/v2 tag-space and cross-version migration.
- Cross-family concurrent file, queue, program, APPC/MRO, and DPL transactions;
  selected product route with independent EIB/RESP assertions for each family.
- Cross-family PostgreSQL concurrent owners and separate-process restart, audit
  saturation, journal failure after provider mutation, full reconciliation and
  retention coverage, and scale/soak. The focused PostgreSQL adapter and CICS
  checkpoint-retention selectors do not close these campaign cells.
- Licensed CICS differential capture and the exact-candidate full-phase gate.

The frozen CICS TS 6.x source maps, generated contracts, and registry are
unchanged. This validation-only slice requires no new IBM semantic source
review. Existing family source citations remain in the cics.application-api status document.
