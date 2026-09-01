# Generic Db2 application catalog

Status: **Frozen for mainframe-env 0.2.0**

The Db2 provider owns one generic schema/row engine. An installed catalog
generation declares tables, ordered columns, null/default policy, primary keys,
foreign keys and delete behavior, host result encoding, optional extract
layouts, and seed rows. It is bound to an application package identity and a
positive generation.

The application boundary verifies and selects the signed package first. The
composition layer then derives every Db2 definition field from the selected
package's signed catalog blob. A bounded streaming decoder enforces blob,
table, column, key, foreign-key, extract, nested, text, aggregate, and duplicate
normalized-name limits before the full owned definition graph exists. The
composition layer checks decoded table identities against the typed SQL section
before calling the provider. The provider validates all schemas, references,
rows, bounds, ownership conflicts,
and generation identity before one durable state write. Compatible legacy and
selected-generation rows are retained rather than recreating tables. Before an
upgrade, the provider snapshots the complete selected schemas and rows; retained
generations can be reselected atomically after restart. Table provenance
distinguishes pre-existing legacy state from application-created state. Before
adoption, the provider retains the legacy schema and rows. Rollback removes
only tables created by the rolled-back application generation, restores an
adopted legacy snapshot and ownership, removes genuinely newer
application-owned dependents, and validates every foreign key before
persistence. It does not inspect an application name to select behavior.

An in-place compatible upgrade requires normalized equality of every semantic
field: table and column names, order, nullability, maximum bytes, result
encoding, defaults, primary keys, foreign-key columns/targets/referenced
columns/delete actions, and extract names/fields/width/trailer/presence. Any
change requires a separate explicit bounded migration; there is no implicit
Raw/VARCHAR, default, constraint, or extraction-layout conversion.
Static SQL DDL redeclaration parses both `CREATE TABLE` and bounded
`ALTER TABLE ... FOREIGN KEY` clauses. Because SQL DDL cannot express the
package-owned extract projection, a no-op redeclaration preserves that exact
installed projection only after every SQL-expressible semantic field compares
equal; DDL cannot add, remove, or modify extraction behavior.

CRUD, cursor, count, DDL/load, and extraction operations derive their table,
columns, keys, bind variables, assignments, referential checks, output
encodings, and fixed extraction records from the request plus installed schema.
Raw predicate operands, counts, primary keys, and cursor positions retain and
compare every byte, including non-UTF-8 and significant leading or trailing
spaces. Declared VARCHAR values use a strict two-byte length prefix and reject
malformed inputs. A fixed CHAR host binding is explicitly distinguished by the
absence of the bounded big-endian prefix, must fit the column, and removes
trailing space padding byte-wise; a zero-leading malformed prefix is rejected.
No lossy UTF-8 fallback participates in selection or order.
Omitted seed values use declared column defaults before row-key
construction. On open, legacy textual row-map keys are deterministically
re-keyed from their exact stored primary-key bytes.
No table shape is inferred from a CardDemo name. Existing durable v1 table rows
remain readable; installing the signed v2 catalog adds schema ownership without
rewriting historical evidence. UPDATE rejects assignments to primary-key
columns before opening a pending unit, so the row map key and row bytes cannot
diverge or bypass duplicate-key detection.
