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

The 0.12 syntax path begins with a bounded private `sqlparser-rs` tokenizer.
It immediately converts into owned Db2 tokens, source spans and diagnostics;
third-party AST, errors, catalogs and types are not public or durable state.
The lexer is not an execution route and grants no statement recognition credit
until later owned parser and Conformance IR slices bind complete syntax.

Owned AST primitives normalize ordinary identifiers while preserving delimited
identifiers, retain qualified names, host variables and indicator variables,
represent built-in/distinct type syntax, and store expressions in a bounded
append-only arena. Arena nodes can reference only prior nodes, so cycles,
forward references, excessive depth, lists, literals and node counts fail before
an AST can be published. Type compatibility and name resolution remain binder
responsibilities rather than parser guesses.

Statement parsing is added only in complete source-reviewed families. The first
family owns typed COMMIT, ROLLBACK and SAVEPOINT AST, including WORK,
named/unnamed savepoint rollback, UNIQUE, and retain-clause structure. It
rejects other statement families and extra tokens; parsing alone is not an
execution or transaction-authority path.

SQL identifiers and host identifiers remain distinct. SQL names apply Db2
ordinary/delimited rules; host identifiers preserve host-language spelling,
including COBOL hyphens, for the host binder to resolve with its own ABI rules.

The common dynamic-SQL parser owns static-host PREPARE, EXECUTE, and EXECUTE
IMMEDIATE structure, including SQLDA naming modes, attribute indicators, USING
lists and descriptors. It rejects PL/I string expressions, SQL PL array
elements, multi-row source buffers, and forbidden source indicators until their
separate obligations land; this partial family support grants no whole-row
recognition credit.

The public SELECT-core syntax surface owns one bounded subselect with
quantifiers, expression/wildcard items, named sources, WHERE, GROUP BY, HAVING,
ORDER BY, OFFSET, and FETCH. It accumulates expression-node limits across the
statement and rejects joins, aliases, CTEs, set operations, subqueries, SELECT
INTO, and undeclared outer clauses. It is not a binder, plan, or execution path.

The public common CREATE TABLE syntax surface owns named columns, built-in or
distinct type syntax, NOT NULL, constant/NULL defaults, and table PRIMARY KEY,
UNIQUE, and FOREIGN KEY constraints with the declared ON DELETE actions. The
public prepared DECLARE CURSOR surface preserves explicit/default scroll,
sensitivity, holdability, returnability, target, and rowset positioning. Both
fail closed outside their recorded subsets and grant no whole-row credit.

The public common type boundary resolves parser syntax into validated numeric,
character, graphic, binary, and datetime shapes while preserving precision,
scale, length, time zone, and nullability. Its assignment and comparison
classifications are deterministic but perform no conversion. Distinct types,
LOBs, ROWID, XML, arrays, explicit CCSID/collation and context-sensitive
datetime strings remain explicit binder/catalog obligations.
