# Db2 0.12 parser and executor reuse spike

Status: **Recovered development observation; zero conformance credit**

Provenance: recovered on 2026-09-29 from the lost 2026-09-22 v0.12 Codex lane
(commit `de4252a9`, never pushed). The executable probe was a temporary `/tmp`
program in that session and was not retained; the case counts and results below
are transcribed from the session record and were not re-run on the current
toolchain or host.

This lost-lane spike records build-versus-buy observations for DB2-1201 and
DB2-1203. It compares `sqlparser-rs` 0.63.0 and Apache DataFusion 55.1.0 on a
bounded 49-case Db2 13 sample before either library can enter production. It
does not freeze the 0.12 common subset, prove a catalog row, or substitute a
third-party result for Db2 behavior.

## Source and method

The frozen denominator is
`ibm-db2-for-zos-13-2026-08-13`: 158 SQL-statement rows plus 16 SQL PL rows.
The sample contains 46 valid forms and three invalid forms covering query,
DML, DDL, constraints, decimal and character types, transactions, cursors,
dynamic SQL, COBOL-style host variables, package and plan privileges, SQL PL,
and negative parsing.

Pinned HTML was read offline from the caller-provided archive under
`raw/html/sha256`. Each used body was checked for exact byte count and SHA-256
against `conformance/0.2/manifests/db2-topics.json`, then converted with the
plain-text parser in `conformance/tools/ibm_docs.py`. Representative reviewed
topics include:

| Catalog row | Topic | SHA-256 |
|---|---|---|
| SQL 0026 COMMIT | `db2z_sql_commit.html` | `61cb2e1f8c0e7c4f33716b4b276707e46b43f8d257cae64bfa54fddc0802de62` |
| SQL 0050 CREATE TABLE | `db2z_sql_createtable.html` | `104cc7fd0f43e804819da99c18887de60983cad8fa78b7d550ffaf63dfd299d6` |
| SQL 0060 DECLARE CURSOR | `db2z_sql_declarecursor.html` | `1362df251cb5c1a478c0846ee1db7ad85f31ae1f68e9f91324392a645f06ff55` |
| SQL 0085 GRANT package | `db2z_sql_grantpackageprivileges.html` | `e178c674373db7a602585975e8d91f4bd46623ee0ac0b0b73e48eac961cd8765` |
| SQL 0086 GRANT plan | `db2z_sql_grantplanprivileges.html` | `078dcd7909556f4b789a160fa47134806eaf404c9a5b4aa42b578e6b5d37a8aa` |
| SQL 0101 PREPARE | `db2z_sql_prepare.html` | `677d1a9d48585a9d65279b9bb0f98c54bc078677d24070e6200d31d04d29f423` |
| SQL 0119 ROLLBACK | `db2z_sql_rollback.html` | `087441bdef9562e0e73c8f201dff58f8398cad578163bde9103fb59444dce786` |
| SQL 0120 SAVEPOINT | `db2z_sql_savepoint.html` | `3e168615ab06619ff7c73960212d52080822bd015cc15976da9bee34e8b1f14f` |
| SQL 0121 SELECT | `db2z_sql_select.html` | `656ded0628526a8b16b326b32e5e9cac6d1590d7f70386ea3e077316a241ad3e` |
| SQL PL 0004 compound | `db2z_compoundstatement4nativesqlpl.html` | `6edc047b9b2d255e06bc56416b725cf45bf370db7562ed1e4ba985d2cdfe23b8` |
| SQL PL 0006 GET DIAGNOSTICS | `db2z_getdiagnosticsstatement4nativesqlpl.html` | `3539f87d1d36083d6511b5dce055f33dfab82916474bb31542d1cd5e8e87842b` |
| SQL PL 0016 WHILE | `db2z_whilestatement4nativesqlpl.html` | `f7b32a5dacb18880bc6b646b89ad9abfd2263766a097ddf2c7c961d5b20ac13a` |

The offline corpus reproduces 147 of the 174 catalog-referenced topic bodies exactly.
Twenty-seven exact pins are absent, including ALTER TABLE, DELETE, FETCH, OPEN,
UPDATE, VALUES INTO, and SQL PL SIGNAL/RESIGNAL. Those rows remain source-review
pending; archive bodies with another digest are not treated as authority. The
spike cases for missing bodies measure third-party behavior only and grant no
semantic or source-review claim.

The executable probe used the pinned repository toolchains:

- Rust 1.98.0 for the parse/plan run;
- Rust 1.95.0 for the declared MSRV compile check;
- `sqlparser` 0.63.0 with default `std` and recursive-protection features;
- `datafusion` 55.1.0 with default features disabled and `sql` enabled; and
- `datafusion-sql` 55.1.0 with default features disabled.

`sqlparser::Parser` used `GenericDialect`. `DFParser` used its generic dialect.
DataFusion logical planning used two empty in-memory tables with integer,
nullable character, decimal, parent-key, category, and boolean columns. Column
names were lower-case solely to remove DataFusion's unrelated lower-case
normalization failure from most plan results; Db2 upper-case folding remains an
owned semantic gap. No plan was executed.

The exact frozen inputs were:

| Case | SQL text |
|---|---|
| SELECT literal | `SELECT 1` |
| SELECT table columns | `SELECT ID, NAME FROM T` |
| `IS NULL` | `SELECT ID FROM T WHERE NAME IS NULL` |
| inner join | `SELECT T.ID, U.NAME FROM T INNER JOIN U ON T.ID = U.ID` |
| group/having | `SELECT CATEGORY, COUNT(*) FROM T GROUP BY CATEGORY HAVING COUNT(*) > 1` |
| order/FETCH FIRST | `SELECT ID FROM T ORDER BY ID FETCH FIRST 10 ROWS ONLY` |
| common table expression | `WITH X AS (SELECT ID FROM T) SELECT ID FROM X` |
| UNION ALL | `SELECT ID FROM T UNION ALL SELECT ID FROM U` |
| scalar subquery | `SELECT ID, (SELECT MAX(ID) FROM U) FROM T` |
| correlated EXISTS | `SELECT ID FROM T WHERE EXISTS (SELECT 1 FROM U WHERE U.ID = T.ID)` |
| CASE/COALESCE | `SELECT CASE WHEN NAME IS NULL THEN 'N' ELSE COALESCE(NAME, 'X') END FROM T` |
| DECIMAL cast | `SELECT CAST(AMOUNT AS DECIMAL(9,2)) FROM T` |
| VARCHAR cast | `SELECT CAST(NAME AS VARCHAR(40)) FROM T` |
| VALUES rows | `VALUES (1, 'A'), (2, 'B')` |
| INSERT values | `INSERT INTO T (ID, NAME) VALUES (1, 'A')` |
| INSERT select | `INSERT INTO T (ID, NAME) SELECT ID, NAME FROM U` |
| UPDATE | `UPDATE T SET NAME = 'A' WHERE ID = 1` |
| DELETE | `DELETE FROM T WHERE ID = 1` |
| MERGE | `MERGE INTO T USING U ON T.ID = U.ID WHEN MATCHED THEN UPDATE SET NAME = U.NAME WHEN NOT MATCHED THEN INSERT (ID, NAME) VALUES (U.ID, U.NAME)` |
| CREATE TABLE | `CREATE TABLE X (ID INTEGER, NAME VARCHAR(40))` |
| CREATE TABLE constraints | `CREATE TABLE X (ID INTEGER NOT NULL PRIMARY KEY, PARENT_ID INTEGER, CONSTRAINT FK_X FOREIGN KEY (PARENT_ID) REFERENCES T(ID))` |
| ALTER TABLE add column | `ALTER TABLE T ADD COLUMN NOTE VARCHAR(20)` |
| CREATE INDEX | `CREATE UNIQUE INDEX IX_T_NAME ON T(NAME)` |
| CREATE VIEW | `CREATE VIEW V_T AS SELECT ID, NAME FROM T WHERE FLAG = TRUE` |
| DROP TABLE | `DROP TABLE X` |
| GRANT table privileges | `GRANT SELECT, UPDATE ON TABLE T TO USER APPUSER` |
| GRANT package privilege | `GRANT EXECUTE ON PACKAGE COLL1.PKG1 TO USER APPUSER` |
| GRANT plan privilege | `GRANT EXECUTE ON PLAN PLAN1 TO USER APPUSER` |
| REVOKE table privilege | `REVOKE SELECT ON TABLE T FROM USER APPUSER` |
| COMMIT | `COMMIT` |
| ROLLBACK | `ROLLBACK` |
| SAVEPOINT retain cursors | `SAVEPOINT S1 ON ROLLBACK RETAIN CURSORS` |
| DECLARE CURSOR | `DECLARE C1 CURSOR FOR SELECT ID, NAME FROM T` |
| OPEN cursor | `OPEN C1` |
| FETCH INTO host variables | `FETCH C1 INTO :HV_ID, :HV_NAME` |
| CLOSE cursor | `CLOSE C1` |
| PREPARE FROM host variable | `PREPARE S1 FROM :SQL_TEXT` |
| EXECUTE USING host variable | `EXECUTE S1 USING :HV_ID` |
| EXECUTE IMMEDIATE | `EXECUTE IMMEDIATE :SQL_TEXT` |
| SELECT INTO host variables | `SELECT NAME INTO :HV_NAME :HV_NAME_IND FROM T WHERE ID = :HV_ID` |
| VALUES INTO host variable | `VALUES CURRENT DATE INTO :HV_DATE` |
| compound SQL PL | `BEGIN ATOMIC SET V = 1; END` |
| IF | `IF V IS NULL THEN SET V = 0; ELSE SET V = V + 1; END IF` |
| WHILE | `WHILE V < 10 DO SET V = V + 1; END WHILE` |
| SIGNAL | `SIGNAL SQLSTATE '75001' SET MESSAGE_TEXT = 'FAILED'` |
| GET DIAGNOSTICS | `GET DIAGNOSTICS :HV_COUNT = ROW_COUNT` |
| malformed SELECT | `SELECT FROM T` |
| unterminated string | `SELECT 'ABC` |
| duplicate WHERE | `SELECT ID FROM T WHERE ID = 1 WHERE ID = 2` |

## Results

For the 46 valid forms, `sqlparser-rs` recognized 36 and DataFusion's parser
recognized 35. DataFusion planned 24. Both parsers correctly rejected two of
the three invalid forms, but both accepted and DataFusion planned the malformed
`SELECT FROM T` case. A pass below means only that the third party accepted or
planned the text; it is not evidence of Db2 equivalence.

| Case | Official row | sqlparser | DF parser | DF plan |
|---|---|---:|---:|---:|
| SELECT literal | SQL 0121 | pass | pass | pass |
| SELECT table columns | SQL 0121 | pass | pass | pass |
| `IS NULL` | SQL 0121 | pass | pass | pass |
| inner join | SQL 0121 | pass | pass | pass |
| group/having | SQL 0121 | pass | pass | pass |
| order/FETCH FIRST | SQL 0121 | pass | pass | fail |
| common table expression | SQL 0121 | pass | pass | pass |
| UNION ALL | SQL 0121 | pass | pass | pass |
| scalar subquery | SQL 0121 | pass | pass | pass |
| correlated EXISTS | SQL 0121 | pass | pass | pass |
| CASE/COALESCE | SQL 0121 | pass | pass | pass |
| DECIMAL cast | SQL 0121 | pass | pass | pass |
| VARCHAR cast | SQL 0121 | pass | pass | pass |
| VALUES rows | SQL 0156 | pass | pass | pass |
| INSERT values | SQL 0096 | pass | pass | pass |
| INSERT select | SQL 0096 | pass | pass | pass |
| UPDATE | SQL 0155 | pass | pass | fail |
| DELETE | SQL 0065 | pass | pass | pass |
| MERGE | SQL 0099 | pass | pass | pass |
| CREATE TABLE | SQL 0050 | pass | pass | pass |
| CREATE TABLE constraints | SQL 0050 | pass | pass | fail |
| ALTER TABLE add column | SQL 0015 | pass | pass | fail |
| CREATE INDEX | SQL 0039 | pass | pass | pass |
| CREATE VIEW | SQL 0059 | pass | pass | pass |
| DROP TABLE | SQL 0072 | pass | pass | pass |
| GRANT table privileges | SQL 0090 | pass | pass | fail |
| GRANT package privilege | SQL 0085 | fail | fail | fail |
| GRANT plan privilege | SQL 0086 | fail | fail | fail |
| REVOKE table privilege | SQL 0115 | pass | pass | fail |
| COMMIT | SQL 0026 | pass | pass | pass |
| ROLLBACK | SQL 0119 | pass | pass | pass |
| SAVEPOINT retain cursors | SQL 0120 | fail | fail | fail |
| DECLARE CURSOR | SQL 0060 | pass | pass | fail |
| OPEN cursor | SQL 0100 | pass | pass | fail |
| FETCH INTO host variables | SQL 0078 | fail | fail | fail |
| CLOSE cursor | SQL 0024 | pass | pass | fail |
| PREPARE FROM host variable | SQL 0101 | fail | fail | fail |
| EXECUTE USING host variable | SQL 0075 | pass | pass | fail |
| EXECUTE IMMEDIATE | SQL 0076 | pass | pass | pass |
| SELECT INTO host variables | SQL 0122 | pass | fail | fail |
| VALUES INTO host variable | SQL 0157 | fail | fail | fail |
| compound SQL PL | SQL PL 0004 | fail | fail | fail |
| IF | SQL PL 0008 | pass | pass | fail |
| WHILE | SQL PL 0016 | fail | fail | fail |
| SIGNAL | SQL PL 0015 | fail | fail | fail |
| GET DIAGNOSTICS | SQL PL 0006 | fail | fail | fail |
| malformed SELECT (must reject) | SQL 0121 | **accept** | **accept** | **accept** |
| unterminated string (must reject) | SQL 0121 | reject | reject | reject |
| duplicate WHERE (must reject) | SQL 0121 | reject | reject | reject |

Observed DataFusion plan failures included unsupported `FETCH`, foreign keys,
ALTER TABLE, GRANT/REVOKE, cursors, EXECUTE USING, and SQL PL. UPDATE also exposed
identifier-normalization behavior inconsistent with the fixture schema.

## Dependency and lifecycle cost

Both projects are Apache-2.0 and actively published by the Apache DataFusion
project. DataFusion 55.1.0 declares Rust 1.94.0; both candidates compiled on the
repository's Rust 1.95.0 MSRV. `sqlparser-rs` does not declare a package MSRV,
so the successful 1.95.0 compile is the only accepted compatibility statement.

The locked cross-target dependency closure contained 23 packages for
`sqlparser-rs` and 235 for DataFusion; the combined probe resolved 243 packages.
The DataFusion development build occupied 3.9 GiB before cleanup. DataFusion
55.1.0 also selected `sqlparser` 0.62.0, so adding a direct 0.63.0 adapter would
temporarily duplicate parser versions. The probe target directories were
cleaned after both toolchain sequences.

`sqlparser-rs` defaults to a parser recursion limit of 50 and exposes an
override, but it does not impose the product's statement-byte, token-count,
statement-count, identifier, literal, list, or AST-node limits. An owned adapter
must preflight bytes, cap tokens/statements/nodes, keep recursion at or below the
product limit, and fail allocation and overflow explicitly. DataFusion's Arrow
arrays, logical plans, types, errors, catalogs, and storage handles would add
separate bounds and cannot enter any stable or durable contract.

## Semantic gaps and ownership

Neither candidate supplies Db2 identifier folding, delimited-name rules,
data-type lengths, EBCDIC/Unicode CCSID and collation, assignment/conversion,
three-valued null logic, SQLCODE/SQLSTATE/SQLCA, package/plan behavior,
host-variable indicators, cursor sensitivity/holdability, Db2 privilege sets,
SAF decisions, savepoint/UOW effects, locks/isolation, SQL PL handlers, or
restart/recovery semantics. DataFusion additionally assumes Arrow types,
DataFusion catalog/storage authority, and its own optimizer/executor outcomes.

`sqlparser-rs` 0.63.0 exposes dialect hooks and bounded recursion. Its generic
`Statement` enum cannot represent the complete Db2 statement set, so the
accepted extension is an owned Db2 parser that may reuse the tokenizer and
selected common grammar routines, then immediately converts successful syntax
into bounded `mainframe-env` AST nodes. Missing Db2 statements require owned
parsing over the token stream; third-party AST values cannot cross the adapter.
No fork is approved. If the adapter cannot close every frozen recognition and
negative fixture without invasive upstream changes, the fallback is the owned
lexer/parser and removal of the dependency.

## Decision

The archived `impl/0.12.0` branch records a separate 48-case study of
`sqlparser` 0.58.0 and DataFusion 50.3.0 in
`tools/spikes/db2-sql/README.md` at `refs/archive/impl-0.12.0`. It rejected
both evaluated versions as direct production dependencies. This lost lane
evaluated versions 0.63.0 and 55.1.0 on 49 different cases and conditionally
favored private `sqlparser-rs` reuse. The cohorts, versions, and adapters differ;
neither result overrides the other. Dependency adoption remains undecided and
requires a current, reviewed selection before implementation.

- **Lost-lane proposal: conditionally accept `sqlparser-rs` 0.63.0 as a private syntax substrate.**
  Adoption requires the owned bounded AST boundary, all 174 deterministic
  recognition mappings, exact negative diagnostics, the repository dependency
  policy gates, and no public/durable third-party types. The dependency is not
  added by this spike.
- **Reject Apache DataFusion 55.1.0 for production parsing, planning, or
  execution.** Its limited incremental coverage does not justify the 235-package
  closure, duplicate parser version, Arrow boundary, unsupported Db2 core, or a
  second catalog/optimizer/execution authority.

If parser reuse is selected, the proposed removal plan is: the owned token
and AST adapter is the only product-facing boundary, so an owned lexer can
replace `sqlparser-rs` without changing semantic identities, packages,
checkpoints, durable catalogs, SQLCA, or conformance evidence.
