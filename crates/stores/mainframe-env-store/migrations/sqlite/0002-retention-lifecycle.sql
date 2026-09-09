CREATE TABLE IF NOT EXISTS retention_archive (
  archive_id TEXT PRIMARY KEY,
  target TEXT NOT NULL,
  archived_tick INTEGER NOT NULL CHECK(archived_tick > 0),
  watermark_tick INTEGER NOT NULL,
  row_count INTEGER NOT NULL CHECK(row_count > 0),
  payload_bytes INTEGER NOT NULL CHECK(payload_bytes >= 0)
);

CREATE TABLE IF NOT EXISTS retention_archive_row (
  archive_id TEXT NOT NULL,
  ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
  namespace TEXT NOT NULL,
  key TEXT NOT NULL,
  source_version INTEGER NOT NULL CHECK(source_version > 0),
  payload BLOB NOT NULL,
  retention_tick INTEGER NOT NULL CHECK(retention_tick > 0),
  owner_execution TEXT,
  PRIMARY KEY(archive_id, ordinal),
  FOREIGN KEY(archive_id) REFERENCES retention_archive(archive_id)
);

CREATE TABLE IF NOT EXISTS retention_observation (
  target TEXT NOT NULL,
  namespace TEXT NOT NULL,
  key TEXT NOT NULL,
  observation_version INTEGER NOT NULL CHECK(observation_version > 0),
  source_version INTEGER NOT NULL CHECK(source_version > 0),
  source_digest BLOB NOT NULL CHECK(length(source_digest) = 32),
  observed_tick INTEGER NOT NULL CHECK(observed_tick > 0),
  owner_execution TEXT,
  accounted_bytes INTEGER NOT NULL CHECK(accounted_bytes > 0),
  PRIMARY KEY(target, namespace, key)
);

CREATE TABLE IF NOT EXISTS retention_lock (
  singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
  epoch INTEGER NOT NULL CHECK(epoch >= 0),
  clock_tick INTEGER NOT NULL CHECK(clock_tick >= 0)
);

INSERT OR IGNORE INTO retention_lock(singleton, epoch, clock_tick) VALUES(1, 0, 0);

CREATE TRIGGER IF NOT EXISTS provider_state_retention_insert
BEFORE INSERT ON provider_state
BEGIN
  UPDATE retention_lock SET epoch=epoch+1 WHERE singleton=1;
END;

CREATE TRIGGER IF NOT EXISTS provider_state_retention_update
BEFORE UPDATE ON provider_state
BEGIN
  UPDATE retention_lock SET epoch=epoch+1 WHERE singleton=1;
END;

CREATE TRIGGER IF NOT EXISTS provider_state_retention_delete
BEFORE DELETE ON provider_state
BEGIN
  UPDATE retention_lock SET epoch=epoch+1 WHERE singleton=1;
END;

UPDATE provider_state
SET key=printf('%03d',length(CAST(substr(namespace,length('durable-audit:')+1) AS BLOB)))
        ||':'||substr(namespace,length('durable-audit:')+1)||':'||key,
    namespace='durable-audit-v1'
WHERE namespace LIKE 'durable-audit:%';
