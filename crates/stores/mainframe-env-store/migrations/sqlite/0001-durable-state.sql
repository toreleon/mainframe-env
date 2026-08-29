CREATE TABLE IF NOT EXISTS provider_state (
  namespace TEXT NOT NULL,
  key TEXT NOT NULL,
  version INTEGER NOT NULL CHECK(version > 0),
  payload BLOB NOT NULL,
  PRIMARY KEY(namespace, key)
)
