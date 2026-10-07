# ADR-0026: Self-recorded CardDemo run bundle

Status: **Proposed**
Owner: **conformance and xtask maintainers**
Scope: **jes.execution READACCT batch regression evidence**
Applies from: **mainframe-env current subsystem contracts**

## Context

ADR-0024 distinguishes self-recorded regression pins from independent expected
values. A reproducible READACCT run needs to retain the input and output bytes,
the installed executable identity, the effective logical environment, and the
observations that explain the result. The modernize-ai runner-role question in
modernize-ai#6 remains open.

## Decision

`run-bundle@1` contains pinned corpus commit and tree, source and helper digests,
compiler inputs and installed artifact identities, the initial catalog and
record bytes, ordered coarse effects, terminal job and step states, spool and
output dataset records, and wall-clock observations. The canonical logical
projection is the `logical` JSON object serialized with sorted object keys and
compact JSON; `replay_digest` is SHA-256 of those bytes. It includes the pinned
2022-07-06 COBOL current date. Wall-clock start, finish and duration are
observations outside the digest. A replay with the same logical inputs must
produce the same digest despite a different wall clock.

The current capture observes job, step, spool and dataset surfaces after the
run. It does not yet capture the ordered internal dataset open/read/write,
program-call (including COBDATFT), or spool-write effect stream. The bundle
records that gap explicitly. It asserts the three READACCT datasets' names,
attributes, counts and byte identities, and checks selected fields against
CBACT01C's source logic and input records.

The local mainframe-env interpreter maps to modernize-ai's `modern-candidate`
runner role for schema projection only. Its authority is always
`development-only`. The projection maps actual local outputs and termination
to `Observation`, and local identities to `ExecutionManifest`. Fields for
modernize-ai artifacts that this run does not produce use explicit local
`NotApplicableLocal` references. The modernize-ai schema fixes locale to
`en-US`, while this run's effective COBOL locale is `C`; the projection's
`en-US` is a schema placeholder and must not be read as the runtime locale.
The schema also requires a random seed, so its projected zero is a placeholder
for this run's unused, unset random source. The bundle remains the source of
truth for effective inputs. Process identity, attempt ID, process exit code,
byte counts and cleanup status are not captured for a separate runner process;
`RunnerResultManifest` is deferred rather than inventing its attempt receipt.
The external real-COBOL runtime's runner role is owned by modernize-ai#6.

The bundle is self-recorded regression evidence under ADR-0024, not an
independent expected value, customer capture, remote oracle, licensed IBM
result, or z/OS run. The five modernize-ai schema files are vendored as test
fixtures with a commit and SHA-256 manifest. No modernize-ai crate, submodule,
path dependency, or implementation code is imported.

## Consequences

`cargo xtask carddemo-readacct --check` reruns the pinned job, compares its
logical projection with the checked-in bundle, verifies vendored bytes, and
validates local projections offline. A change to product semantics or pinned
inputs needs a reviewed new self-recorded bundle. These checks grant zero
independent conformance credit.
