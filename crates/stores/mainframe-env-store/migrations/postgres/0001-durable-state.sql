CREATE TABLE IF NOT EXISTS provider_state (
  namespace TEXT NOT NULL,
  key TEXT NOT NULL,
  version BIGINT NOT NULL CHECK(version > 0),
  payload BYTEA NOT NULL,
  PRIMARY KEY(namespace, key)
)
