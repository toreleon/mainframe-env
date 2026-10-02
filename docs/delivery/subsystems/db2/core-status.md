# Db2 — Engine and common SQL progress

Subsystem: **db2**
Phase: **core**
Target release: **0.12.0**

Status: **In progress — second/third-wave pure surfaces sealed; catalog/binder and execution pending**

This recovery slice starts from `origin/main` commit `26437e2c`. This file is
program control, not product conformance or licensed execution evidence.

## Original lane handoff and merge reconciliation

The original `codex/implement-0-12-0` lane was paused at the user's request on
2026-10-01 and published as draft PR #380. Its completed feature commits remain
in branch history. The merge with `main` at `2f5191be` retains the reviewed
recovery implementations, module layout, source pins and acceptance boundaries
already integrated through the separate feature PRs. Superseded duplicate
modules and the original parser dependency are not reintroduced.

The original second wave remains declared but unimplemented. Its four private
module scaffolds contain no behavior, and all four CLI workers exited before
making changes because the CLI account rejected `gpt-6.1-sol`.

| Slice | Declared scope and owner |
|---|---|
| `DB2-1201.insert-values-syntax` | SQL 0096; bounded INSERT INTO target, optional unique columns and uniform-width parenthesized VALUES rows; `insert_syntax`. |
| `DB2-1201.create-index-common-syntax` | SQL 0039; uniqueness modes, qualified names and bounded unique column keys with ordering; `create_index_syntax`. |
| `DB2-1201.create-view-common-syntax` | SQL 0059; qualified name, optional result columns, SELECT core and CHECK OPTION; `create_view_syntax`. |
| `DB2-1202.schema-qualification` | Pure static/dynamic qualification with explicit context; `name_resolution`. |

These declarations grant no row, execution or coverage credit. Implementation
remains paused; the conflict repair does not complete v0.12 or waive its gates.

## Resumed implementation on 2026-10-02

The user resumed the original goal after PR #380 merged. The candidate branch
`codex/continue-v012-20261002` starts from current `origin/main` at
`213ed878e`. It consumes the reviewed recovered parser/type modules and the
subsystem documentation layout. The historical pause above is superseded by
this resumption. Worker configuration is Codex CLI 0.160.0, `gpt-6.1-sol`, high
reasoning, goals enabled, fast mode disabled and default service tier.

Four isolated worktrees implement the existing second-wave slices. Each worker
owns only its named Rust module and one unique changelog fragment. Shared
exports, status, provider documentation, catalog obligations and derived
documentation remain manager-owned. Each feature is verified and sealed with
its own ID before another feature starts.

| Slice | Mandatory bounded obligations before acceptance |
|---|---|
| `DB2-1201.insert-values-syntax` | Preserve target/columns and row boundaries; accept expression, DEFAULT and NULL values in parenthesized rows; enforce unique columns, uniform widths and explicit column width; reject column references inside VALUES expressions, qualified column-list extensions, nonparenthesized rows, INCLUDE/OVERRIDING/fullselect/CTE/host arrays/isolation/QUERYNO/FOR-n-ROWS and malformed or trailing input. |
| `DB2-1201.create-index-common-syntax` | Preserve uniqueness modes and explicit/default key ordering; enforce at most 64 distinct unqualified column keys and configured bounds; reject expression/XML/auxiliary/BUSINESS_TIME/INCLUDE/physical/partition/storage forms and malformed or trailing input. |
| `DB2-1201.create-view-common-syntax` | Preserve name/result columns/SELECT core and explicit/default CHECK OPTION; enforce known result width, unique columns and required names for unnamed/duplicate projections; reject wildcard widths requiring catalog binding, host variables/parameters, CTE/fullselect extensions and unsupported functions/clauses. Catalog-dependent validity and CHECK OPTION applicability remain pending. |
| `DB2-1202.schema-qualification` | Use explicit bounded static/dynamic run/bind/define/invoke context, preserve qualified names, classify missing context and unsupported synonym/SQL-path/EXPLAIN/catalog/authorization resolution explicitly, and never infer an application-specific name rule. |

These are pure syntax/qualification kernels with no host route, catalog/store
mutation, authorization, SQLCA, transaction or backend integration. Applicable
acceptance is focused positive/negative/span/bounds matrices, the affected Db2
package test/check, Rust 1.95 compatibility, catalog/format/diff gates, changelog
validation and exact-path feature sealing. Memory-only tests grant no durable
or official row credit. Manager surface-integration slices follow these four
seals; broad execution still requires the exact common/deferred freeze.

The four current pinned topic hashes are unchanged from the original declaration:
INSERT `5411a1a3404556e93aacca6bc0c1b6cd04ec9db4717abb70a4f2eff93805ce30`,
CREATE INDEX `33c93323290bf2768d4107445cdb8c473b797741444ca697c80f4f9d46b32a91`,
CREATE VIEW `2e5f8b1f122d42ee5fceb45bff571596b834a287ae5dba17fbed4b40f9230a26`,
and name resolution `a1e2b49f72cd742e3d5a4866417ba9acf30dccca8091d0b48a1483ec50019f8d`.
Their exact manifest byte counts and hashes were verified in the supplied raw
archive and read with the shared plain-text parser. The ordinary cache reader
still lacks a verified Db2 TOC; no network refresh or source credit is inferred.

### Current source availability and manager integration declarations

The resumed candidate uses the owner-approved #350 repins already committed on
`main`, with topic-set digest
`83f04d82753771425aab203c7913dd9fea1c768766cf315df421100015e72463`.
On 2026-10-02, checking the 174 catalog-referenced topics against their exact
manifest byte counts and SHA-256 values found 171 matching retained/archive
bodies and no accepted mismatches. The three unavailable pins are SQL 0013
ALTER SEQUENCE (`e02c3cdfe5c1bfac9f7043b6387acec421c8c069553bbeff2d1a9d13c247d163`),
SQL 0049 CREATE STOGROUP (`b46b9b521a329df85aa35f24b6015f84fe60c51cb1e8ba25349bc272004be4c5`),
and SQL 0127 SET CURRENT APPLICATION ENCODING SCHEME
(`cf5be34926528b6653fde14d0ff6a42a8069b959168d1a7f7f2bf99211de030c`).
Availability is not semantic review, full-catalog freeze or execution evidence.
The historical 27-topic and numeric-constants blockers below describe the
recovered candidate, not the current source set. The current numeric-constants
introduction (`bf0cb79eac0636348209b6919c4f4ee680f1c3d2ada9cab186dddfdd39a13fa8`)
was verified and read locally; lifting existing parser fences requires a later
declared, tested slice. No network refresh was performed.

After worker seals, the manager owns these separate public integration slices:

| Slice | Exact boundary and acceptance |
|---|---|
| `DB2-1201.second-wave-syntax-surface` | Export the three owned INSERT VALUES / CREATE INDEX / CREATE VIEW APIs through `lib.rs`; add `tests/second_wave_syntax.rs`, one unique changelog fragment, provider README and progress/derived documentation. Preserve the worker rejection/bounds/span obligations above; test public callers and combined package behavior. No statement dispatch, catalog/backend mutation, SQLCA or whole-row credit. |
| `DB2-1202.qualification-surface` | Export the owned qualification candidate/context/error APIs through `lib.rs`; add `tests/schema_qualification.rs`, one unique changelog fragment, provider README and progress/derived documentation. Test public static/dynamic contexts and unresolved synonym/catalog behavior. No catalog lookup, privilege decision, execution or statement-row credit. |

Both consume the sealed worker kernels without changing their IBM semantics.
They are pure public library surfaces, so backend/durability, authorization,
recovery and licensed execution remain outside their acceptance claims rather
than being marked passed. Focused public/package regressions, formatting, Rust
1.95 compatibility, catalog/changelog/docs checks, dependency policy and exact
feature seals apply. The user confirmed no licensed Db2 13 environment is
available; implementation continues with differential explicitly pending.

The second-wave `DB2-1201.create-index-common-syntax` and
`DB2-1202.schema-qualification` worker kernels are integrated as separate sealed
feature commits. Their source/body and bounded obligations above remain the
acceptance boundary. The qualification surface exports the owned context,
candidate and located error types plus `qualify_db2_name`; public integration
tests preserve explicit and derived names and verify that caller-confirmed
synonym absence never becomes object-existence or authorization evidence.
INSERT and CREATE VIEW were amended after manager identifier-escape review and
integrated as their own sealed commits. The
`DB2-1201.second-wave-syntax-surface` exposes these APIs and the CREATE INDEX
kernel without a shared statement dispatcher or execution route. Public tests
exercise owned output, decoded names, relocated spans, duplicate/width checks,
forbidden references/functions and aggregate budgets. Literal escape text,
SELECT's compiled raw-name ceiling and CAST's delimited first-type-component
restriction remain inherited limitations, not licensed behavior. No whole-row
recognition or execution count changes. Repository-wide architecture validation remains
blocked by the unrelated unavailable CICS topic
`SSJL4D_6.x/applications/designing/dfhp37p.html` and the batch service's
7,404-production-line count exceeding its 7,402-line legacy ceiling; these are
not retried as Db2 tests.

### Third-wave declarations

The second-wave public syntax integration is sealed in `f400452c`; its affected
package sequence passed 173 unit and 18 integration tests, Rust 1.95 check,
formatting, statement-catalog, changelog and documentation gates. Those receipts
belong to that tested feature input, not to the following unimplemented kernels.
The unchanged dependency-policy inputs passed during the qualification-surface
sequence. Full-catalog recognition, common/deferred freeze and execution credit
remain pending.

The next four isolated CLI workers retain `gpt-6.1-sol`, high reasoning, goal
mode, bypass, fast mode disabled and default service tier. Each owns exactly its
named private Rust module and unique changelog fragment; shared exports,
dispatchers, status, catalog obligations and derived docs remain manager-owned.

| Slice | Exact source-backed bounded obligations | Owned module / fragment |
|---|---|---|
| `DB2-1201.searched-delete-syntax` | SQL 0065: DELETE FROM qualified target with optional existing-expression search condition; preserve names and all original-source spans; reject bare-value WHERE, invalid predicate compositions, positioned CURRENT OF, correlation/period/INCLUDE/SET/fullselect/fetch/isolation/SKIP LOCKED/QUERYNO forms and extra statements. Binding, target kind, privileges, constraints and deletion remain pending. | `delete_syntax.rs`; `db2-searched-delete-syntax.toml` |
| `DB2-1201.searched-update-syntax` | SQL 0155: qualified target, nonempty unique unqualified single-column SET assignments using common expression/DEFAULT/NULL, optional search condition; preserve assignment boundaries, names and relocated spans; enforce aggregate limits and structural predicates; reject positioned, tuple/row-fullselect/UNPACK, correlation/period/INCLUDE/isolation/SKIP LOCKED/QUERYNO forms and aggregate assignment functions. Catalog-dependent function distinctions and all mutation/type/default/nullability semantics remain pending. | `update_syntax.rs`; `db2-searched-update-syntax.toml` |
| `DB2-1201.open-fetch-host-syntax` | SQL 0100 / 0078: OPEN unqualified cursor with optional bounded static-host USING list or SQLDA descriptor; single-row FETCH with omitted/NEXT/PRIOR/FIRST/LAST/CURRENT orientation, optional FROM and optional static-host INTO list or SQLDA descriptor. Preserve omitted spelling, host case, indicators, decoded cursor identity and spans; reject SQL PL globals/arrays, structures, rowsets, sensitivity, WITH CONTINUE, BEFORE/AFTER, ABSOLUTE/RELATIVE and undeclared forms. Cursor state, scrollability, parameter/target counts, host binding and SQLCA remain pending. | `cursor_operation_syntax.rs`; `db2-open-fetch-host-syntax.toml` |
| `DB2-1202.integer-decimal-constant-types` | Language-element kernel, no statement-row claim: classify bounded signed/unsigned integer spellings of at most 19 digits by INTEGER/BIGINT range and out-of-BIGINT DECIMAL; classify decimal-point forms and unpointed out-of-BIGINT forms up to 31 digits with exact precision/scale including leading/trailing zeros. Return existing owned resolved type with NOT NULL, not an evaluator. Longer unpointed in-range spellings, exponent/DECFLOAT/special forms, comma decimal conventions and malformed inputs fail explicitly as outside this initial subset. No lexer-fence lifting, conversions or expression binding. | `numeric_constant_types.rs`; `db2-integer-decimal-constant-types.toml` |

Pinned topics were verified in the supplied archive and read with the shared
plain-text parser after ordinary search/read reported the unverified TOC:
DELETE (`0e8760f121bb0e982541d6442ed565adbd291678dfbab44d6d2fe2dc4d176317`,
225829 bytes), UPDATE
(`0ddb9b81001292f17d7b2fe9d20db41274bb921a85ca11a8fab3ec2c0931c4ab`,
252756 bytes), OPEN
(`b0b06e74f611c2840d7cb81d8c64a151ea7dbb02404aa911c367defd5cc8eeee`,
44706 bytes), FETCH
(`b8109d9f6389937ae432fa9ff1abd49df237598fa81216032b752c31b413e57d`,
211364 bytes), and constants introduction
(`bf0cb79eac0636348209b6919c4f4ee680f1c3d2ada9cab186dddfdd39a13fa8`,
17915 bytes). Topic names are `db2z_sql_{delete,update,open,fetch}.html` and
`db2z_constantsintro.html` under the same pinned Db2 13 SQL-reference path.
Identifier, expression/search-condition and data-type context topics must also
be consulted before their rules change.

All four are pure library kernels with no host route, store/catalog mutation,
SAF decision, transaction binding, SQLCA mapping or official row credit. Their
applicable acceptance is positive/negative/boundary/located diagnostic matrices,
affected package tests/check, Rust 1.95 compatibility, formatting, catalog and
changelog gates plus exact-path seals. A new module must stay below 1200
production lines. Unchanged global architecture/source blockers are reported,
not retried without new evidence. Public integration follows separate seals.

The FETCH source's target-variable description also requires that a target not
appear more than once in INTO (verified topic line 454). The syntax kernel
rejects exact repeated primary host-target spelling; host-language aliases and
case-equivalent targets require later binding. Repeated OPEN input references
remain legal. Neither check infers scalar/structure identity from a name.

Before integration, the manager declares these independent public slices:

| Slice | Manager-owned boundary and required checks |
|---|---|
| `DB2-1201.third-wave-syntax-surface` | Export DELETE, UPDATE and OPEN/FETCH owned syntax APIs in `lib.rs`; add `tests/third_wave_syntax.rs`, `changes/unreleased/db2-third-wave-syntax-surface.toml`, provider README and status/derived docs. Public checks cover owned names/expressions/host operands, original spans, valid predicate composition, duplicate assignments/targets, aggregate bounds and the declared rejections. No shared statement dispatcher or durable host-route change. |
| `DB2-1202.numeric-constant-type-surface` | Export the bounded numeric constant classifier, existing-type wrapper, limits and fixed located errors in `lib.rs`; add `tests/numeric_constants.rs`, `changes/unreleased/db2-numeric-constant-type-surface.toml`, provider README and status/derived docs. Public checks cover signed INTEGER/BIGINT range boundaries, exact DECIMAL precision/scale, source/spelling/span limits and explicit deferred forms. No lexer-fence lifting, evaluation, conversion or expression binding. |

Each consumes only reviewed sealed kernels, runs affected public/package
regressions plus formatting, Rust 1.95, catalog/changelog/docs and exact-path
seal checks, and commits before the other integration slice begins. Dependency
policy is reused only when its exact inputs are unchanged. Backend, recovery,
authorization and licensed claims remain absent for these pure library routes.

Catalog inspection confirms that the accepted application catalog v1 owns
`max_bytes`, Raw/Varchar result framing, defaults and nullability, but no exact
SQL scalar type/precision/scale/CCSID/collation. Its legacy host-name matching
also differs from owned SQL identifier identity. A later binder must not infer
SQL types from byte layout or treat legacy normalized names as equivalent to
all SQL identifiers. Typed catalog evolution must retain the existing package,
generation selection and persistence authority with explicit versioned metadata
and compatibility tests; no parallel catalog or private executor is introduced
by this wave. That boundary is not yet implemented or accepted.

The four third-wave kernels have now passed isolated review and were integrated
as separate sealed feature commits. Their package checks, source identities
and receipts do not establish execution or full-row recognition. The public
numeric slice adds four integration regressions for signed range boundaries,
precision/scale, owned results, UTF-8/CRLF locations, bounded failures and
deferred forms while leaving shared lexer fences unchanged. Its pinned sources
are `db2z_constantsintro.html` (17915 bytes,
`bf0cb79eac0636348209b6919c4f4ee680f1c3d2ada9cab186dddfdd39a13fa8`)
and `db2z_datatypesintro.html` (22904 bytes,
`a488006755eedd9ef58da3ba8ef9f304a3d79c3910cc39da637dda1f3c38f570`),
under the same Db2 13 baseline. These language elements have no independent
statement-catalog row. Licensed differential remains pending by user direction.

The public syntax slice adds six integration regressions for searched DELETE,
searched UPDATE and static-host OPEN/FETCH. It preserves owned decoded names,
original complete-source spans, recursive scalar/predicate distinctions,
orientation spelling/defaults, host/indicator/descriptor operands, duplicate
effective assignment names, exact FETCH targets and aggregate resource budgets.
No shared statement dispatcher, cursor state, SQLCA, catalog or durable route
changes. Pinned Db2 13 catalog rows SQL0065, SQL0155, SQL0100 and SQL0078 bind
the DELETE, UPDATE, OPEN and FETCH topic identities declared above. The missing
scalar-function overview is reported by the UPDATE worker; ambiguous
aggregate-family calls remain explicitly deferred pending function binding.
Full-row recognition, common/deferred freeze, execution and licensed gates
remain pending. These partial APIs cannot close DB2-1201 or DB2-1202.

## Fourth-wave catalog/binder prerequisites

Before further implementation, the manager declares two independent pure type
kernels and one read-only catalog evolution review. All use current owned AST
and resolved type contracts; no executor, value conversion or parallel catalog
is introduced. Public integration follows reviewed feature seals.

| Lane | Ownership and mandatory boundary |
|---|---|
| `DB2-1202.arithmetic-result-types` | `src/arithmetic_types.rs` and a unique `db2-arithmetic-result-types.toml` fragment. Unary/binary numeric result metadata, explicit DEC15/DEC31 and minimum-divide-scale context, preserved constant-digit provenance, nullability, and explicit runtime truncation/overflow/warning obligations. No guessing installation/package options or evaluation. |
| `DB2-1202.result-combination-types` | `src/result_combination_types.rs` and a unique `db2-result-combination-types.toml` fragment. Ordered numeric/binary/datetime result-type combination, bounded operands, nullability and untyped-NULL handling; character/graphic CCSID-dependent combinations fail explicitly until attributes exist. No CASE evaluation or function overload resolution. |
| Catalog evolution review | Read-only current catalog/compiler/package/server/store/rollback compatibility inspection. Report exact existing authority paths, required versioned metadata, finite reader/writer policy, canonical identity and migration boundaries outside Git. Do not edit code or claim a catalog feature complete. Manager chooses the integrated contract before catalog mutation. |

Pinned arithmetic topic `db2z_witharithmeticoperators.html` (37064 bytes,
`83a3db8b0d913dcbaff86c47624fda48fd2d462988499f83e73a6965d0f7d459`),
result rules `db2z_rules4resultdatatypes.html` (36969 bytes,
`7c23258e9e9f63a5be873ca3ce6f87a2f9b893d4f0aab7d0799b7f02e6bcc206`)
and CASE `db2z_caseexpression.html` (39911 bytes,
`fac861b774379be825c697a83d9447f40c125fbda877f716714b0e8ea930cd4a`)
were verified against the manifest and read from retained/raw local HTML under
the Db2 13 baseline. Language elements have no standalone statement row.
Ordinary search/read remains TOC-blocked. Package/MSRV, focused matrices,
format/catalog/changelog/docs/dependency policy and exact-path seals are
required for the two code slices. Global unchanged architecture blockers are
not retried. These declarations grant no full-row or licensed execution credit.

The read-only catalog review completed against `c53976ed` without checkout
changes, builds or tests. It identified two additional integration obligations:
legacy cells cannot establish NULL versus empty, and rollback to a selected
package without `data/db2/catalog` currently skips Db2 publication. Neither is
repaired or accepted by this review. The manager's proposed
[typed catalog evolution boundary](../../../decisions/0028-db2-typed-catalog-evolution.md)
retains one installed authority, requires versioned package/persistence/cell
contracts and explicit migration, and preserves old signature preimages and
legacy byte behavior. The proposal is not owner acceptance, schema freeze or
runtime completion. It fixes the direction before catalog mutation.

Before public integration, the manager declares
`DB2-1202.numeric-expression-type-surface`: export only the reviewed arithmetic
and result-combination metadata APIs through `lib.rs`, add
`tests/numeric_expression_types.rs` and one unique
`db2-numeric-expression-type-surface.toml` fragment, and update the provider
README and status/derived documentation. The independent kernels are sealed
as worker `8125ea3a` (manager `1623045f`) and worker `3cd84d9c`
(manager `477ad7f6`). Public regression checks cover explicit arithmetic
context, verified constant provenance, runtime obligations, ordered combination,
NULL/application context, owned outputs, original spans and bounded failures.
Run affected package/MSRV, format, statement-catalog, changelog/docs and exact
path seals; reuse dependency policy only with unchanged policy inputs.
No shared dispatcher, binder, typed catalog, evaluator or durable route changes.
Language elements have no standalone statement row; full-row recognition,
common/deferred freeze and licensed differential remain pending.

The reviewed public numeric-expression boundary now owns arithmetic contexts,
constant provenance, fixed diagnostics and runtime obligations, plus ordered
result-combination inputs, contexts, owned outputs and conversion traces. Six
public regressions exercise these boundaries without claiming parser, binder,
value conversion or execution support. A rustfmt-only reflow in the arithmetic
unit test accompanies integration; its production semantics are unchanged.
COALESCE context additionally uses `db2z_bif_coalesce.html` (12698 bytes,
`6f6c80fad7a56d48a50b66fee4c33b6d1ea5d29f4d24ed11fb314a36528941a4`)
under the same pinned Db2 13 baseline. Mixed decimal/binary-float arithmetic with
DECFLOAT remains unresolved: neither the result-combination table (which excludes
arithmetic) nor a scalar conversion-function default establishes its temporary
precision. FLOAT(n) aliases, character/graphic CCSID/collation and datetime-string
combination still require source-backed binding/context. No statement-catalog
numerator, durable route, shared signature preimage or parent milestone changes.

## Fifth-wave parser and freeze preparation

The manager declares four conflict-isolated CLI lanes after the fourth-wave
public boundary is sealed. Two syntax kernels remain private until manager
review and separate public integration. The other two lanes are read-only
preparation reports, not accepted schemas, coverage evidence or completed parent
milestones. All use gpt-6.1-sol/high, goals and default service tier with fast
mode disabled. No orchestration skill or nested workers are used.

| Lane | Exact ownership and boundaries |
|---|---|
| `DB2-1201.drop-common-object-syntax` | Only `src/drop_syntax.rs` and `db2-drop-common-object-syntax.toml`. SQL0072 common DROP TABLE/VIEW/INDEX/ALIAS structure, effective owned names, explicit supported alias-designator spelling and complete located bounds/errors. Unsupported DROP portfolios fail explicitly; no catalog validity, dependency deletion, package invalidation, authorization or execution. No full DROP-row credit. |
| `DB2-1201.rename-common-object-syntax` | Only `src/rename_syntax.rs` and `db2-rename-common-object-syntax.toml`. SQL0105 RENAME TABLE/INDEX source TO unqualified new identifier. Preserve source qualification and explicit destination spelling; current-server object applicability, alias resolution, dependencies, privileges and runtime mutation remain binding-owned. No statement dispatch or full-row credit. |
| Exact 174-row freeze preparation | Read-only proposed common/deferred obligation map for every pinned SQL and SQL PL row, independent of passing tests. Preserve every row, source locator and owner; identify partial recognition and mandatory remaining obligations. Missing sources remain explicit pending reviews. Do not change catalogs, denominator, IR, ledgers or status; the manager must review and integrate any normative freeze through existing authorities. |
| Typed catalog wire-design preparation | Read-only concrete actual-blob schema and codec/canonical-vector proposal under ADR-0028. Resolve metadata/default/cell/name/string-policy/version ownership and exact generated projections/gates before schema mutation. Do not install a second catalog, change package signatures, persist typed data or claim migration/acceptance. |

Pinned DROP `db2z_sql_drop.html` (320964 bytes,
`5f75fdde9c96290ba968fb9b2629ca73d9274de9d8b8bd1002e56c341cd446a0`)
and RENAME `db2z_sql_rename.html` (25184 bytes,
`ea6063fba847a91a891db54d4b1741f6dc379a7dbf61c8f15069867365f63984`)
were hash-verified locally under the Db2 13 baseline; ordinary reader remains
TOC-blocked. Invocation/authorization and catalog-dependent restrictions are
not parser proof. Parser checks cover valid/negative/source-location/effective
identifier and exact/one-beyond input/token/name bounds with owned outputs,
plus package/MSRV, format, policy, statement-catalog, changelog/docs and exact
path seals. Pure parser slices do not affect backend or durability routes.
Source review of the 174-row map is a required scoped freeze prerequisite, not
a whole-cache refresh/audit; no network or browser request is authorized.

The freeze preparation identified a prerequisite mismatch: frozen participant
v1 permits only the accepted CICS mapping and fixes Db2's dependency to a 0.13
binding, while new 0.12 mutating integration requires an owned early binding.
The manager's proposed [core participant evolution](../../../decisions/0029-db2-core-participant-evolution.md)
preserves v1 and chooses a shared versioned extension for the bounded local
Db2 core, with action/context applicability and minimum durable proof before
admission. No capability has been accepted and no guard or runtime route changed.
Full rollback and savepoint rollback cannot share a blanket CICS/IMS rejection:
SQL0026 prohibits COMMIT there; SQL0119 permits only savepoint rollback there.
Diagnostic/context sources and actual binding remain pending before mutation.

The typed wire-design review completed against `29eba935` with a clean,
read-only checkout and no executed gates. Its closed-shape, strict bounded
preflight, generated DTO/tag and independent-vector proposals inform the next
implementation, not an accepted schema. In particular, the manager does not
freeze a permanent `metadata_only` representation tag as the final installable
catalog contract. Private preparation is an admission stage; final catalog,
typed cells, defaults, string policies, package references and durable migration
must retain the actual requested end state. Non-NULL default constants and
checks cannot be passed off as validated SQL values or predicates today.

Before catalog schema mutation, the manager declares
`DB2-1202.exact-numeric-constant-values`: only
`src/numeric_constant_values.rs` and
`changes/unreleased/db2-exact-numeric-constant-values.toml` are worker-owned.
Construct exact INTEGER/BIGINT/DECIMAL literal values from the existing bounded
original-source classifier and reuse the foundation `DecimalValue` primitive;
do not create another decimal arithmetic engine. The manager adds that existing
workspace dependency and private module registration at the coordination base.
Outputs retain existing resolved types, original spans and owned values. This
is natural literal materialization, not assignment/default conversion, numeric
expression evaluation, persistence or canonical catalog identity. Float/DECFLOAT,
string, binary and datetime values and NULL cells remain explicit later work.
Source pins are the verified constants/datatype topics already declared above;
these language elements have no standalone statement row. Acceptance requires
fixed expected coefficients/scales and signed range/precision boundaries,
UTF-8/CRLF locations, bounds and all existing classifier rejections, owned output,
package/MSRV, formatting, dependency policy, catalog/changelog/docs and exact
path seals. No public route, backend or licensed credit changes.

The manager declares `DB2-1201.fifth-wave-syntax-surface` after reviewing and
integrating the sealed DROP and RENAME kernels. Exact ownership is `lib.rs`,
`tests/fifth_wave_syntax.rs`, the provider README, this status, the unique
`db2-fifth-wave-syntax-surface.toml` fragment and derived documentation manifest.
Expose the existing owned APIs without changing their implementations or adding
dispatch. Public regressions cover the four DROP object kinds, omitted versus
explicit alias designators, both RENAME kinds, explicit source qualification,
unqualified destination, effective identifiers, original UTF-8/CRLF spans,
owned output, rejection fences and configured input/token/name boundaries.
Binding, dependencies, privileges, mutation and full statement-row recognition
remain pending. The new direct encoding dependency requires current-candidate
dependency policy rather than reuse of pre-dependency receipts.

The DROP and RENAME private kernels were sealed independently, then integrated
without production edits. The manager's public surface passes six focused
regressions and the combined Db2 package: 267 unit tests and 40 integration
tests, zero failures or skips. Original source pins bind SQL0072 and SQL0105
under `ibm-db2-for-zos-13-2026-08-13`; the normal reader is TOC-blocked and the
matching raw archive remains the offline fallback. These observations do not
complete either official row or any execution/licensed obligation.

The read-only freeze report enumerates all 174 row IDs exactly once, with all
labels and source paths checked against the unchanged catalog. Its proposed
allocation is 87 rows with common portions (71 SQL and all 16 SQL PL), 84 wholly
deferred and three source-pending. These are planning counts, not gate numerators.
It is not an accepted exhaustive clause/context freeze: existing Conformance IR
has no Db2 bindings, ordinary scalar closure remains to be enumerated, and SQL
PL handler/atomic/context and singleton-assignment rules need explicit resolution.
The missing ALTER SEQUENCE, CREATE STOGROUP and SET CURRENT APPLICATION ENCODING
pins remain unavailable; no source refresh has been authorized. The user has
confirmed no licensed Db2 13 environment and requested continued implementation
with differential pending, without substituting local tests for oracle evidence.

The manager declares `DB2-1202.exact-numeric-value-surface` to expose the
reviewed, separately sealed literal materializer. Exact manager ownership is
`lib.rs`, `tests/numeric_constant_values.rs`, provider README, this status,
`changes/unreleased/db2-exact-numeric-value-surface.toml` and the derived docs
manifest. Public consumers receive an opaque verified type/span/value pairing;
DECIMAL reuses the existing foundation `DecimalValue` without an alternate
arithmetic engine. Public tests cover natural INTEGER/BIGINT boundaries,
independently expected DECIMAL coefficients/precision/scale, signed zero,
original UTF-8/CRLF locations, owned output and inherited limits/rejections.
Assignment/default conversions, expression evaluation, NULL cells, all other
literal families, catalog wire/canonical identities and durable effects remain
distinct implementation work; no full-row or licensed credit is granted.

Two read-only followups run independently of the numeric implementation: the
existing Conformance IR/driver admission design for partial SQL0072/SQL0105,
and native SQL PL ATOMIC, handler/RESIGNAL and transaction-context resolution.
They may propose concrete paths and independently sourced obligations, but
cannot self-accept reviewed rules, mutate the IR or change gate applicability.

The reviewed private literal kernel is integrated without production edits.
Its public surface passes six focused tests and the combined Db2 package:
278 unit tests and 46 integration tests, zero failures or skips. Verified
constants/datatype language-element pins have no standalone statement row;
publication review and local tests do not discharge licensed obligations.

The next declared worker slice is
`DB2-1202.exact-numeric-literal-assignment`, owning only the existing
`src/numeric_constant_values.rs` value owner and
`changes/unreleased/db2-exact-numeric-literal-assignment.toml`. Add a bounded
conversion from the opaque materialized INTEGER/BIGINT/DECIMAL literal to an
existing resolved SMALLINT/INTEGER/BIGINT/DECIMAL target. Retain the matching
target type, exact value and source span in an owned opaque result; use the
existing assignment compatibility and `DecimalValue` authorities. Preserve
whole parts, truncate decimal fractional digits toward zero, scale up exactly
only within target precision, and report discarded fractional digits separately
from invented SQLCA warnings or SQLCODE. Integer-to-decimal temporary attributes
must retain the pinned 11,0/19,0 rule. Overflow fails before an output is admitted.
Do not broaden into floating/DECFLOAT/NULL/string/datetime, arbitrary raw-value
constructors, default-expression legality, wire/cell/catalog/persistence or
general arithmetic evaluation. These remain required common implementation,
not reclassified as deferred because this slice is narrower. The exact numeric
assignment pin is `db2z_numericassignments.html`, 18430 bytes,
`3f6ba8a8290190c2e36590aa348fb816fff6a166c7483b97d1b51cc53bbd8302`;
compatibility uses `db2z_assignmentandcomparison.html`, 40605 bytes,
`2cc975449a25d1d825cbd8a6551f5ea9c6dfc6ed5dc9956643f0e1dc9948af8a`.
Both match the Db2 13 archive; retained files are absent and normal reading
is TOC-blocked. Independent conversion/range/scale/truncation/location/ownership
regressions, package/MSRV and required policy/docs/seal checks precede its commit.

The IR followup found that adding only passing syntax obligations could falsely
pass a whole row gate. The manager therefore declares
`DB2-1201.syntax-harness-foundation`: a real DROP/RENAME driver, independent
typed fixtures and strict observation comparators in the existing conformance
tooling, with zero official Db2 bindings or credit until maintainer acceptance.
Worker ownership is only `src/db2_syntax.rs`, `src/db2_syntax_fixtures.rs` and
module/export registration in the conformance crate's `lib.rs`, plus
`changes/unreleased/db2-syntax-harness-foundation.toml`. No xtask/framework/spec,
schema, ledger or reviewed-rule registry mutation is allowed. Handlers install
only when actual compiled references exist; unknown/missing/extra handlers and
malformed output fail closed. Drivers call the public parsers with source and
limits, not row IDs; comparators use independent fixed expectations, not a
product-computed success flag. Tests exercise generic-success/wrong-field/span/
route, missing/duplicate output and bounds mutants. Rejection of a legal deferred
form records a product limitation, never official recognition evidence. Existing
six-gate applicability, 174 rows and licensed-pending disposition stay unchanged.

The manager declares `DB2-1202.exact-numeric-assignment-surface` to expose the
separately sealed, reviewed literal-assignment kernel without production edits.
Exact ownership is `lib.rs`, `tests/numeric_constant_assignment.rs`, provider
README, this status, `changes/unreleased/db2-exact-numeric-assignment-surface.toml`
and the derived documentation manifest. Public fixed-vector regressions cover
signed target boundaries after fractional truncation, DECIMAL precision/scale
31, overflow before expansion, independent fractional-loss observations,
11,0/19,0 temporary attributes, nullable target metadata, compatibility versus
unsupported conversions and owned original UTF-8/CRLF spans. The prerequisite
kernel's 20 focused tests, 287 unit and 46 integration tests passed on its own
base; these are not relabeled as manager-candidate evidence. Public integration
requires current package/MSRV and policy/catalog/changelog/docs/seal checks.
The numeric-assignment and assignment/comparison pins above remain authority;
they are language elements without standalone catalog rows. Binder/default
legality, other ordinary value families, execution and licensed differential
remain pending, with zero official recognition or row/gate credit added.

The public assignment surface passes six focused tests and the current combined
Db2 package: 287 unit tests and 52 integration tests, zero failures or skips.
Rust 1.95 all-target/all-feature checking also passes. The integrated private
value-owner blob is unchanged from the worker's sealed commit; public consumers
receive only existing proof constructors and read-only metadata. These local
results do not discharge default applicability or oracle obligations.

The manager declares `DB2-1201.located-column-default-intents` before typed
catalog/default binding. The current CREATE TABLE parser cannot represent the
valid operand-less DEFAULT form and discards individual operand locations.
Worker ownership is exactly `statement/create_table/mod.rs`, `columns.rs`,
`tests.rs`, `tests/create_table_defaults.rs` and
`changes/unreleased/db2-located-column-default-intents.toml`. Preserve the
existing column/default types, with explicit absence versus present type-default
versus literal/NULL intent. `Db2ColumnDefault::value()` changes to
`Option<&Db2Literal>`: `None` means a present operand-less default clause, not
an omitted clause or implicit NULL. This is an intentional development source-API
adjustment; migrate existing direct consumers rather than inventing a literal
or panicking for the new form. Do not change accepted durable/wire contracts.

Retain complete original clause and operand spans; numeric defaults additionally
retain sign and number-token spans so trivia between them cannot be replaced by
a fabricated contiguous source span. Preserve DEFAULT/WITH DEFAULT spelling
and inherited literal escape text. Recognize valid operand-less clauses before
an element boundary or the next admitted column clause, enforce duplicate and
NOT NULL/explicit NULL conflicts, and reject malformed or still-unsupported
default expressions/registers. Do not invent a zero, current timestamp, implicit
NULL, typed value, conversion result or catalog identity in the parser.
Focused/public regressions cover all three intents, signed/trivia operands,
quoted/UTF-8/CRLF locations, ownership, clause order, negative forms and exact
resource bounds; package/MSRV, required policy/docs/seal checks precede its commit.

Source SQL0050 `db2z_sql_createtable.html`, 874327 bytes,
`104cc7fd0f43e804819da99c18887de60983cad8fa78b7d550ffaf63dfd299d6`,
defines operand-less type defaults, assignment-compatible constants, nullable
implicit DEFAULT NULL and NOT NULL without a default. The corresponding original
description/table and diagram are source authority; syntax alone does not prove
their semantic values or insertion effects. The retained file is absent; matching
archive HTML was read offline under the Db2 13 baseline, with normal reader
TOC-blocked. The typed catalog, default resolver and all ordinary value families
remain required implementation, not narrowed to these initial numeric slices.

The reviewed Stage A syntax-harness commit is integrated without code changes.
Its nine focused tests compare 73 fixed authored cases and comparator mutants
through the actual public DROP/RENAME APIs. On the manager candidate before
default integration, conformance package tests pass 269 unit and 19 integration
tests after supplying the existing clean, pinned local CardDemo corpus at
`59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e`; the worker's four missing-environment
failures are retained in their original receipts. Two existing PostgreSQL/manual
Zowe tests are ignored and earn no credit. No official Db2 binding or driver
installation was added. Full selected global CI remains pending, including
unchanged contract API-doc, dependency Clippy and architecture/source blockers.

Located CREATE TABLE default intents are also integrated without production
edits. The combined manager Db2 candidate passes six focused public regressions,
290 unit tests and 58 integration tests, zero failures/skips, plus Rust 1.95
all-target/all-feature checking. The intentional optional `value()` adjustment
preserves omitted/type-default/explicit NULL distinctions and original clause,
operand, sign and number-token locations; no inferred value or effect is claimed.
Its source remains SQL0050 under the pinned Db2 13 baseline. The pure parser and
ordinary test harness do not complete SQL0050, SQL0072, SQL0105 or a parent milestone.

The manager declares `DB2-1202.literal-string-values` as the next ordinary value
prerequisite, owning `type_system/mod.rs` only for child-module registration,
new `type_system/string_constants.rs`, `syntax.rs` only for first-class BX
binary-token admission and its focused regressions, and the unique
`changes/unreleased/db2-literal-string-values.toml`. Use the existing type owner,
lexer, location/limit shapes and bounded resolved-type authority. A located
opaque proof must distinguish Unicode UTF-8 character text/hex character forms
from BX binary forms, preserve empty versus NULL, decode escaped delimiters or
hex once, retain the matching natural VARCHAR/VARBINARY length and NOT NULL,
and own original source spans. Natural empty constants may have length zero;
column/type-syntax length minima must not be relaxed. The new Binary string-kind
variant is an intentional development API extension, not an accepted wire change;
existing lexer forms, spans and numeric fences remain unchanged. Unicode character metadata
must explicitly retain CCSID 1208/MIXED rather than infer a host encoding; binary
constants have no invented CCSID. No arbitrary raw type/value constructor or
default/assignment/cell/wire/execution claim is admitted. Unsupported encoding,
delimiter, graphic and other ordinary families remain explicit implementation
pending, not silently converted or normatively deferred.

Review constantsintro (17915 bytes, bf0cb79eac0636348209b6919c4f4ee680f1c3d2ada9cab186dddfdd39a13fa8),
characterstokens (14271 bytes, ed63dd289fc68abe18ec5863756239f79d93941cd525968ba48f962d1299d4ae),
charstrings (17888 bytes, 54c3f8ec1620479ffb002788e198c788050d8616e1ee4fbac431fb45bb59d835)
and binarystringsintro (3946 bytes, 69af1d1645cf137f58d372fe93589e1f299a76937c4e9d1284ad1aa6e2643036)
under `SSEPEK_13.0.0/sqlref/src/tpc/`, plus actual relevant encoding context.
Retained selected files are absent; matching archive identities are verified,
normal reading is TOC-blocked. Source presence grants no execution credit.
Tests require independently fixed decoded bytes, empty/NULL distinctions,
unescaped-length/type proofs, Unicode/CRLF endpoints, malformed and incompatible
forms, configured/compiled source/token/literal bounds and original-source
ceilings before allocation. Private source verification must not lex unrelated
source outside the selected constant or change numeric span authority. Full
package/MSRV, required policy and exact four-path sealing precede its commit.

The manager separately declares `DB2-1202.db2-clippy-readiness` to repair the
observed scoped Db2 lint failures without changing semantics or suppressing lints.
Ownership is `arithmetic_types.rs`, `create_view_syntax.rs`, `update_syntax.rs`,
`numeric_constant_types.rs`, `numeric_constant_values.rs` and unique
`changes/unreleased/db2-clippy-readiness.toml`. Only the existing doc paragraph,
range-loop/match and test-initializer diagnostics are in scope. Preserve original
source transfer, bounds, diagnostics, opaque proofs and all regressions; no parser,
type rule, baseline, accepted contract, dependency or gate criterion changes.
Run affected regressions/package/MSRV and strict package Clippy with `--no-deps`;
global dependency Clippy failures remain distinct and are not repeated unchanged.
The first scoped run exposed `drop_non_drop` in the existing public numeric
ownership test. Ownership is expanded only to
`tests/numeric_constant_values.rs` for returning a clone from the source/result
lexical scope, preserving all independent assertions without lint suppression.
Seal all seven exact paths and clean the assigned checkout after verification.

The manager declares `DB2-1202.literal-string-surface` to expose the separately
sealed, fully reviewed natural string-value proof without production changes.
Exact ownership is provider `lib.rs`, `tests/string_constants.rs`, README, this
status, `changes/unreleased/db2-literal-string-surface.toml` and the derived
documentation manifest. Public fixed-vector tests cover decoded apostrophes,
Unicode UTF-8 and X character bytes, first-class BX binary bytes, empty NOT NULL
natural length zero versus unchanged column minima, explicit MIXED DATA case
rules, complete original UTF-8/CRLF locations, ownership, forged spans,
malformed/pending forms and independent source/spelling/token/value/publication
bounds. Existing type, lexer and proof owners remain unchanged.

The constants, character, binary, token and delimiter pins above are the source
authority; these language elements have no standalone catalog rows. The additional
delimiter pin is `db2z_apostrophesandquotesindelims.html`, 5549 bytes,
`94746a42021315234baea45342774882f497fcaa3f8360cdd837c0673d414700`.
Matching archive bytes were read locally; retained files are absent and normal
reading remains TOC-blocked. Public integration requires current focused/package,
MSRV, formatting, policy/catalog/changelog/docs and exact-path seal checks.
No assignment/default applicability, other ordinary values, typed cells/catalog,
execution, official row/gate or licensed differential credit is claimed.

The natural string kernel and seven-path lint-readiness feature are integrated
from separate sealed commits. Public string integration passes six independent
tests and the combined Db2 package: 304 unit plus 64 integration tests, zero
failures/skips. Rust 1.95 all-target/all-feature checking and strict package
Clippy with `--no-deps` pass. Two newly observed string-owner Clippy expressions
were repaired equivalently in a separate focused commit, without suppressions;
all package tests and MSRV checking pass after that repair. Initial failed lint
receipts retain their original identity. Global dependency Clippy, contract
API-doc and architecture/source blockers are not discharged by this scoped pass.
The pinned catalog denominator, official gate numerators and licensed-pending
disposition remain unchanged. Default/assignment binding and actual typed catalog
admission remain subsequent required implementation, not a metadata-only end state.

The catalog followup rejects the earlier tables-only `metadata_only` proposal as
the final catalog@2 destination. Preparation must preserve all ordinary value,
default, constraint, relation/index/routine/package and privilege requirements;
final schema/tag/IR, package and durable-version acceptance remain pending.
Current exact numeric proofs and located default intents permit real default
admission work, but source proofs, assigned values and decoded durable-cell proofs
must stay distinct. No additional official Db2 bindings or credit are added.

The manager declares `DB2-1202.located-numeric-operands`, parent DB2-1202, as the
source-proof prerequisite for signed defaults separated by trivia. Exact worker
ownership is `numeric_constant_types.rs`, `numeric_constant_values.rs` and
`changes/unreleased/db2-located-numeric-operands.toml`. Extend existing classifier
and materializer authorities to original operand/sign/number-token locations;
validate all byte and line/column endpoints, source bounds, actual sign/number
tokens and intervening trivia before value accumulation. Reuse the existing lexer
without lexing unrelated surrounding SQL, fabricating normalized source, accepting
extra expressions or duplicating numeric type/range/coefficient rules. Existing
adjacent-spelling APIs, errors/fences and regressions remain intact. Returned
opaque natural type/value proofs retain the original combined operand span.
Manager owns public exports and default binding. Focused independent vectors,
malformed/forged/provenance/trivia/boundary tests, package/MSRV and applicable
policy/catalog/changelog/docs/seal checks precede the private feature commit.
Only new private API reachability warnings may await public integration;
diagnosed owned semantic/lint errors must be repaired without suppression.

The manager also declares `DB2-1202.string-storage-assignment`, parent DB2-1202.
Exact worker ownership is `type_system/string_constants.rs` only for child-module
registration, new `type_system/string_constants/storage_assignment.rs` and
`changes/unreleased/db2-string-storage-assignment.toml`. Consume only existing
opaque literal proofs and validated resolved target types under caller-explicit
Unicode UTF-8/MIXED character or binary target context. Same-encoding CHAR/VARCHAR
storage may remove only excess trailing ASCII blanks and pads fixed targets with
blanks; BINARY/VARBINARY rejects every oversize value and pads fixed targets with
zero bytes. Preserve target length/nullability, original source span and explicit
encoding; reject natural length-zero types used as declared targets. No arbitrary
raw value/type constructor, CCSID inference, binary/BitData coercion, NULL value,
retrieval truncation, host/SQLCA, default applicability or persistence is admitted.
Other encoding/conversion/graphic/ordinary targets remain explicitly pending.
Bounds and immutable-owned outputs precede allocation; use the current compatible
type authority rather than a second conversion engine. Independent fixed padding,
truncation/error/UTF-8/NUL/empty/ownership/context/target-limit tests plus focused
package/MSRV and applicable policy/docs/exact-path sealing precede its commit.
Manager owns root exports and independent public integration tests.

These nonmutating proof/conversion slices have no backend/restart/authorization
claim, no standalone language-element row and no SQL dispatch. SQL0050 default
context remains pending binding, while its source delegates constants to assignment
rules. String storage uses `db2z_stringassignmentintro.html`, 12061 bytes,
`a72f1dd0d80391b8ef20100313bfca498dc881b9881fbb49beb9177263e7f46e`,
with constants/character/binary and compatibility pins above. Main read the
hash-verified archive source locally after retained-path checks; normal reading
is TOC-blocked. No refresh or licensed credit is inferred. Exact 174-row obligation
freeze remains required before broad DB2-1203 execution.

The scalar followup inventories existing CP037, fixed-decimal and decimal-floating
primitives separately from COBOL-owned policies and IEEE floating paths. It leaves
HFP layout/tie/extrema/diagnostic closure, mixed DECFLOAT arithmetic precision,
string provenance/CCSID combinations and disputed datetime/zone contexts open.
No ordinary family is deferred merely because unsupported. Main read the complete
external review; its proposed vectors were not executed or maintainer-accepted.

The manager declares `DB2-1202.floating-type-aliases`, parent DB2-1202, for actual
source-backed type resolution, independently of unavailable HFP value semantics.
Worker ownership is `type_system/mod.rs` only for FLOAT resolution/comments,
`type_system/tests.rs`, `result_combination_types.rs` only comments and affected
tests, new `tests/floating_type_aliases.rs`, and
`changes/unreleased/db2-floating-type-aliases.toml`. Resolve FLOAT(1..21) to the
existing REAL shape and FLOAT(22..53), including omitted precision 53, to DOUBLE.
Preserve syntax provenance in the AST and resolved nullability; reject zero/54,
extra arguments and unsupported time-zone/attribute contexts as before. This is
an intentional development resolver behavior adjustment, not a wire/schema or
floating storage claim. Retain the existing public Float variant and explicit
unresolved-shape guard rather than removing APIs or admitting raw fabricated types.

Independent public tests verify all precision values 1..53, defaults/boundaries,
parser-to-resolver ownership, canonical alias equivalence and existing assignment,
comparison/arithmetic/result-combination consumers. Only affected obsolete alias
expectations migrate; unrelated production rules, coefficient owners and source
fences remain unchanged. Focused/package/MSRV, strict scoped Clippy, formatting,
policy/catalog/changelog/docs and exact five-path sealing precede this commit.
The SQL0050 CREATE TABLE pin above, complete floating type rule lines 220..227,
provides alias authority; main verified and read it locally. HFP values, IEEE host
conversion, floating literals/evaluation, exact SQL diagnostics, typed catalog and
licensed differential remain required pending implementation and evidence.

The manager declares `DB2-1202.string-storage-surface` to expose the separately
sealed storage-assignment kernel without changing its production semantics.
Exact ownership is provider `lib.rs`, new `tests/string_storage_assignment.rs`,
README, this status, `changes/unreleased/db2-string-storage-surface.toml` and
the derived documentation manifest. Independent public tests cover fixed/varying
character and binary targets, empty nonnull values, byte-exact UTF-8 boundaries,
NUL, X/BX family separation, excess-blank versus binary-overflow rules, explicit
target encoding, nullable metadata, positive declared lengths, configured output
budgets and immutable-owned provenance after source/target scope ends. Root
exports make the existing proof authority reachable; no second converter is added.

The pinned string-assignment topic cited above supplies storage rules, not
retrieval-warning or SQLCA authority. Focused/package/MSRV, strict scoped Clippy,
formatting, dependency policy/catalog/changelog/docs and exact-path sealing must
pass before this public feature commit. Defaults, other encodings/ordinary
families, typed durable cells/catalog, executor and licensed differentials remain
pending; source review and local proofs grant no official row/gate credit.

The manager declares `DB2-1202.located-numeric-surface` for the separately sealed
numeric-operand prerequisite. Exact ownership is root `lib.rs` exports, new
`tests/located_numeric_operands.rs`, README, this status, unique
`changes/unreleased/db2-located-numeric-surface.toml` and derived documentation
manifest. Independent public tests establish signed natural type boundaries,
spelling-derived decimal precision/scale and exact coefficient, original UTF-8/
CRLF ownership, existing exact target assignment, forged component/trivia errors,
both original-source budgets and token/literal/count limits. Parser DEFAULT
locations supplement fixed expected vectors without proving default legality.
The original adjacent APIs and the lexer's trailing-point/floating fences stay
unchanged. Root exports must discharge both private reachability warnings;
focused/package/MSRV, strict scoped Clippy and applicable policy/docs/exact-path
checks precede the public feature commit. No row/gate or licensed credit is added.

Constants/token/datatype pins above supply numeric rules; SQL comments additionally
uses `SSEPEK_13.0.0/sqlref/src/tpc/db2z_sqlcomments.html`, 6508 bytes,
`c8e3c16d1152a165b0c2c7b71ddfad4ca8032f910f2c92f0eca5484ce5b68ca2`.
Main verified/read all 43 archive lines after retained-path checks; generic SQL
trivia support is not host-language/tool-specific comment applicability. Actual
NULL/type-default/constant-default admission, ordinary value closure, typed
catalogs and licensed evidence remain subsequent required implementation.

The string storage and located numeric prerequisites are integrated as separately
sealed private/public features. Public integration adds six independent tests
for each and discharges the private reachability warnings through exports. The
last numeric public candidate passes 325 unit plus 76 integration tests, strict
scoped Clippy, both Rust checks and applicable policy/catalog/changelog/docs/seal
checks, with clean tree and absent target. Those receipts retain their exact
pre-FLOAT candidate identity, not a claim for a later combined candidate.

The FLOAT feature is also integrated from its own final five-path seal. Main
review found missing alias-specific CASE/COALESCE tests; the same worker added
three fixed-context tests and resealed its unpublished feature, preserving the
initial receipts separately. Main read the full final diff/test additions and
handoff; production remains only source-backed resolve_float normalization.
The new tests retain omitted/present/NULL ELSE, COALESCE nullable/nonnull/untyped
NULL, ordered owned results and invalid/all-untyped context fences. They establish
metadata behavior, not CASE/COALESCE execution or floating values. HFP conversion,
other ordinary values, default binding, typed catalogs and full conformance
remain required pending work; licensed differential remains pending as requested.

The combined FLOAT integration candidate passes ten focused alias tests and
325 unit plus 86 integration tests, with zero failures/skips. Both Rust checks,
strict scoped Clippy, formatting, catalog/changelog/docs and committed private
seal checks pass. Unchanged dependency-policy inputs reuse the prior numeric
candidate receipt without relabeling its execution identity. These local results
do not discharge global blockers, source gaps, reviewed-rule acceptance or any
official Db2/ licensed gate. The final documentation reconciliation changes no
tested production bytes; its generated hashes are checked before commit/cleanup.

## Next scalar/default implementation cohort

The manager declares three independent DB2-1202 prerequisite slices from the
combined scalar candidate. Each has one conflict-isolated CLI worker, its own
feature seal and unique fragment; manager owns public integration. Workers use
gpt-6.1-sol high, goals/bypass and fast mode disabled. The latest fetched
`origin/main` is already an ancestor; no integration merge is needed.

`DB2-1202.column-default-binding` owns
`src/statement/create_table/mod.rs` (child registration only), new
`src/statement/create_table/default_binding.rs` and
`changes/unreleased/db2-column-default-binding.toml`. Paths under `src/` are in
the existing Db2 provider. Its bounded entry reparses the original CREATE TABLE
source, resolves each column through the existing type owner, and distinguishes
omitted nullable NULL, omitted NOT NULL with no default, explicit NULL, bare
operand-less DEFAULT producers, and assigned numeric/string constant proofs.
Numeric and string conversion reuse the existing opaque authorities; character
default UTF-8 length is checked against 1536 before target trimming/padding.
Current date/time/timestamp defaults remain insertion-time producers, not
CREATE-time clock reads or fabricated literal proofs. No constraints, names,
privileges, installed catalog or execution are implied by this column-only
binding result. SQL0050 is partial; its actual default obligations remain in the
174-row denominator. The contradictory WITH DEFAULT prose is explicitly pending
at binding, without changing its syntactic representation or guessing meaning.

`DB2-1202.finite-decfloat-assignment` owns existing
`src/numeric_constant_values.rs` (child registration only), new
`src/numeric_constant_values/decfloat_assignment.rs`, provider `Cargo.toml`,
`Cargo.lock`, new `docs/delivery/subsystems/db2/decfloat-primitive-decision.md`,
and `changes/unreleased/db2-finite-decfloat-assignment.toml`. It converts only
existing opaque INTEGER/BIGINT/DECIMAL constant proofs to resolved DECFLOAT(16/34),
with caller-explicit owned rounding mode/context and owned finite coefficient,
exponent and inexact observation. The existing pinned dec 0.4.11 is a candidate
standards primitive behind the adapter, not Db2 policy. Its dependency decision
must record exact license/MSRV/features/transitives/failure/determinism/removal
before production adoption; required supply-chain checks must pass. No external
types/bytes enter public or durable identity. Exponent/special SQL literals, HFP,
general scalar conversion, arithmetic, SQLCA and catalog cells remain required
pending work. This language-element slice has no standalone catalog row.

`DB2-1202.search-condition-truth` owns existing `src/type_system/mod.rs` (child
registration only), new `src/type_system/truth.rs`, and
`changes/unreleased/db2-search-condition-truth.toml`. A closed owned predicate
truth domain implements the pinned nine AND/OR pairs, three NOT outcomes and
TRUE-only row qualification. NULL predicates never produce UNKNOWN. This is not
a SQL Boolean scalar type, parser extension or expression evaluator; no statement
execution, comparison, short-circuit order, filtering or durable row claim follows.
This language-element slice has no standalone catalog row.

All three slices are deterministic non-mutating kernels: memory-only unit tests
exercise values/plans, not backend authorization, UOW, restart or recovery routes.
Those routes are unchanged, not waived for later mutating integration. Acceptance
requires independently fixed positive/negative/boundary vectors, owned lifetimes,
applicable limits/provenance, affected package tests/checks, Rust 1.95, formatting,
dependency/catalog/changelog/docs policy, exact-path seal and target cleanup.
Private reachability warnings alone may await manager exports; all other new
diagnostics must be repaired. Public selected-route tests and strict scoped
Clippy are required at each subsequent public integration.

Pinned baseline remains `ibm-db2-for-zos-13-2026-08-13`, product SSEPEK_13.0.0.
Main verified/read SQL0050 DEFAULT lines 380–524 and numeric assignments (99
lines), plus complete CURRENT DECFLOAT ROUNDING MODE (62), DECFLOAT rounding
mode (8), DECFLOAT function (71), search conditions (101), NULL predicate (26).
Existing constants/datatype/string assignment pins remain unchanged. Relevant
topic paths are `sqlref/src/tpc/db2z_currentdecfloatroundingmode.html` (6515 bytes,
`f560f8647556aa0d8b8c56f96cbcfae7cbab9edb694a5ecc24f8382568c86932`),
`db2z_decfloatroundmode.html` (1723,
`edeb3384e9752fa02caf746884d51e8c47e5765cb526481c8f0d4cd9d81122cf`),
`db2z_bif_decfloat.html` (21071,
`22ef060c92f35e04190a11ac5403b0a196dbf5961b36db0f3524987dacc0d74b`),
`db2z_searchconditionssql.html` (27571,
`79fafaf79779ef1f7b947502d99efd2ab7d87bf1046ff65602076b8fbf4e5627`), and
`db2z_nullpredicate.html` (8909,
`c1ca9abcb98036bf0666b6710fd37c26c1499fbd8e787b670fa67a8c5c02ef9d`).
Normal readers remain TOC-blocked; retained paths were checked first, then exact
raw archive bytes verified and parsed locally. No refresh or publication-body
commit is authorized. Source review and local kernels grant zero official gate
or licensed credit; exact common/deferred freeze and licensed differential stay
pending. The user confirmed implementation should continue without an oracle.

The manager additionally declares `DB2-1202.search-condition-truth-surface`:
root `lib.rs` exports, new `tests/search_condition_truth.rs`, provider README,
this status, unique `changes/unreleased/db2-search-condition-truth-surface.toml`
and derived documentation manifest. It exposes the sealed predicate domain and
tests independent fixed tables/NULL outcomes, UNKNOWN-preserving compositions,
TRUE-only qualification and owned const results through the public crate route.
No evaluator, Boolean scalar, SQL null-storage or backend mutation is added.
Acceptance is focused/public and package tests, Rust checks including 1.95,
strict scoped Clippy, formatting and applicable policy/catalog/changelog/docs,
exact-path sealing and target cleanup. Root export must discharge the private
reachability warning. Parser/executor integration and official/licensed gates
remain pending; no accepted mutable or durable contract changes.

The truth prerequisite is integrated from its three-path private seal; main read
the complete 218-line module, all fixed tests, fragment and full worker handoff.
Private checks passed eight focused tests, 333 unit plus 86 integration tests,
both Rust checks and policy/catalog/docs/spec; clean-candidate evidence-fast and
committed seal checks also passed. Their original worker identity is retained.
The manager public integration adds four independent tests and only a root
export in production. It passes 333 unit plus 90 integration tests, Rust 1.98/1.95,
strict scoped Clippy, formatting, catalog/changelog/docs and unchanged private
seal checks. Dependency/advisory/license/source policy inputs are unchanged;
the private worker's actual policy receipt is reused without relabeling its
execution identity. These local proofs add no official recognition/execution
or licensed numerator, and no evaluator consumes the predicate domain yet.

The manager declares `DB2-1202.column-default-binding-surface`, parent DB2-1202,
for the separately sealed column-only binder. Exact ownership is existing
`src/statement.rs` child facade and root `lib.rs` exports, new
`tests/column_default_binding.rs`, provider README, this status, unique
`changes/unreleased/db2-column-default-binding-surface.toml` and generated
documentation manifest. Main read all 1254 module lines, the 195-line handoff,
registration/fragment and verified the final source blob. Independent public
vectors cover four clause intents, exact target values, UTF-8/CRLF signed
provenance and owned lifetimes, character/binary assignment, pre-trim 1536/1537
bytes, declared source/output budgets, producer precision/zone and explicit
pending/type/NULL-conflict errors. Backend/catalog/constraint/name/privilege
binding and runtime producers are not added; SQL0050 and licensed gates remain
partial/pending. Acceptance requires focused/package/Rust/MSRV checks, strict
scoped Clippy with all private reachability warnings discharged, formatting,
applicable unchanged-input policy/catalog/changelog/docs, both private/public
exact seals and cleanup. No mutable/durable/schema contract changes.

The column-default public candidate passes six focused tests and 347 unit plus
96 integration tests, with no failures/skips. Rust 1.98/1.95, strict scoped Clippy,
formatting, catalog/changelog/docs and committed private-seal checks pass; the
public exports discharge the private warnings. Unchanged dependency-policy
inputs retain the private worker's actual passing receipt identity. Main's public
tests do not use binder results to generate expected values or locations. The
tested production bytes are unchanged by the final documentation reconciliation;
generated hashes are checked again before the public feature seal and cleanup.

## Dependency gate

All three release commits are ancestors of the candidate.

| Dependency | Accepted identity | Disposition and retained authority |
|---|---|---|
| 0.2.0 | commit `e8dfa89583d866a365f496297d50aeb602e468bf`; tree `b60b14435b7466e937f58391297a62a38fb86ee2`; annotated tag `mainframe-env-v0.2.0` | Released catalog/package/handler authority; `docs/releases/0.2.md` and `conformance/0.2/` retained. No later Db2 semantic numerator is inferred. |
| 0.4.0 | commit `4a50a4e66f08b9cb5d293fb276cfbd52424b07fc`; tree `92ba4ce0855c02b6157d08fedac2cae51977a972`; source `sha256:143050b6cc13f22dd23e1edba1a4f5a872dde835be9bb7c7ce02a2479ad903ca` | Released COBOL host ABI; pass-with-licensed-differential-pending, Enterprise COBOL 0/153. |
| 0.5.0 | commit `bd5e8ecd211b7da4f3e18dfcfc807352d0ebd2e8`; tree `7e89e90c372a8bb1ca63a7b06c3c744e1a85e8d9`; source `sha256:67faf4b40e2e8c1c619a9564188d4ffe26efff5354e2c60b7032035e37d8b4f6` | Released RACF/SAF authority; pass-with-licensed-differential-pending, RACF/SAF 0/48. |

The pending licensed obligations of 0.4 and 0.5 remain unchanged. They do not
block consumption of the recorded implementation baselines and do not grant a
licensed disposition to 0.12.

## Current work package

The recovered [reuse spike](reuse-spike.md) records 49 representative
Db2 forms and a conditional parser-reuse proposal. The archived 48-case study
used different versions and rejected both as direct dependencies. Neither study
settles current adoption. No parser dependency was added.

The recovered official catalog derives a typed 174-row identity/descriptor projection
directly from the frozen 0.2 catalog and rejects denominator, row, locator, and
generated-output drift. It includes pinned topic paths and hashes. Remaining stages define exact recognition obligations,
source-review state, and the common/deferred disposition without granting
execution credit. The slice cannot be sealed until every row has pinned source
review.

The recovered `DB2-1201.lexer` slice supplies owned token kinds, byte and
line/column spans, a peekable cursor, bounded input/token/nesting limits, and
explicit lexical diagnostics. It ports the intent of lost commit `3cf6858c`
with a hand-written scanner; no parser dependency or execution route was added.
ASCII ordinary and delimited identifiers, character/graphic/hex literal shapes,
comments, host-variable and parameter markers, operators, and terminators are
lexed. The scanner rejects unsupported forms before a later parser or executor
could receive them. Float and decfloat constants explicitly fail with a
source-pending diagnostic naming #350. Integer and decimal digit/period forms
are tokenized only; their numeric semantics remain source-pending on #350.

The recovered `DB2-1201.ast-primitives` slice ports lost commit `ee1079c8`:
bounded owned identifiers, qualified names, static host and indicator references,
built-in and distinct type syntax, literals, operators, and an append-only
acyclic expression arena. Arena nodes own D2 byte and line/column spans.
Constructors reject invalid limits, names, type arguments, literals, lists,
references, node counts, and depth. Expression parsing, name resolution, type
compatibility, execution, and SQLCA remained pending at the AST slice; the
common type compatibility subset is recovered below.

The recovered `DB2-1201.transaction-syntax` slice ports lost commit `0ddebbd3`:
an owned cursor parser produces typed COMMIT, ROLLBACK, and SAVEPOINT nodes.
It accepts optional WORK, unit or named/unnamed savepoint rollback, and
SAVEPOINT with optional UNIQUE directly after the name, the required ON
ROLLBACK RETAIN CURSORS, and optional ON ROLLBACK RETAIN LOCKS (the two retain
clauses in either order). It rejects a missing RETAIN CURSORS, a misplaced
UNIQUE, duplicate clauses,
malformed operands, savepoint names beginning with SYS, trailing tokens,
multiple statements, and other statement families with bounded located
diagnostics. D3 byte and line/column spans are retained. RELEASE SAVEPOINT
was not in the lost commit and remains pending. No transaction execution route
is connected.

The recovered `DB2-1201.host-reference` slice ports the host-identifier
distinction from lost commit `999718c6`: host identifiers are distinct from SQL
identifiers and preserve host-language spelling, including COBOL hyphens. This
port also adds a bounded parser for the pinned reference diagram. It recognizes
a required colon-prefixed host variable with an optional colon-prefixed indicator.
The `INDICATOR` keyword is allowed only before the indicator. It rejects missing
or misplaced parts and trailing tokens. Host structures, host-language binding,
and execution remain pending. This slice adds no conformance or licensed credit.

The recovered `DB2-1201.dynamic-syntax` slice ports the common static-host
subset of lost commit `6c47ed20`: PREPARE owns a statement name, optional SQLDA
and naming mode, optional attribute host reference and a required non-indicated
host source; EXECUTE owns a statement name with optional bounded host-reference
list or SQLDA; EXECUTE IMMEDIATE owns one non-indicated host source. PL/I string
expressions, SQL PL variables and array elements, multi-row source buffers,
forbidden indicators, malformed clauses, and extra statements fail explicitly.
Rows SQL 0075, SQL 0076, and SQL 0101 remain partial with no whole-row
recognition, conformance, differential, or licensed credit. Dynamic execution
and remaining parser families remain pending.

The recovered `DB2-1201.expression-parser` slice ports lost commit `06bd397f`:
an owned arena parser for common literals, host and column references, parameter
markers, function calls, CAST, simple and searched CASE, arithmetic,
concatenation, comparisons, Boolean operators, and NULL predicates. It retains
byte and line/column spans, AST node/list/depth limits, and a parser recursion
limit. COUNT and COUNT_BIG alone accept a sole wildcard argument. The parser
rejects malformed or trailing forms, parameter markers within NULL predicates,
unsupported special registers and broader expression families. D2 still fences
float and decfloat numeric constants on #350; the parser also fences Boolean
constant spelling on #350. Integer/decimal tokens are held as text without
numeric semantics. Following the pinned CAST and CASE diagrams, a bare NULL
is accepted only as a CAST operand or a CASE THEN/ELSE result; the lost
parser accepted it as any operand. Function resolution, expression typing,
evaluation, other parser families, and execution remain pending. This slice
earns no whole-row, conformance, differential, or licensed credit.

## Source review

The recovered `DB2-1201.declare-cursor-prepared-syntax` slice ports lost
`c9f9e17b` (manager import `8f633cab`). It parses a prepared statement name
with optional scroll/sensitivity before CURSOR and independent, once-only
holdability, returnability, and rowset-positioning groups before FOR. Omitted
parts retain typed defaults. Inline select statements await full
select-statement syntax;
OPEN/FETCH behavior remains fenced on #350. Cursor execution, other cursor
forms, whole-row recognition, conformance, differential, and licensed credit
remain pending. The Db2 13 declaration source is the pinned
`SSEPEK_13.0.0/sqlref/src/tpc/db2z_sql_declarecursor.html` topic with SHA-256
`1362df251cb5c1a478c0846ee1db7ad85f31ae1f68e9f91324392a645f06ff55`
(catalog row SQL 0060). The configured topic cache lacked a verified TOC;
the matching retained HTML was verified locally.

The recovered `DB2-1201.select-core-syntax` slice ports lost `67942e88`
(manager import `78f7e0a4`). It owns one bounded subselect with ALL/DISTINCT,
expression and wildcard items, comma-separated named tables, WHERE search
condition, GROUP BY expressions, HAVING search condition, ORDER BY expressions
or ordinals, OFFSET, and FETCH. It reuses the D7 expression parser and D2 byte
spans. The recovered worker deliberately rejected CTEs, joins, aliases,
subqueries, set operators, SELECT INTO, and outer SELECT clauses; these remain
fenced. Fullselect composition and inline DECLARE CURSOR therefore remain
pending. `SELECT INTO` and `VALUES INTO` remain fenced on #350. This parser
has no execution route or whole-row, conformance, differential, or licensed
credit. The pinned Db2 13 SQL 0121 SELECT topic is
`SSEPEK_13.0.0/sqlref/src/tpc/db2z_sql_select.html` (SHA-256
`656ded0628526a8b16b326b32e5e9cac6d1590d7f70386ea3e077316a241ad3e`).
The configured cache lacks the Db2 TOC; matching retained HTML for the accepted
clause diagrams was verified locally. The pinned `db2z_sql_updateclause.html`
body was unavailable locally and no update clause is accepted.

The recovered `DB2-1201.create-table-common-syntax` slice ports final worker
`9b53b898` and the CREATE TABLE public surface from manager `46cf2c3f`.
It owns a bounded named-table definition with common columns, built-in or
distinct type syntax, NOT NULL, constant or NULL defaults, and table PRIMARY
KEY, UNIQUE, and FOREIGN KEY constraints with the recorded ON DELETE actions.
It rejects CHECK and other undeclared column, constraint, and physical table
clauses. Column options may appear in any order, each at most once, as in the pinned
column-definition repeat group; CHECK stays out because the lost lane rejected it. Float and decfloat
DEFAULT constants, Boolean constants, and `IN` tablespace options stay fenced
on #350. Type compatibility remains D11. This syntax is disconnected from
binding and execution and earns no whole-row recognition, conformance,
differential, or licensed credit. The pinned Db2 13 CREATE TABLE source is
`SSEPEK_13.0.0/sqlref/src/tpc/db2z_sql_createtable.html` (SHA-256
`104cc7fd0f43e804819da99c18887de60983cad8fa78b7d550ffaf63dfd299d6`),
catalog SQL 0050. Its matching retained HTML and the pinned data-type topic
were verified locally; the configured topic cache lacks the Db2 TOC.

The recovered `DB2-1202.common-type-compatibility` slice ports final worker
`ca01da01` (manager import `60c7a3a6`) and the public type surface from
`7089988a`. It resolves the existing D3 built-in `Db2DataType` syntax into
validated scalar shapes and classifies assignment and comparison across the
supported numeric, character, graphic, binary, and datetime families. The
assignment and comparison matrices are tested cell by cell over their supported
operand subset. It preserves nullability, precision, scale, length, and
timestamp time-zone shape without performing conversion. Distinct types, LOBs,
ROWID, XML, explicit CCSID/collation, and unsupported argument or zone forms
fail explicitly; string-to-datetime assignment and datetime/string comparison
require later binder context. Bit-data and Boolean have no resolved shape here.
Float and decfloat type shapes come from the available pinned data-type topic;
float, decfloat, and Boolean constants remain fenced on #350 because the pinned
`db2z_constantsintro.html` body is absent. Name/function binding, result-type
inference, execution, SQLCA, and broader type families remain pending. This
boundary grants no statement-row, conformance, differential, or licensed credit.

The Db2 13 baseline `ibm-db2-for-zos-13-2026-08-13` pins
`SSEPEK_13.0.0/sqlref/src/tpc/db2z_datatypesintro.html`
(`a488006755eedd9ef58da3ba8ef9f304a3d79c3910cc39da637dda1f3c38f570`)
for scalar families and parameter bounds, and
`SSEPEK_13.0.0/sqlref/src/tpc/db2z_assignmentandcomparison.html`
(`2cc975449a25d1d825cbd8a6551f5ea9c6dfc6ed5dc9956643f0e1dc9948af8a`)
for assignment/comparison cells. Both retained HTML bodies matched their
manifest hashes. The configured cache reader lacks the Db2 TOC. The locally
verified result-data-type (`7c23258e`), promotion (`7b24ea61`), and casting
(`0f1f2200`) topics were reviewed but add no behavior to this lost worker slice.

Baseline: `ibm-db2-for-zos-13-2026-08-13`, product `SSEPEK_13.0.0`, manifest
digest `6fd51d84b9d5c365d2017f1037f4edefb8d9af71de9277556cd50cf3e2f13757`.

The lexer source review verified retained HTML bytes for these pinned SQL
reference topics: `db2z_characterstokens` (`ed63dd289fc68abe18ec5863756239f79d93941cd525968ba48f962d1299d4ae`) for token delimiters
and parameter markers; `db2z_sqlidentifiers` (`d5c99a5640234e19a310d2e44f1abd9bbe3bf9c0fea1cf87c06b24ee9f4a1f8b`) for identifiers;
`db2z_sqlcomments` (`c8e3c16d1152a165b0c2c7b71ddfad4ca8032f910f2c92f0eca5484ce5b68ca2`) for simple and nested bracketed comments;
`db2z_graphicstringconstants` (`7755678351defbdf211b1164eb5127ee088860ad6fddd7835a965040ea41689a`) for graphic/Unicode hex forms; and
`db2z_refs2hostvars` (`3bf1787c8a0538738a20724bb4ba7d8302f6f77a2efac5605f9795de28e61e54`) for host references. Paths are under
`SSEPEK_13.0.0/sqlref/src/tpc/` with `.html` suffixes in
`conformance/0.2/manifests/db2-topics.json`. The pinned
`db2z_constantsintro.html` body (`9bbe7aeaa74f37a2a55977368714d0f8d3fd33465e65d3e416795218236f0fa7`) is absent from the offline
corpus; its replacement is not accepted as the pin. Numeric-constant rules
remain pending on #350. The configured cache reader could not verify the
baseline TOC; the matching retained topic HTML was read locally.

The AST review verified retained bytes for these Db2 13 baseline topics under
`SSEPEK_13.0.0/sqlref/src/tpc/` in
`conformance/0.2/manifests/db2-topics.json`:

| Topic | SHA-256 |
|---|---|
| `db2z_sqlidentifiers.html` | `d5c99a5640234e19a310d2e44f1abd9bbe3bf9c0fea1cf87c06b24ee9f4a1f8b` |
| `db2z_resolutionofobjnames.html` | `a1e2b49f72cd742e3d5a4866417ba9acf30dccca8091d0b48a1483ec50019f8d` |
| `db2z_refs2hostvars.html` | `3bf1787c8a0538738a20724bb4ba7d8302f6f77a2efac5605f9795de28e61e54` |
| `db2z_datatypesintro.html` | `a488006755eedd9ef58da3ba8ef9f304a3d79c3910cc39da637dda1f3c38f570` |
| `db2z_expressionsintro.html` | `dfa6cfc7c00b87a14e16ef65ef22a6970b46eb4a02ee308b7f5dbbc97194bfa8` |
| `db2z_assignmentandcomparison.html` | `2cc975449a25d1d825cbd8a6551f5ea9c6dfc6ed5dc9956643f0e1dc9948af8a` |
| `db2z_nullpredicate.html` | `c1ca9abcb98036bf0666b6710fd37c26c1499fbd8e787b670fa67a8c5c02ef9d` |

Expression syntax additionally reviewed these Db2 13 topic diagrams and rules
from the same baseline. The configured cache could not verify the TOC; each
listed retained HTML body matched its manifest hash and was read locally.
Paths are under `SSEPEK_13.0.0/sqlref/src/tpc/`.

| Topic path | SHA-256 | Reviewed fragment |
|---|---|---|
| `db2z_precedenceofoperations.html` | `97a841c3abedc4fd3df5b4c5a99f808add08292d5bd08aec579ac94fc924f351` | prefix, multiply/divide/concatenate, add/subtract, left association |
| `db2z_basicpredicate.html` | `cb335c59883dc5933531916f3934c46245d2e4301c736f1c09400d1eb7eff7e0` | comparison operator alternatives |
| `db2z_searchconditionssql.html` | `79fafaf79779ef1f7b947502d99efd2ab7d87bf1046ff65602076b8fbf4e5627` | NOT, AND, OR, parentheses around search conditions |
| `db2z_castspecification.html` | `056a0bc5d4b81506cd24f66e46413555f9a4e76206fa0c51558dfc5766761dc2` | CAST operand, AS type, bounded built-in and distinct type alternatives |
| `db2z_caseexpression.html` | `fac861b774379be825c697a83d9447f40c125fbda877f716714b0e8ea930cd4a` | searched/simple WHEN repetition, required THEN result, optional ELSE, END |
| `db2z_functioninvocation.html` | `6467e8424ae657e575748bb9de381111fc8e5035238ffd29ac771bd9241ed182` | function name and parenthesized argument list |
| `db2z_aggregatefunctionsintro.html` | `0bc5423111878ac0ad14577faf7b151c2299c84ff796ddd8392f343689fb2764` | COUNT and COUNT_BIG wildcard exception |
| `db2z_specialregistersintro.html` | `6e4df297110a29e608931a58adbaa79f2366e9740fc52414a13025f44acc168b` | special-register forms fenced pending a later parser slice |

The cache reader could not verify the baseline TOC; each cited retained HTML
body matched its manifest hash and was reviewed locally. These are source
references only and grant no conformance or licensed credit.

Common dynamic syntax reviewed these pinned Db2 13 statement diagrams in the
same baseline. The configured cache lacks the bodies, so matching retained
HTML was hash-verified and read locally. Paths are under
`SSEPEK_13.0.0/sqlref/src/tpc/` in
`conformance/0.2/manifests/db2-topics.json`:

| Catalog row | Topic path | SHA-256 |
|---|---|---|
| SQL 0075 EXECUTE | `db2z_sql_execute.html` | `80434f9a2ac28e35a675d3336a6ca629db08e37fed6db4d2021e917a09f538de` |
| SQL 0076 EXECUTE IMMEDIATE | `db2z_sql_executeimmediate.html` | `5ce657731c9783c096b320aab953d14d7ff3c1773af4625eb2221534afedc3cd` |
| SQL 0101 PREPARE | `db2z_sql_prepare.html` | `677d1a9d48585a9d65279b9bb0f98c54bc078677d24070e6200d31d04d29f423` |

Transaction syntax reviewed these pinned Db2 13 statement topics in the same
baseline. The cache reader could not verify the baseline TOC, so the matching
retained HTML bytes were read locally. Paths are under
`SSEPEK_13.0.0/sqlref/src/tpc/` in
`conformance/0.2/manifests/db2-topics.json`:

| Catalog row | Topic path | SHA-256 |
|---|---|---|
| SQL 0026 COMMIT | `db2z_sql_commit.html` | `61cb2e1f8c0e7c4f33716b4b276707e46b43f8d257cae64bfa54fddc0802de62` |
| SQL 0119 ROLLBACK | `db2z_sql_rollback.html` | `087441bdef9562e0e73c8f201dff58f8398cad578163bde9103fb59444dce786` |
| SQL 0120 SAVEPOINT | `db2z_sql_savepoint.html` | `3e168615ab06619ff7c73960212d52080822bd015cc15976da9bee34e8b1f14f` |

The caller-provided offline archive reproduces exact manifest bytes for 147 of
the 174 catalog-referenced topics. Twenty-seven pins are absent; no mismatched
body is accepted in their place. The missing set includes ALTER TABLE, DELETE,
FETCH, OPEN, UPDATE, VALUES INTO, and SQL PL SIGNAL/RESIGNAL. Work can continue
on catalog and parser infrastructure, but affected semantics and the final
common/deferred freeze remain pending until matching bodies are available.

## Declared slice ownership

| Boundary | Existing authority to extend | 0.12 rule |
|---|---|---|
| Official denominator and evidence | `conformance/0.2/catalogs/db2.json` and shared Conformance IR | Preserve all 174 row IDs, locators, obligations, and six independent gates. |
| SQL syntax and semantic types | `mainframe-env-db2` plus typed IR owners | Private parser substrate converts immediately to owned bounded Db2 tokens/AST/diagnostics; no third-party public types. |
| Public host route | `Db2Request`, `Db2Operation`, and canonical host effects | Extend additively; do not introduce a text bypass or a second provider route. |
| Catalog and application data | `Db2CatalogGeneration` and selected signed application package | Keep application names, schemas, rows, packages, and privileges data-driven. |
| Authorization | `EnterpriseAuthorizer` and `DB2TABLE`/`DB2PLAN`/`DB2UOW` resources | Typed decisions precede mutation; deny and failure do not mutate. |
| Transactions and recovery | execution coordinator, effect journal, provider UOW/replay rows | No Db2-private coordinator or automatic mutation redispatch. |
| Durable storage | provider object-row v1 and shared store/migration contracts | Add versioned readers/migrations only when schemas change; preserve restart/rollback. |
| Backends | memory, SQLite, PostgreSQL selected provider-state adapters | Behavioral claims require affected durable parity; memory alone grants no restart credit. |

## Work-package ledger

| Work package | State | Slices / next boundary |
|---|---|---|
| DB2-1201 | proposed | Reuse observations, official statement identities, owned lexer, AST primitives, transaction syntax, host references, common dynamic syntax, common expressions, prepared cursor syntax, SELECT core, and common CREATE TABLE syntax recovered; fullselect and inline cursor syntax, full dynamic obligations, catalog freeze, remaining statement parsers and SQLCA diagnostics pending |
| DB2-1202 | proposed | Common type compatibility and public type surface recovered; binder, names, functions, expression binding, privileges, result-type inference, and remaining limits pending |
| DB2-1203 | pending | relational IR, generic query/DML/DDL, constraints, indexes, views |
| DB2-1204 | pending | transaction/isolation, cursor/dynamic/static SQL, COBOL ABI, SQLCA, package/plan |
| DB2-1205 | pending | common DDL and bounded SQL PL routines/control flow |
| DB2-1206 | pending | current H1/H3 scan, migration/compatibility, property/failure/recovery/licensed suites |

## Blockers and pending external evidence

- Exact pinned HTML is currently missing for the three catalog rows listed in
  the resumed source-availability record, rather than the historical 27. This blocks semantic
  changes for those rows and the final common/deferred freeze, not unrelated
  parser/catalog infrastructure.
- The repinned numeric-constants introduction is available and reviewed, but
  the recovered lexer/expression fences have not been lifted by a declared
  numeric slice. Float/decfloat forms remain rejected and integer/decimal tokens
  carry no numeric semantics here.
- No pinned licensed Db2 13 oracle receipt is present. Differential remains
  pending and 0.12 cannot pass its exit gate without the required environment.
- The early shared participant contract must be audited before DB2-1204 mutating
  integration; 0.12 will extend it rather than create a Db2-private protocol.

No execution, conformance, differential, licensed, release, or compatibility
credit is claimed for this recovery slice.
