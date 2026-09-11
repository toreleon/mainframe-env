# Backup and restore

## SQLite local profile

Stop admission and drain active requests. Run the store integrity check, create
a SQLite `VACUUM INTO` backup at a new explicit path, and record its SHA-256.
Open the backup read/write with the same binary, require migration head
`0002-retention-lifecycle`, rerun `PRAGMA integrity_check`, then start providers and
verify readiness. The automated store test performs this complete round trip.

## PostgreSQL 18 profile

Use a PostgreSQL 18 physical or `pg_dump --format=custom` backup under the
database operator's retention policy. Restore into an empty database, run the
embedded expand/contract migration bundle through
`0003-executable-artifact-metadata`, and execute the ignored
`postgres18_migration_and_durable_contracts` test against the restored URL.
Verify that `artifact_object.executable_metadata`, the
`artifact_object_schema_version_v2_check` constraint, and both quota rows are
present and consistent. Never restore over an active authority. A binary
downgrade after `0003` requires a drained service and the pre-migration backup;
do not destructively remove the column or constraint in place.

## Artifacts

For a local profile, copy the immutable `objects/<prefix>/<sha256>` tree with
metadata preserved. On restore, read every retained object through
`LocalArtifactStore`; digest or media-envelope mismatch is an integrity
failure, never a cache miss.

The PostgreSQL profile stores immutable objects in `artifact_object`; include
that table and both `store_quota` rows in the same physical or custom-format
database backup. On restore, `PostgresStateStore` and
`PostgresArtifactStore` reconcile recorded quota usage against actual rows and
fail closed before serving a missing, partial, or corrupt object.
