# Generic Db2 application catalog

Status: **Frozen for mainframe-env 0.2.0**

The Db2 provider owns one generic schema/row engine. An installed catalog
generation declares tables, ordered columns, null/default policy, primary keys,
foreign keys and delete behavior, host result encoding, optional extract
layouts, and seed rows. It is bound to an application package identity and a
positive generation.

The application boundary verifies and selects the signed package first. The
composition layer then derives every Db2 definition field from the selected
package's signed catalog blob and checks its table, column, nullability, and key
shape against the typed SQL section before calling the provider. The
provider validates all schemas, references, rows, bounds, ownership conflicts,
and generation identity before one durable state write. Compatible legacy and
selected-generation rows are retained rather than recreating tables. Before an
upgrade, the provider snapshots the complete selected schemas and rows; retained
generations can be reselected atomically after restart. Rollback removes tables
owned only by the newer selected generation, restores the complete retained
schema and row snapshot, and validates all foreign keys before persistence. It
does not inspect an application name to select behavior.

CRUD, cursor, count, DDL/load, and extraction operations derive their table,
columns, keys, bind variables, assignments, referential checks, output
encodings, and fixed extraction records from the request plus installed schema.
Raw host variables retain every byte, including non-UTF-8 and significant
leading or trailing spaces; VARCHAR length-prefix decoding remains a separate
path. Omitted seed values use declared column defaults before row-key
construction. On open, legacy textual row-map keys are deterministically
re-keyed from their exact stored primary-key bytes.
No table shape is inferred from a CardDemo name. Existing durable v1 table rows
remain readable; installing the signed v2 catalog adds schema ownership without
rewriting historical evidence. UPDATE rejects assignments to primary-key
columns before opening a pending unit, so the row map key and row bytes cannot
diverge or bypass duplicate-key detection.
