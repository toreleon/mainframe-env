# Core-server operations

The `core-server` profile is a single-node service with one active worker pool.
Configuration precedence is file, then `MAINFRAME_ENV_*` environment values,
then CLI overrides. `mainframe-env.config@1` rejects unknown fields and
incompatible schema versions. PostgreSQL URLs and TLS private keys are secret
references; they are not stored in config, diagnostics, or evidence.

Production readiness requires the selected SQL store, provider registry,
artifact root, and Rustls TLS material. `/zosmf/info` reports the product,
contract version, capabilities, and derived readiness. Shutdown first stops
admission, then waits up to `shutdown_millis` for the bounded active request set;
durable jobs and suspended CICS sessions remain recoverable.

The product exports stable counters for admitted requests, failures, active
requests, authentication sessions, and console messages. Queue, session,
dataset, spool, artifact, checkpoint, and provider limits are mandatory and
fail with typed resource-exhaustion outcomes.
