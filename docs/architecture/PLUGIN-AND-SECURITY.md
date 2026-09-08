# 0.1 Security and Capability Architecture

Status: **Accepted by repository owner**
Owner: **security and architecture maintainers**
Scope: **capability, provider, plugin, secret, and transport boundaries**
Applies from: **mainframe-env 0.1.0**

## 0.1 boundary

0.1 uses statically linked, reviewed Rust implementations only. Wasm components,
native dynamic plugins, supervised process plugins, remote workers, plugin
marketplaces, and distributed generation lifecycle are explicitly out of scope.

The 0.1 capability model is still explicit so production code does not depend on
global state and later releases can add isolation without changing mainframe
semantics.

## Principal and authorization

Authentication produces an immutable `Principal`. Authorization occurs at:

1. z/OSMF/CLI admission;
2. compiler/program/execution selection;
3. every sensitive dataset, JES, CICS, and security host operation; and
4. administrative lifecycle operations.

RACF/SAF is the authoritative 0.1 security provider. Policy errors and missing
profiles fail closed. No authorization result is inferred from HTTP routing,
possession of a Rust handle, or successful capability lookup.

## Capability grants

An invocation receives only grants needed by its selected workload, for
example:

```text
host.dataset.read
host.dataset.write
host.program.invoke
host.terminal
host.spool.write
host.security.authorize
host.clock
host.audit
```

Capabilities identify permission families and interface versions. They do not
contain provider references, credentials, filesystem paths, or mutable global
state.

## Scoped service handles

COBOL, CICS, JCL/JES, and z/OSMF code access providers through typed service
clients constructed from the invocation context. Each call rechecks:

- principal and required grant;
- execution, run-unit, session, and transaction identity;
- interface/provider version;
- deadline and cancellation;
- request and result size limits;
- idempotency/effect sequence for mutation; and
- audit requirements.

No handler or interpreter receives a broad `AppState`, RACF database, dataset
catalog lock, JES state map, or CICS provider internals.

## RACF/SAF provider

The RACF provider owns:

- user and group identities required by 0.1 fixtures;
- authentication results and stable failure categories;
- dataset and general-resource profiles;
- SAF authorization requests and decisions;
- administrative mutation transactions admitted by 0.1;
- audit events and redaction; and
- versioned persistence and configuration.

The host contract distinguishes deny, not found, invalid credentials, revoked,
cancelled, timed out, resource exhausted, provider failure, and infrastructure
failure.

## Secret handling

- Configuration and durable contracts carry `SecretRef`, not plaintext secret
  values.
- Providers resolve a secret only inside an authorized operation scope.
- Diagnostics, logs, events, HTTP responses, spool, and evidence classify and
  redact secret fields before serialization.
- Compatibility tests compare exact isolated values before publication-time
  redaction when equality is necessary.
- Long-lived credentials are never copied into compiler artifacts, machine
  checkpoints, or terminal state.
- Ephemeral request secrets live in a bounded zeroizing scope that removes its
  resolver entry on every return path. Password creation and change share one
  policy and use a fresh CSPRNG salt; retained Argon2 verifiers enforce history
  without deterministic salts.
- HTTP bearer credentials are returned once and stored only through a
  domain-separated digest. Version 3 sessions have absolute and idle expiry,
  a per-user cap enforced through one durable CAS index across server
  instances, and a non-reusable authentication epoch derived from the account's
  randomly salted verifier and state version. Every use revalidates active
  principal state and epoch, advances idle expiry by CAS, and removes expired
  or revoked state. Deleting and recreating the same user cannot revive an old
  session. Bearer authentication rotates the credential atomically while
  preserving the absolute lifetime, so the previous token is immediately
  invalid. Legacy raw-token rows are revoked and deleted during startup rather
  than recovered.

## Transport security

Rustls is the default TLS implementation for z/OSMF/server transport. Crypto
provider and certificate sources are application configuration. TLS types do
not enter execution or security contracts.

## Panic, unsafe, and failure containment

- Owned foundation, contract, compiler, execution, COBOL, JCL, JES, CICS, and
  RACF crates forbid unsafe code.
- Adapter crates requiring unsafe or FFI document and audit each block.
- Provider panics are caught at the invocation boundary where unwinding is
  enabled and become infrastructure failures.
- Panic containment does not claim to contain OOM, abort, memory corruption, or
  process termination.
- Provider failure cannot mutate unrelated run units or bypass transaction and
  idempotency policy.

## Supply-chain policy

- Commit `Cargo.lock` for applications.
- Use approved registries or pinned reviewed sources.
- Run advisory, license, source, and duplicate dependency checks.
- Record audits for security-sensitive dependency upgrades.
- Keep Rustls, SQLx/database drivers, Axum/Hyper, and parser dependencies within
  supported release windows.
- Contract packages keep minimal dependency surfaces and expose no third-party
  public types.

## Post-0.1 rule

External plugin support requires a new ADR and threat model. It must use a
language-neutral sandboxed or supervised boundary and cannot make native Rust
dynamic libraries a stable ABI. This rule creates no external-plugin work in
0.1.
