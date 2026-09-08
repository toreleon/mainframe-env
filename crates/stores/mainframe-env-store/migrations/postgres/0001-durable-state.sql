CREATE TABLE IF NOT EXISTS provider_state (
  namespace TEXT NOT NULL,
  key TEXT NOT NULL,
  version BIGINT NOT NULL CHECK(version > 0),
  payload BYTEA NOT NULL,
  PRIMARY KEY(namespace, key)
);

CREATE TABLE IF NOT EXISTS store_quota (
  quota_key TEXT PRIMARY KEY,
  max_rows BIGINT NOT NULL CHECK(max_rows > 0),
  used_rows BIGINT NOT NULL CHECK(used_rows >= 0 AND used_rows <= max_rows)
);

CREATE TABLE IF NOT EXISTS artifact_object (
  object_key TEXT PRIMARY KEY,
  schema_version SMALLINT NOT NULL CHECK(schema_version = 1),
  media_type TEXT NOT NULL CHECK(length(media_type) > 0),
  payload_digest BYTEA NOT NULL CHECK(octet_length(payload_digest) = 32),
  payload BYTEA NOT NULL
);
