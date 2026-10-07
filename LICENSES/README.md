# Retained third-party license texts

`ICU.txt` is the approved complete text for locked `decnumber-sys 0.1.6`, as
recorded by ADR-0008.

The sandbox runtime also brings the conformance composition package into the
distributed dependency closure. Four locked MIT monorepo packages omit their
root license text from their published archives:

| Retained text | Packages | Upstream commit and path |
|---|---|---|
| `jsonschema-MIT.txt` | `jsonschema-regex 0.52.1`, `jsonschema-value 0.52.1` | `Stranger6667/jsonschema`, `94546ceb734c6076e73c4a6723de98804ad63ae6`, `LICENSE` |
| `simd-MIT.txt` | `uuid-simd 0.8.0`, `vsimd 0.8.0` | `Nugine/simd`, `d74c030d9dc4f3cae02146d1f497ff62726ef09a`, `LICENSE` |

The generator checks exact package names, versions, declared MIT license,
repository, published archive VCS identity and retained text SHA-256. It
includes these complete texts in executable bundles. New or changed package
identities require their own source review; this is not a generic license
fallback. Provenance is retained in `tools/sandbox/inputs.json`.
