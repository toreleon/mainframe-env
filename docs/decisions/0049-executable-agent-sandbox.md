# ADR-0049: Executable application sandbox and agent boundary

Status: Implemented for native and container execution; Cube integration pending
Owner: Application and developer tooling maintainers
Scope: Application profiles, lifecycle, packaging and agent access
Applies from: mainframe-env current subsystem contracts

## Decision

Publish the user-facing executable as Mainframe Sandbox while retaining the
`mainframe-env` Rust framework and its 26-package topology. Application-specific
composition remains in existing CardDemo tooling and exports a portable runtime
binary with explicit resource paths. It does not use repository discovery or
invoke certification to launch an application.

The Python standard-library controller owns child processes, source workspaces
and atomically selected application generations. CLI and MCP stdio clients share
typed operations. Language, execution, provider, authorization and storage
semantics remain behind the existing Rust contracts and authenticated gateways.
Native mode owns application state and processes; containers or MicroVMs supply
OS isolation for arbitrary agent shell commands.

CardDemo reference verification and editable application compilation are separate
inputs. The upstream corpus still provides verified seed data. Deployment creates
a fresh state generation and activates it only after readiness, preserving the
previous generation for rollback. Restart retains deployed source and data.
Reset retains deployed source while starting fresh seed data. This avoids changing
the current immutable program installation authority or silently migrating data
between modified application schemas.

The image pins Rust/Python base digests and Git source bytes, builds compatible
Linux binaries, and exports complete dependency legal texts including the
distributed CardDemo composition closure. A Cube template can inject `envd` and
probe aggregate readiness. VM creation, cloning and rollback require trusted
host-side controller rekeying before an agent receives access; snapshotting
credentials does not establish independent identities. Shared writable host
mounts are excluded from the per-instance database boundary.

## Verification

The application gate runs real upstream CardDemo through two independent
controllers, including persistent edits, source deployment, reset, rollback and
failed candidate isolation. Protocol checks use an independent official MCP
client. Container checks exercise the same profile under a non-root user with
read-only root filesystem, explicit resource limits and dropped capabilities.
Cube lifecycle and ingress require validation on a KVM-capable deployment.

The [sandbox guide](../guides/MAINFRAME-SANDBOX.md) defines setup, tool operations,
limits and the supported profile boundary. Existing conformance gates remain the
authority for subsystem compatibility and licensed differential status.
