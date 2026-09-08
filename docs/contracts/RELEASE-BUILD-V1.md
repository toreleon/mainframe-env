# mainframe-env target release build type v1

Status: **Implemented**
Owner: **release and supply-chain maintainers**
Scope: **target release build inputs, outputs, and attestation contract**
Applies from: **mainframe-env 0.8.3 development**

Type URI:
`https://github.com/toreleon/mainframe-env/blob/main/docs/contracts/RELEASE-BUILD-V1.md`

This build type produces the CLI and server for one explicit supported target.
It runs the repository's canonical release build with the `release` profile,
all features, and locked Cargo resolution, then executes the exact target
binaries' version/help and server-readiness probes before generating receipts.

## External parameters

The complete external interface is:

- `source`: the `git+https` repository URI at one 40-hex commit;
- `version`: the exact release version;
- `target`: `aarch64-apple-darwin` or `x86_64-unknown-linux-gnu`;
- `profile`: `release`;
- `allFeatures`: `true`; and
- `locked`: `true`.

No other external parameter is accepted. `source` resolves to the source-tree
descriptor and the exact configuration, lock, migration, toolchain, build
script, and retained CycloneDX schema descriptors in `resolvedDependencies`.

## Internal parameters and outputs

The trusted local controller supplies `invocationUri` and the digest of its
reviewed CI-input lock. The build emits two binary subjects. Manifest, SBOM,
build-input, and license reports are signed provenance byproducts. The SBOM is
CycloneDX 1.6 over the exact target-filtered normal dependency closure of the
CLI and server, including dependency edges and excluding build-only and
development-only packages. The full license report independently retains the
broader normal/build closure needed for distribution compliance.

Initiate the build only through a release-tag Jenkins run. The controller binds
the unique Jenkins `BUILD_URL`, injects the file credential named
`mainframe-env-release-ed25519-pkcs8` only into receipt generation, and verifies
the DSSE envelope against `config/release-attestation-policy.json` before any
receipt is written.
