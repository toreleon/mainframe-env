# mainframe-env-db2

Ownership: a bounded durable generic static-SQL catalog, typed table/column/key
and foreign-key metadata, rows, cursors, SQLCA-shaped results, extraction
layouts, replay, and unit-of-work commit/rollback.

Application identities and table shapes are not production code. A selected
application package supplies a content-addressed `Db2CatalogGeneration` with
schemas, optional seed rows, constraints, result encodings, and extract
layouts. Installation validates the entire generation and persists schema,
rows, ownership, identity, and selection atomically. Same-identity retry is
idempotent; same-generation conflict, cross-application table collision, stale
generation, malformed schema, missing reference, and non-quiescent install fail
closed.

Signed catalog JSON is decoded through streaming cardinality, text, nesting,
duplicate, and aggregate bounds before the complete owned graph is constructed.
Compatible in-place upgrades require full normalized semantic equality.
Provenance distinguishes legacy tables from application-created tables, so
rollback restores adopted legacy state while removing only newer
application-owned tables and dependents. Raw predicates and cursors compare
exact bytes; only declared VARCHAR columns use strict length-prefix decoding.

Tables, schemas, catalog generations, run-scoped units of work, cursors, and
replay receipts persist independently under the
[provider row contract](../../../docs/contracts/PROVIDER-ROW-PERSISTENCE-V1.md).

The SQL route parses table, column, key, assignment, cursor, and DDL identities
against the installed catalog. It contains no application table or host-variable
dispatch. Non-goals are the complete Db2 13 SQL language, optimizer, utility,
package/plan, and distributed portfolio assigned to later versions. Verify with
`cargo test -p mainframe-env-db2 --locked`, `cargo xtask db2-catalog --check`,
`cargo xtask db2-statement-catalog --check`, and the pinned CardDemo Db2 and
authorization gates.

The proposed 0.12 lexer is a separate owned, bounded token stream for later
parser slices. It exposes token kinds, byte and line/column spans, diagnostics,
and a peekable cursor; it does not route SQL to execution. Float and decfloat
constant forms fail with a source-pending diagnostic under issue #350.

The proposed 0.12 AST primitives own normalized and delimited identifiers,
qualified names, host and indicator references, built-in and distinct type
syntax, literals, operators, and an append-only expression arena. Constructors
bound names, type arguments, literals, lists, nodes, references, and depth.
Expressions retain D2 source spans. Name resolution remains pending.

The proposed 0.12 transaction parser owns typed COMMIT, ROLLBACK, and SAVEPOINT
syntax, including optional WORK, named or unnamed rollback targets, UNIQUE,
and both retain clauses. It rejects unsupported families, duplicate or
malformed clauses, invalid savepoint names, and extra statements with bounded
located diagnostics. Parsing does not route SQL to execution.

Host identifiers remain distinct from SQL identifiers. The lexer preserves
host-language spelling, including COBOL hyphens. The bounded host-reference
parser accepts a variable alone or with an indicator, with optional
`INDICATOR` before the indicator variable, and rejects misplaced parts.
Host structures and host-language binding remain pending. Parsing has no
execution route.

The proposed common dynamic-SQL parser owns static-host PREPARE, EXECUTE, and
EXECUTE IMMEDIATE structure, including SQLDA naming modes, attribute indicators,
USING lists, and descriptors. It rejects PL/I string expressions, SQL PL
variables and array elements, multi-row source buffers, and forbidden source
indicators. This partial family support grants no whole-row recognition credit
and has no execution route.

The proposed prepared DECLARE CURSOR parser owns scrollability and sensitivity,
holdability, returnability, and rowset positioning. It preserves omitted
keywords as typed defaults, rejects duplicate or misplaced clauses with
locations, and fences inline queries until full select-statement syntax is
available. It has no cursor
execution route or whole-row recognition credit.

The proposed SELECT core parser owns a bounded subselect with select items,
named table sources, WHERE, GROUP BY, HAVING, ORDER BY, OFFSET, and FETCH.
It reuses the owned expression parser and rejects joins, aliases, set operators,
subqueries, SELECT INTO, and outer SELECT clauses with located diagnostics.
Fullselect and inline cursor integration remain pending. This syntax has no
execution route or whole-row recognition credit.

The proposed common CREATE TABLE parser owns one named-table definition with
bounded columns, built-in or distinct type syntax, NOT NULL, operand-less,
constant or NULL defaults, and table PRIMARY KEY, UNIQUE, and FOREIGN KEY constraints with the
recorded ON DELETE actions. It rejects unsupported column and physical-table
clauses, including CHECK, before binding or execution. The public syntax is
disconnected from the SQL execution route and earns no whole-row recognition
or conformance credit.

Omitted default clauses remain distinct from present operand-less type defaults
and explicit NULL/literal operands. `Db2ColumnDefault::value()` now returns
`Option<&Db2Literal>`; `None` is a present type-default clause, never an inferred
NULL or zero. Clause/operand spans preserve original source; numeric operands
also retain separate sign and number-token spans across intervening trivia.
This intentional development API adjustment changes no accepted wire/durable
contract. Quoted names decode escapes once. Default values/applicability and
all binding or insertion effects remain unresolved by this syntax API.

The proposed common type boundary resolves the owned AST's built-in type syntax
to bounded numeric, character, graphic, binary, and datetime shapes. It exposes
directional assignment and symmetric comparison classifications, preserving
nullability and timestamp time-zone distinctions. Distinct types, LOBs, ROWID,
XML, explicit CCSID/collation, and context-sensitive datetime strings remain
explicitly rejected or deferred. This pure boundary performs no conversion or
execution. Float and decfloat *type shapes* use the pinned data-type topic;
float, decfloat, and Boolean *constants* remain fenced on #350.

The common schema-qualification surface returns owned candidates for alias,
index, table and view names under explicit static or dynamic RUN/BIND/DEFINE/
INVOKE context. Qualified names and original spans are preserved. Unqualified
table/view/alias candidates explicitly retain the current-user synonym lookup
dependency unless an external caller confirms absence; every candidate still
needs catalog lookup. SQL-path objects, EXPLAIN output and catalog/authorization
resolution fail explicitly. This surface never establishes object existence,
privileges or execution and grants no statement-row or licensed credit.

The common INSERT VALUES surface preserves qualified targets, unique optional
columns, expression/DEFAULT/NULL values and uniform-width rows. Nested column
references in VALUES and undeclared forms fail explicitly. The CREATE INDEX
surface preserves three uniqueness modes and omitted/ASC/DESC/RANDOM ordering
on at most 64 distinct unqualified column keys; physical and expression-index
forms remain unsupported. CREATE VIEW reuses SELECT core and transfers it into
owned located syntax with result-name/width validation and CHECK OPTION spelling.
It rejects wildcard widths, host/parameter references and forbidden functions.
All three retain original-source spans and aggregate bounds. Identifier transfer
decodes quoted-name escapes; inherited literal escape text, SELECT's compiled
raw-name ceiling and CAST's first-component limitation remain explicit. Catalog
validity, CHECK applicability, binding, execution and whole-row credit remain
pending; none of these APIs changes the durable SQL route.

The pure numeric-constant classifier accepts one located signed or unsigned
INTEGER/BIGINT/DECIMAL spelling from bounded original UTF-8 source. It returns
the existing resolved type with NOT NULL, exact decimal precision/scale and
verified byte/line/column endpoints. Fixed errors distinguish invalid input
from deferred exponent/special/comma/long-integer forms. It performs no numeric
evaluation, conversion or expression binding, and does not lift lexer fences.

The exact natural numeric-literal materializer pairs that verified type and
original span with an owned INTEGER/BIGINT/DECIMAL value. DECIMAL uses the
foundation `DecimalValue` fixed-point primitive, preserving scale and natural
precision, including leading/trailing zeroes; numeric negative zero becomes
zero. It never passes through floating-point arithmetic or lets callers forge
a verified type/value pair. All classifier limits and deferred-form errors
remain intact. This value surface is not assignment/default conversion, numeric
expression evaluation, typed cell persistence or a catalog canonical preimage.

The exact numeric assignment surface converts only opaque materialized literal
proofs into validated SMALLINT/INTEGER/BIGINT/DECIMAL targets. Results own the
matching target type/value and original span. Whole-part bounds precede decimal
scale expansion; fractional digits are truncated toward zero, not rounded.
Conversion observations distinguish discarded digits from discarded nonzero
digits and retain INTEGER/BIGINT temporary DECIMAL(11,0)/(19,0) attributes.
They do not invent SQLCA warnings or SQLCODE. Compatible but unimplemented
floating, DECFLOAT and nonnumeric conversions remain distinct from datatype
incompatibility. This surface does not establish binder/default applicability,
evaluate expressions, assign host memory or implement typed cells/catalogs.

The natural string-literal surface accepts an original located apostrophe,
X character-hex or BX binary constant under explicit Unicode UTF-8 source
context. Opaque owned proofs decode escaped delimiters/hex exactly once and
retain matching VARCHAR/VARBINARY natural byte length, NOT NULL and original
byte/line/column spans. Empty constants have natural length zero, not NULL;
column/type constructors still require positive lengths. Character values carry
CCSID 1208/MIXED independently of the explicit MIXED DATA flag, which controls
X hex uppercase admission. Binary values have no CCSID and preserve arbitrary
bytes, including NUL; they are not character FOR BIT DATA. The lexer admits BX
as a first-class Binary string kind without changing existing literal payloads.
Original source, spelling, token, body and decoded-value budgets precede
allocation. Other encodings, delimiters, graphic and ordinary value families
remain explicitly pending. This surface neither converts target values nor
establishes defaults, collation, catalog/cell identities, execution or row credit.

The searched DELETE/UPDATE surfaces own qualified targets and optional common
search conditions; UPDATE also owns unique single-column expression/DEFAULT/
NULL assignments. Scalar/predicate structure is validated at every expression
depth, decoded names and complete-source spans are retained, and statement-wide
budgets remain enforced. Positioned/fullselect/physical extensions and known
aggregate-family UPDATE calls are fenced pending their declared binding scope.
The static-host OPEN/FETCH surface preserves cursor spelling, orientation
defaults, host/indicator leaves and descriptor operands. Exact repeated FETCH
targets fail while repeated OPEN inputs remain legal. Host aliases, SQLDA
contents, cursor applicability, catalog privileges and all execution remain
pending; no public API changes durable SQL dispatch or grants whole-row credit.

Pure arithmetic metadata exposes unary/binary numeric result types under explicit
DEC15/DEC31 and minimum-divide-scale context. Constant digit provenance is
constructed from verified original source rather than caller-supplied counts;
binder-supplied operands retain only structural span validation. Results own
their types and retain runtime range, divisor, truncation and SQLWARN7 obligations,
not evaluated values. Mixed decimal/binary-float arithmetic with DECFLOAT remains
explicitly unresolved pending its temporary-conversion precision rule.

Ordered result-type combination exposes numeric, binary and same-family datetime
shapes with verified original UTF-8 locations, aggregate operand bounds, explicit
untyped NULL and CASE/COALESCE application context. Precision caps retain whole-part
preservation and conversion obligations, including each ordered candidate step.
Character/graphic CCSID/collation, datetime strings and FLOAT(n) alias binding
remain pending. Neither surface parses expressions, resolves function overloads,
changes the durable SQL route or grants licensed or statement-row credit.

Common DROP syntax exposes TABLE/VIEW/INDEX and non-PUBLIC ALIAS names, keeping
an omitted alias designator distinct from explicit FOR TABLE. RENAME exposes
TABLE/INDEX source names and an explicit unqualified destination. TABLE, VIEW
and ALIAS names allow at most three components; INDEX names allow at most two.
Effective identifiers decode quoted escapes once and preserve case, with
insignificant trailing spaces removed. Both APIs own complete original-source
byte/line/column spans, enforce configured lexer/AST resource limits, and reject
unsupported object families, clauses and additional statements. They neither
infer current-server applicability, resolve an alias or destination qualifier,
nor prove object existence, privileges, dependency effects or mutation. These
partial SQL0072/SQL0105 surfaces do not add a generic dispatcher or full-row credit.
