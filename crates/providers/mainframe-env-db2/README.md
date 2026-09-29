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
Expressions retain D2 source spans. Name resolution and type compatibility
remain pending.

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
bounded columns, built-in or distinct type syntax, NOT NULL, constant or NULL
defaults, and table PRIMARY KEY, UNIQUE, and FOREIGN KEY constraints with the
recorded ON DELETE actions. It rejects unsupported column and physical-table
clauses, including CHECK, before binding or execution. The public syntax is
disconnected from the SQL execution route and earns no whole-row recognition
or conformance credit.
