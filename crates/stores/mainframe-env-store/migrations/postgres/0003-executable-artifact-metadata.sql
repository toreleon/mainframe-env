DO $mainframe_env_migration$
BEGIN
  -- The versioned constraint name is the durable, idempotent migration marker.
  IF NOT EXISTS (
    SELECT 1
    FROM pg_constraint
    WHERE conrelid = 'artifact_object'::regclass
      AND conname = 'artifact_object_schema_version_v2_check'
      AND contype = 'c'
  ) THEN
    -- Serialize shared-store nodes that observe the pre-migration schema, then
    -- recheck the marker after acquiring the table lock.
    LOCK TABLE artifact_object IN ACCESS EXCLUSIVE MODE;
    ALTER TABLE artifact_object ADD COLUMN IF NOT EXISTS executable_metadata BYTEA;
    ALTER TABLE artifact_object DROP CONSTRAINT IF EXISTS artifact_object_schema_version_check;
    IF NOT EXISTS (
      SELECT 1
      FROM pg_constraint
      WHERE conrelid = 'artifact_object'::regclass
        AND conname = 'artifact_object_schema_version_v2_check'
        AND contype = 'c'
    ) THEN
      ALTER TABLE artifact_object
        ADD CONSTRAINT artifact_object_schema_version_v2_check
        CHECK(schema_version IN (1, 2));
    END IF;
  END IF;
END;
$mainframe_env_migration$;
