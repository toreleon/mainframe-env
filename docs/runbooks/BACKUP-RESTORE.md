# Backup and restore

## SQLite local profile

Stop admission and drain active requests. Run the store integrity check, create
a SQLite `VACUUM INTO` backup at a new explicit path, and record its SHA-256.
Open the backup read/write with the same binary, require migration head
`0001-durable-state`, rerun `PRAGMA integrity_check`, then start providers and
verify readiness. The automated store test performs this complete round trip.

## PostgreSQL 18 profile

Use a PostgreSQL 18 physical or `pg_dump --format=custom` backup under the
database operator's retention policy. Restore into an empty database, run the
embedded expand/contract migration bundle, and execute the ignored
`postgres18_migration_and_durable_contracts` test against the restored URL.
Never restore over an active authority.

## Artifacts

Copy the immutable `objects/<prefix>/<sha256>` tree with metadata preserved.
On restore, read every retained object through `LocalArtifactStore`; digest or
media-envelope mismatch is an integrity failure, never a cache miss.
