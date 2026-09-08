# ADR-0008: Retain decNumber under the ICU License

Status: **Accepted by repository owner**
Date: **2026-09-08**

## Context

The interpreter's decimal implementation uses the locked `dec 0.4.11` crate,
which links `decnumber-sys 0.1.6`. The latter declares the OSI-approved `ICU`
license and carries decNumber copyright and permission notices. Before this
decision, `deny.toml` did not allow ICU, so `cargo deny check` failed, and the
release receipt generator listed SPDX expressions without shipping complete
license and notice texts.

The repository owner explicitly requested closure of the pre-0.9 review
findings, including retaining and documenting this dependency. This ADR records
that repository distribution decision. It is not a representation that
external legal advice was obtained.

## Decision

- Retain the exact locked `dec 0.4.11` and `decnumber-sys 0.1.6` dependency
  chain.
- Allow the `ICU` license in `deny.toml`; any additional ICU-licensed package,
  version change, or changed license text requires a new review.
- Track the complete Apache-2.0 project license, the project `NOTICE`, and the
  exact ICU text shipped by `decnumber-sys 0.1.6`.
- Select the Apache-2.0 option for the exact locked `crc-catalog 2.5.0` package,
  whose published crate contains no standalone license file; a version or
  expression change fails closed for renewed review.
- Generate `LICENSES.md` for each release target from target-filtered Cargo
  metadata. The closure begins at the server and CLI, follows normal and build
  dependencies, excludes development-only dependencies, and includes every
  discovered full license and notice file. Generation fails closed if a
  production dependency lacks complete legal text.
- Bind the generated notice digest and production-closure counts into release
  metadata before any target receipt is written.
- Run `cargo deny check` as a blocking Jenkins gate for pull requests, full
  assurance, and release-tag builds.

## Consequences

- Source and binary distributions carry the project terms and the attribution
  text required by the retained decimal dependency.
- Dependency-policy drift and missing legal text stop CI or release generation
  instead of producing an incomplete receipt.
- This decision does not broaden dependency approval beyond the exact locked
  ICU component and does not change SBOM or provenance conformance claims.
