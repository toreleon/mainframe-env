# Db2 — Engine and common SQL progress

Subsystem: **db2**
Phase: **core**
Target release: **0.12.0**

Status: **Proposed — reuse observations, statement identities, lexer, AST primitives, transaction, host-reference, dynamic syntax, common expressions, prepared cursor syntax, SELECT core, CREATE TABLE subset, and common type compatibility recovered**

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

- Exact pinned HTML is still missing for 27 catalog rows. This blocks semantic
  changes for those rows and the final common/deferred freeze, not unrelated
  parser/catalog infrastructure.
- The pinned numeric-constants introduction is unavailable (#350); float and
  decfloat forms are rejected, and integer/decimal tokens carry no numeric
  semantics in this slice.
- No pinned licensed Db2 13 oracle receipt is present. Differential remains
  pending and 0.12 cannot pass its exit gate without the required environment.
- The early shared participant contract must be audited before DB2-1204 mutating
  integration; 0.12 will extend it rather than create a Db2-private protocol.

No execution, conformance, differential, licensed, release, or compatibility
credit is claimed for this recovery slice.
