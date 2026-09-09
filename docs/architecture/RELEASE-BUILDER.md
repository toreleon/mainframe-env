# Release builder security model

Status: **Implemented**
Owner: **release and CI maintainers**
Scope: **trusted local Jenkins builder identity and signing boundary**
Applies from: **mainframe-env 0.8.3 development**

## Local Jenkins v1

Builder ID:
`https://github.com/toreleon/mainframe-env/blob/main/docs/architecture/RELEASE-BUILDER.md#local-jenkins-v1`

This identity covers one single-tenant local Jenkins controller, the host and
repository owner administering it, its capped filesystem, the exact controller
WAR and plugin closure in `tools/jenkins/controller-plugins.lock.json`, the
tool/input versions in `tools/ci-inputs.lock.json`, and the release-signing key
held as a Jenkins file credential. Consumers must accept only the Ed25519
signer/builder pair in `config/release-attestation-policy.json`.

The controller checks out an existing release tag, verifies immutable CI and
dependency inputs, runs full assurance, builds and probes the exact target
binaries, and injects the signing credential only for receipt generation. The
DSSE signature authenticates the exact in-toto Statement bytes and payload type.
The unique Jenkins build URL is the SLSA `invocationId`.

This builder claims **SLSA Build Level 1 only**. The signature provides origin
and tamper detection against the repository trust policy, but the local host,
administrator, Jenkins controller, and tag-controlled build process remain in
the trust base. There is no claim of hosted, isolated, ephemeral, or
non-forgeable Level 2/3 control-plane guarantees. A future stronger builder
requires a new builder ID and signer.

The provenance fields are generated as follows:

- `subject`: hashes of the probed CLI and server binaries;
- `externalParameters`: the complete build-type interface;
- `resolvedDependencies`: source, Cargo lock, toolchain, configuration,
  migrations, release build script, and retained official CycloneDX schemas at
  exact digests;
- `builderDependencies`: Jenkins controller and the reviewed controller/input
  definitions plus the attestation trust policy;
- `byproducts`: target manifest, official-schema-validated exact normal-closure
  SBOM, build inputs,
  and full license notices; and
- `metadata.invocationId`: the unique controller build URL.

Verification authenticates DSSE before parsing its payload, validates the
in-toto Statement and SLSA v1 predicate through the official generated
in-toto/SLSA protobuf bindings, requires all policy identities, binds every
subject/byproduct digest, and validates the SBOM against the byte-pinned
official CycloneDX 1.6 schemas.
