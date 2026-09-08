# Licensed CICS file/UOW pilot handoff

Status: adapter ready for a protected runner; licensed campaign not run.
`differential` remains pending. This handoff does not certify CICS, close #15 or
#23, or grant whole-command credit.

## Protected boundary

The importer trusts one authority identity,
`ibm-cics-protected-runner`, and an Ed25519 public key supplied to the import
process as `MAINFRAME_ENV_CICS_ORACLE_PUBLIC_KEY`. The corresponding private key
must remain in the protected licensed runner. A capture's `kind`, `authority`,
run/job name, `licensed`-looking metadata, hashes, or receipt digest are not
evidence without a valid signature from that key. Local, model, synthetic and
GnuCOBOL captures can test the adapter contract but always receive zero licensed
credit.

## Environment prerequisites

Record the exact CICS TS and Enterprise COBOL versions and maintenance, compiler
options, CP037 configuration, local recoverable KSDS definition, two-byte key,
four-byte fixed record, principal and SAF resource rules, and initial `AA11`
record bytes. Serialize those values into a bounded environment manifest and
hash its canonical bytes. The digest must equal the value frozen into the local
handoff for the candidate; a generic `6.x` documentation URL is not an
environment identity.

Use a disposable file/resource name owned by the campaign. The runner must have
only the authority required to compile, install, execute, read back, and delete
that resource. Do not put credentials, tokens, certificates, customer records,
job logs containing secrets, or unredacted system configuration in the capture.

## Execution and capture

1. Pin the candidate commit, compiled Conformance IR digest, accepted rule/source
   digest, fixture digest, comparison-policy version, and environment-manifest
   digest.
2. Create the disposable recoverable KSDS and seed `AA11` through supported CICS
   resource and data-management paths.
3. Compile and run the seven scoped scenario programs named in
   `conformance/0.9/oracles/cics-licensed-differential.json` through the licensed
   Enterprise COBOL and CICS entry points.
4. Capture application-visible output and read the business record back after
   each mutation/commit/rollback boundary. Internal traces are optional
   diagnostics and are not compared.
5. Serialize `mainframe-env.cics-oracle-capture@1`, compute
   `raw_capture_digest` over the ordered observation array, and sign the
   canonical identity-and-digest payload with the protected runner key.
6. Transfer only the bounded capture. Remove the disposable CICS resource and
   data set through supported cleanup paths whether the run passes or fails.

## Import and comparison

The importer first checks size and schema, exact scenario closure, raw-capture
digest, candidate/spec/fixture/source/environment identity, authority identity,
and Ed25519 signature. It then maps the application output and record hex into
the same typed comparison boundary used by the local pilot. Missing,
malformed/truncated, wrong-candidate, wrong-spec, wrong-environment, unknown,
duplicate or forged captures fail before a verdict can be emitted.

Offline Rust tests exercise valid local/model captures, malformed and missing
observations, identity mismatches, protected signatures, and forged licensed
metadata. Those tests validate plumbing only and grant zero IBM credit. A live
import must retain the protected run/job identity and raw artifact digest in the
shared verdict boundary. Until that import occurs, report exactly: **adapter
ready; licensed campaign not run; differential pending**.
