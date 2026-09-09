CREATE TABLE IF NOT EXISTS retention_archive (
  archive_id TEXT PRIMARY KEY,
  target TEXT NOT NULL,
  archived_tick BIGINT NOT NULL CHECK(archived_tick > 0),
  watermark_tick BIGINT NOT NULL CHECK(watermark_tick >= 0),
  row_count BIGINT NOT NULL CHECK(row_count > 0),
  payload_bytes BIGINT NOT NULL CHECK(payload_bytes >= 0)
);

CREATE TABLE IF NOT EXISTS retention_archive_row (
  archive_id TEXT NOT NULL REFERENCES retention_archive(archive_id),
  ordinal BIGINT NOT NULL CHECK(ordinal >= 0),
  namespace TEXT NOT NULL,
  key TEXT NOT NULL,
  source_version BIGINT NOT NULL CHECK(source_version > 0),
  payload BYTEA NOT NULL,
  retention_tick BIGINT NOT NULL CHECK(retention_tick > 0),
  owner_execution TEXT,
  PRIMARY KEY(archive_id, ordinal)
);

CREATE TABLE IF NOT EXISTS retention_observation (
  target TEXT NOT NULL,
  namespace TEXT NOT NULL,
  key TEXT NOT NULL,
  observation_version BIGINT NOT NULL CHECK(observation_version > 0),
  source_version BIGINT NOT NULL CHECK(source_version > 0),
  source_digest BYTEA NOT NULL CHECK(octet_length(source_digest) = 32),
  observed_tick BIGINT NOT NULL CHECK(observed_tick > 0),
  owner_execution TEXT,
  accounted_bytes BIGINT NOT NULL CHECK(accounted_bytes > 0),
  PRIMARY KEY(target, namespace, key)
);

CREATE TABLE IF NOT EXISTS retention_lock (
  singleton SMALLINT PRIMARY KEY CHECK(singleton = 1),
  epoch BIGINT NOT NULL CHECK(epoch >= 0),
  clock_tick BIGINT NOT NULL CHECK(clock_tick >= 0)
);

INSERT INTO retention_lock(singleton,epoch,clock_tick) VALUES(1,0,0) ON CONFLICT DO NOTHING;

DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM provider_state WHERE namespace LIKE 'durable-audit:%') THEN
    UPDATE provider_state
    SET key=lpad(octet_length(substring(namespace FROM char_length('durable-audit:')+1))::text,3,'0')
            ||':'||substring(namespace FROM char_length('durable-audit:')+1)||':'||key,
        namespace='durable-audit-v1'
    WHERE namespace LIKE 'durable-audit:%';
  END IF;
END;
$$;

CREATE OR REPLACE FUNCTION bump_provider_state_retention_epoch()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  UPDATE retention_lock SET epoch=epoch+1 WHERE singleton=1;
  RETURN NULL;
END;
$$;

CREATE OR REPLACE TRIGGER provider_state_retention_epoch
BEFORE INSERT OR UPDATE OR DELETE ON provider_state
FOR EACH STATEMENT EXECUTE FUNCTION bump_provider_state_retention_epoch();
