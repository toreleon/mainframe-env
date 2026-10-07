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
3. Compile and run the twelve obligation-scoped observations named in
   `conformance/subsystems/cics/application/oracles/cics-licensed-differential.json` through the licensed
   Enterprise COBOL and CICS entry points. Shared setup/program execution is
   allowed, but the capture must emit one exact observation per name. The
   durable-restart observation includes all three declared fault boundaries.
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
digest, candidate/spec/fixture/source/environment identity, and every captured
application output and record against the reviewed expectations in
`pilot-fixtures.json`. Licensed origin additionally requires the protected
authority identity and Ed25519 signature. A valid signature with mismatched
behavior does not grant credit. Missing, malformed/truncated, wrong-candidate,
wrong-spec, wrong-environment, unknown, duplicate, behavior-mismatched or forged
captures fail before a verdict can be emitted.

Offline Rust tests exercise valid local/model captures, malformed and missing
observations, identity and behavior mismatches, protected signatures, and forged
licensed metadata. Those tests validate plumbing only and grant zero IBM credit. A live
import must retain the protected run/job identity and raw artifact digest in the
shared verdict boundary. Until that import occurs, report exactly: **adapter
ready; licensed campaign not run; differential pending**.

Run the importer from the exact candidate checkout. Both capture and protected
public key stay outside the repository; the key file is the raw 32-byte
Ed25519 public key:

```text
cargo xtask cics-oracle \
  --capture /protected-handoff/cics-pilot-capture.json \
  --public-key /protected-handoff/cics-oracle-ed25519.pub
```

For a local/model/synthetic adapter-contract fixture, omit `--public-key`. The
command then reports `licensed-credit=0 differential=pending`. It derives the
candidate, compiled-spec, fixture, accepted-review, comparison-policy and
environment identities from the current checkout; a capture for any other
candidate fails instead of being normalized or re-bound.

## Bounded BIF family extension

The sealed `bif-builtins-v1` manifest at
`conformance/subsystems/cics/application/oracles/cics-licensed-family-bif-builtins-v1.json` covers only
catalog rows `0013 BIF DEEDIT` and `0014 BIF DIGEST`. Its six named observations
and exact UTF-8 application output and output-field bytes are in
`cics-bif-builtins-fixtures.json`. The `record_hex` capture field means the
post-command `FIELD` or `RESULT` bytes for this family; it is empty for the two
condition observations. The DEEDIT example and LENGTH<1 condition come from
IBM CICS TS 6.x `dfhp4_bifdeedit.html`; the three SHA-1 formats and RECORDLEN<1
condition come from `dfhp4_bifdigest.html`. Their baseline, catalog rows, and
verified hashes are in `cics-bif-builtins-source-review.json`. These are
source-derived comparison expectations, not a licensed run result.

The protected runner needs an exact, external environment manifest with schema
`mainframe-env.cics-oracle-environment@1` and **only** these nonempty string
fields: `cics_version_and_maintenance`,
`enterprise_cobol_version_and_maintenance`, `compiler_options`, `encoding`
(including CP037 and transport conversion), `cpacf_msa_availability`,
`program_and_transaction_definition`, `principal_and_saf_configuration`, and
`capture_serialization_version`. Record exact values, not `6.x` placeholders.
The host needs licensed CICS TS and Enterprise COBOL, a runnable transaction,
the authorized principal, and CPACF MSA for BIF DIGEST. Keep credentials and
unredacted configuration outside both manifest and capture. The environment
file and 32-byte Ed25519 public key stay outside Git.

Run all six observations on the **same exact candidate** through Enterprise
COBOL and CICS. DEEDIT uses the documented nine-byte `14-6704/B` field and a
zero LENGTH negative case. DIGEST uses the literal three bytes `X'616263'`,
RECORDLEN 3 for HEX, BINARY and BASE64, and RECORDLEN 0 for LENGERR. Each
application emits the fixture's exact UTF-8 line after explicit transport
conversion and returns raw field bytes as lowercase hex (binary DIGEST remains
binary). Capture one
observation per name in manifest order. Serialize
`mainframe-env.cics-oracle-capture@2` with `family_id=bif-builtins-v1`,
`family_manifest_digest` equal to the SHA-256 of the manifest file, and exact
candidate, compiled-spec, fixture, source-review, and environment byte digests.
Hash the canonical ordered observation array as `raw_capture_digest`. Sign the
canonical identity-and-digest payload, including family ID and manifest digest,
with the protected runner's Ed25519 private key. The runner must retain its
run/job ID and raw capture artifact; the private key never leaves it.

From that exact checkout, import the bounded capture with:

```text
cargo xtask cics-oracle --family bif-builtins-v1 \
  --capture /protected-handoff/cics-bif-capture.json \
  --environment-manifest /protected-handoff/cics-bif-environment.json \
  --public-key /protected-handoff/cics-oracle-ed25519.pub
```

The importer checks manifest/fixture closure, exact behavior and all identity
digests before Ed25519 authority; missing, duplicate, unknown, malformed,
tampered, wrong-candidate, wrong-environment and behavior-mismatched captures
fail. Local/model/synthetic contract fixtures may omit `--public-key` and
always get zero licensed credit. A valid signed family import grants at most
one **scoped family capture** result; it does not complete either whole command
or the full-minor licensed gate. No real protected runner or signed capture is
available in this checkout: **adapter ready; licensed campaign not run;
differential pending**.

The remaining miscellaneous rows `0031`, `0070`, `0074`, `0147`, `0206`,
`0207`, `0239`, and `0256`, all BTS child/LINK and conversation-open rows,
the other executable command families, all unready rows, and the integrated
263-command final-candidate campaign remain outstanding. Each needs its own
source-reviewed bounded manifest and protected environment/runner evidence.
