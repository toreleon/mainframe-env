ALTER TABLE artifact_object ADD COLUMN executable_metadata BYTEA;
ALTER TABLE artifact_object DROP CONSTRAINT artifact_object_schema_version_check;
ALTER TABLE artifact_object ADD CONSTRAINT artifact_object_schema_version_check
  CHECK(schema_version IN (1, 2));
