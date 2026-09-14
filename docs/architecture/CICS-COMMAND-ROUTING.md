# CICS command descriptor and semantic-family routing

Status: **Implemented**
Owner: **CICS provider maintainers**
Scope: **source-reviewed application-command contracts, generated registry shapes, and existing provider routes**
Applies from: **mainframe-env 0.9.0 development**

## Authorities

The readable identity authority is
[`command-descriptors.json`](../../conformance/0.9/cics/command-descriptors.json),
validated by
[`cics-command-descriptors.schema.json`](../../conformance/0.9/schemas/cics-command-descriptors.schema.json).
Its `application_catalog` contains the 263 mandatory `api-commands` row IDs,
official labels and two-byte EIB function codes. The separate `runtime`
collection retains 23 API operations and two explicit SPI compatibility
operations; it does not enlarge the application denominator or make the SPI
operations application-registry routes.

Objective command facts come from digest-pinned IBM CICS TS 6.x HTML, divided
into three disjoint source authorities:

- `sources-a` covers rows `0001`–`0088`: [map](../../conformance/0.9/cics/application-api-sources-a-map.json),
  [corpus](../../conformance/0.9/cics/application-api-sources-a-corpus.json),
  [projection](../../conformance/0.9/generated/cics-application-api-sources-a-candidates.json),
  and [review](../../conformance/0.9/cics/application-api-sources-a-review.json).
- `sources-b` covers rows `0089`–`0176`: [map](../../conformance/0.9/cics/application-api-sources-b-map.json),
  [corpus](../../conformance/0.9/cics/application-api-sources-b-corpus.json),
  [projection](../../conformance/0.9/generated/cics-application-api-sources-b-candidates.json),
  and [review](../../conformance/0.9/cics/application-api-sources-b-review.json).
- `sources-c` covers rows `0177`–`0263`: [map](../../conformance/0.9/cics/application-api-sources-c-map.json),
  [corpus](../../conformance/0.9/cics/application-api-sources-c-corpus.json),
  [projection](../../conformance/0.9/generated/cics-application-api-sources-c-candidates.json),
  and [review](../../conformance/0.9/cics/application-api-sources-c-review.json).

Each batch binds a topic manifest and extraction plan to content-addressed HTML
bodies in `$MAINFRAME_ENV_IBM_DOCS_CACHE`. Fresh bodies are obtained through the
repository browser-fetch bridge and the user's Chrome session; PDF is not a
source path. The projector emits structural facts and fragment hashes, while
the independent verifier reparses the pinned HTML before comparing the
projection. Reviews auto-accept objective matches, fail on source or
reprojection gaps, and retain row-and-dimension bounded ambiguities. There is no
human-only approval gate. Source projection and review alone grant no coverage,
semantic, execution or differential credit.

The semantic authority produced from all three accepted reviews is
[`cics-application-command-contracts.json`](../../conformance/0.9/generated/cics-application-command-contracts.json),
validated by its
[`schema`](../../conformance/0.9/schemas/cics-application-command-contracts.schema.json).
It freezes a 263-row contract across grammar, option legality and direction,
bounds, resource and capability intent, EIB/RESP/RESP2 and conditions,
applicability, effect, cancellation, audit and recovery. Its status is
`frozen-with-bounded-ambiguities`: all rows are closed as either source-resolved,
bounded, or not applicable, but bounded dimensions are not represented as
resolved. The contract explicitly has `execution_authority=false` and zero
coverage, semantic and differential credit.

The contract also binds one 121-name EIBRESP authority for dynamic
`HANDLE CONDITION` and `IGNORE CONDITION` clauses. Its early participant view
records two known mutating rows, 260 bounded-effect rows, one explicit UOW
boundary and 261 bounded-UOW rows. Those values describe current contract
certainty; they are not execution or conformance counts.

The same generator emits the compact
[`CICS application IR registry`](../../crates/foundation/mainframe-env-ir/src/generated/cics_application_registry.rs).
It contains all 263 API registry shapes with deterministic recognition,
option-shape, family, EIBFN and handler identities. Readiness is deliberately
split:

- 3 `typed-runtime` API routes (`READ`, `REWRITE`, and `SYNCPOINT`);
- 20 `legacy-compatibility` API routes that remain on the pre-existing raw
  compatibility path; and
- 240 `unready` rows that are recognized but fail explicitly as unsupported.

The 20 raw compatibility routes' implemented option subsets are owned by the
separate versioned
[`legacy-execution-options.json`](../../conformance/0.9/cics/legacy-execution-options.json)
catalog and its
[`schema`](../../conformance/0.9/schemas/cics-legacy-execution-options.schema.json).
It binds the logical 263-row application identity digest, not the physical
descriptor file, so runtime-readiness edits cannot invalidate frozen IBM
source receipts. The generator requires its route identities to match the
legacy API runtime set exactly and verifies every admitted option against a
current accepted source projection before emitting the registry.

The current 34 API routes are the only advertised application commands.
`ASKTIME ABSTIME` returns its packed-decimal destination and refreshes EIBDATE
and EIBTIME. Bare `ASKTIME` is a distinct route that refreshes only those two
packed-decimal EIB fields; it cannot manufacture an ABSTIME destination. Both
forms lower through distinct typed plan operations, while retained raw
ASKTIME artifacts remain readable by the compatibility interpreter.
The typed FORMATTIME route currently admits only its source-checked legacy
subset: packed ABSTIME input, valued DATESEP/TIMESEP, five explicit date
formats, TIME, MILLISECONDS, and common response options. Other official
FORMATTIME fields remain explicit compiler rejections until their output and
timezone contracts are implemented.
`ABEND` also lowers through a typed task plan: ABCODE is captured as a bounded
literal or a pre-resolved 1–4 character storage input, and CANCEL/NODUMP remain
distinct flags. Retained raw ABEND artifacts remain readable, but new
compilations do not carry their command text across the executable boundary.
`HANDLE ABEND` uses the same typed task dialect with a source label or bounded
program-name input and mutually exclusive CANCEL/RESET actions; its provider
continues to own authorization and durable active/canceled exit state.
The typed local LINK subset binds PROGRAM and an optional COMMAREA before
dispatch. COMMAREA is one input/output storage identity, so registering its
return destination cannot replace the captured request bytes. Channel, explicit
length, input-message, remote-system, transaction, and SYNCONRETURN forms remain
compiler rejections until their separate contracts are implemented.
Each legacy route admits only the source-valid option subset whose behavior is
implemented by that raw handler. A catalog-known option outside that subset
fails explicitly before compatibility lowering instead of being silently
dropped. The pre-registry `DATASET` spelling remains an exact alias for `FILE`
on the existing file and browse operations; specifying both spellings fails as
an ambiguous resource selection.
`automatic_registration` remains false, the default handler is null, and an
unready row cannot reach a generic-success fallback. The application registry
does not accept or dispatch SPI or FEPI identities. A separate generated,
compiler-only compatibility descriptor admits exactly `INQUIRE PROGRAM` to the
pre-existing raw `Inquire` route; it is bound to SPI row `0155`, excluded from
the 263-row registry and its digest, and does not admit `SET FILE`, other
`INQUIRE` forms, or unknown options. The two retained SPI compatibility
operations otherwise remain confined to the separate legacy runtime
collection; SPI/FEPI completion belongs to 0.10.

[`tools/generate_cics_descriptors.py`](../../tools/generate_cics_descriptors.py)
deterministically writes the provider descriptors, host-API identity table,
263-row contract, compact IR registry, and the isolated compiler-only
`INQUIRE PROGRAM` SPI compatibility descriptor. `--check` compares all
generated outputs without writing. Schema, freshness, digest and
module-boundary checks run under `cargo xtask architecture-fast --check`;
hand-editing a generated artifact or changing an authority without
regeneration fails the gate.

CIC-901 is therefore an incremental, non-release architecture boundary. It
freezes source-backed contracts and fail-closed registry shape so CIC-902 and
later work can implement vertical semantic families on the shared runtime. It
does not claim that 263 commands execute, satisfy conformance, pass licensed
differentials, or make 0.9.0 release-ready.

## Existing runtime families

| Family | Owns |
|---|---|
| `task-control` / `handle-state` | task context, durable HANDLE state, ASSIGN, RETRIEVE, ABEND, and pseudo-conversation RETURN |
| `time` | ASKTIME clock acquisition and FORMATTIME conversion |
| `program-control` | program inquiry, LINK, and XCTL |
| `terminal-control` / `terminal-run` | BMS and text send/receive behavior plus terminal-task lifecycle cleanup |
| `file-control` | file status, keyed I/O, and browse behavior |
| `queue-control` | transient-data queue writes |
| `recovery` | SYNCPOINT coordination, rollback, and subsystem unit-of-work completion |

This table describes the seven families already present in the 25-row runtime
collection. The 263-row application registry also assigns every row a
deterministic future family owner, but that assignment is routing shape rather
than an executable handler. `CicsService::invoke_run` selects an existing
runtime descriptor first and routes on its family. Each existing runtime family
has a reviewed implementation module under
`crates/providers/mainframe-env-cics/src/handlers/`; no command is dispatched by
an ad hoc keyword match in the service monolith. The service retains the shared
session/run state, authorization boundary, durable compare-and-swap primitives,
common condition/response machinery, and bounded codecs used across families.
The accepted `retention.rs` sibling owns provider-lifecycle codecs and
dependency descriptions; it is deliberately outside the command-family layer
and cannot become an alternate dispatch path.

The compiler/runtime boundary is governed by
[ADR-0011](../decisions/0011-typed-language-hir-and-semantic-ir.md). A migrated
COBOL CICS family resolves static command identity, options, resource bindings,
and output destinations before execution, then emits the existing owned typed
request through the execution coordinator. The provider never parses COBOL HIR
or source syntax, and the migration cannot introduce a parallel CICS provider,
store, unit-of-work protocol, or condition authority.

Execution-context facts that are not command operands travel in bounded,
versioned invocation bindings. `mainframe-env.cics.execution-context@1`
currently distinguishes local execution, DPL with `SYNCONRETURN`, DPL without
syncpoint ownership, and `EXECUTIONSET=DPLSUBSET`. The recovery handler uses
that context before changing UOW state; CIC-905 owns populating it from a
future public DPL route. It is not embedded in source tokens or inferred from a
successful transport call.

For a DPL invocation that owns the syncpoint, the bounded
`mainframe-env.cics.syncpoint.remote-outcome@1` binding records whether the
remote system is commit-capable or unable to commit. The latter drives the UOW
into rollback, persists the rolled-back terminal state, backs out local
recoverable work, and raises `ROLLEDBACK` with RESP 82. The binding is rejected
outside a `dpl-synconreturn` context; CIC-905 owns populating both bindings from
the eventual public DPL transport.

## Change contract

Adding an identity to the application projection requires a reviewed change to
the pinned official denominator and regeneration. It never updates
`CicsOperation`, dispatch or coverage by itself.

Adding or changing an executable typed CICS command requires one reviewable
change that:

1. updates the readable descriptor catalog and its official row binding;
2. regenerates the Rust descriptor module;
3. adds semantics to the named family module, keeping it below the hard
   1,200-production-line limit; and
4. adds focused condition, authorization, durability, and recovery tests
   appropriate to the operation.

A new semantic family changes the accepted module inventory and requires an ADR
amendment. Generated descriptors never contain behavior, and handler modules
are never generator-owned. [ADR-0010](../decisions/0010-rust-module-review-budgets.md)
records the hard limits and exact legacy ceiling policy.

## Verification

```bash
python3 -B tools/generate_cics_descriptors.py --check
python3 -B tools/generate_cics_source_map.py --batch all --check
python3 -B conformance/0.9/tools/fetch_cics_application_sources.py --batch all --cache $MAINFRAME_ENV_IBM_DOCS_CACHE --check
python3 -B conformance/0.9/tools/extract_cics_application_sources.py --batch a --cache $MAINFRAME_ENV_IBM_DOCS_CACHE --check
python3 -B conformance/0.9/tools/extract_cics_application_sources.py --batch b --cache $MAINFRAME_ENV_IBM_DOCS_CACHE --check
python3 -B conformance/0.9/tools/extract_cics_application_sources.py --batch c --cache $MAINFRAME_ENV_IBM_DOCS_CACHE --check
python3 -B conformance/0.9/tools/verify_cics_application_sources.py --batch a --cache $MAINFRAME_ENV_IBM_DOCS_CACHE
python3 -B conformance/0.9/tools/verify_cics_application_sources.py --batch b --cache $MAINFRAME_ENV_IBM_DOCS_CACHE
python3 -B conformance/0.9/tools/verify_cics_application_sources.py --batch c --cache $MAINFRAME_ENV_IBM_DOCS_CACHE
python3 -B conformance/0.9/tools/review_cics_application_sources.py --batch a --cache $MAINFRAME_ENV_IBM_DOCS_CACHE --check
python3 -B conformance/0.9/tools/review_cics_application_sources.py --batch b --cache $MAINFRAME_ENV_IBM_DOCS_CACHE --check
python3 -B conformance/0.9/tools/review_cics_application_sources.py --batch c --cache $MAINFRAME_ENV_IBM_DOCS_CACHE --check
python3 -B -m unittest tools.tests.test_cics_descriptors tools.tests.test_cics_source_map tools.tests.test_module_boundaries
cargo test -p mainframe-env-host-api -p mainframe-env-cics --all-features --locked
cargo xtask architecture-fast --check
```
