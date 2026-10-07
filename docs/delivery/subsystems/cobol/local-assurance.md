# mainframe-env cobol.execution local COBOL assurance matrix

Status: **pass-with-licensed-differential-pending**

## Gate coverage

The pinned denominator contains 173 official COBOL rows. Of those, 153 are
executable programming rows: 44 procedure statements, 82 intrinsic functions,
17 data-description clauses, and 10 file-description clauses. The remaining 20
compiler directive rows are structural and have no runtime behavior. The shared
Conformance IR gives every executable row normal execution, quantum identity,
resource exhaustion, cancellation-without-mutation, and checkpoint identity
obligations. Focused condition obligations additionally cover arithmetic size
and divide-by-zero errors, program and object-call exception, file end/invalid
key, STRING and UNSTRING overflow, malformed JSON/XML, a dynamic-length limit,
invalid NUMVAL forms, and invalid intrinsic domains. Focused restart
obligations cover bounded dynamic storage,
SEARCH index state, in-memory SORT/RELEASE/RETURN state, out-of-line PERFORM
repetition, XML processing-event progress, declarative error-handler call
frames, and stateful RANDOM sequence progress.

Special registers are not included in the 173-row denominator because the
pinned catalog gives those 28 descriptors no `row_id`. They have a separate
exact runtime fixture set and contribute no official row credit.

The local obligation inventory contains 412 executed verdicts, 332 conditioned
verdicts, and 162 recovered verdicts. The most recently run ledgers pass at
412/412 executed, 332/332 conditioned, and 162/162 recovered. The recognized
and validated structural denominator
remains 173/173 at each gate. Differential
coverage remains 0/153 pending a licensed receipt.

## Deterministic bounds and normalization

- COBOL decimal intermediates are limited to 34 digits. Receiver PICTURE limits,
  scale truncation, `ROUNDED`, packed/zoned/binary encoding, and atomic
  `ON SIZE ERROR` assignment staging remain owned adapters.
- `OCCURS ... UNBOUNDED` has at most 4,096 runtime occurrences and at most
  16 MiB of backing storage per unbounded table. The current `DEPENDING ON`
  value is checked on every subscript operation.
- Dynamic items use their declared `LIMIT`; their live extent is checkpointed.
- Output, storage, steps, effects, frames, sort records, function arguments,
  fixture text, and observation text use the accepted invocation/conformance
  bounds. Exhaustion is typed and never converted to success.
- Each executable fixture is interrupted after one operation and must return
  the typed cancelled category without changing its snapshot. The same fixture
  must also finish with byte-identical terminal state under one-step and wide
  execution quanta.
- Alphanumeric comparisons and sort keys use the owned CP037 collation key.
  Dataset `CODE-SET EBCDIC` normalizes to CCSID 37 at the provider boundary.
  National data is normalized as UTF-16BE and UTF-8 data remains validated UTF-8
  bytes. DBCS storage retains explicit two-byte code units; licensed code-page
  interaction results remain part of the differential campaign.
- Clock and compile time are explicit invocation bindings. The runtime uses no
  ambient wall clock. Intrinsic fixtures compare one-step and wide-quantum runs.
- JSON uses a bounded standards parser/serializer with nested groups, fixed and
  ODO-governed array bounds, explicit UTF-8/CP037 encoding, and owned
  name/suppression/null-conversion adapters. XML generation and parsing support
  bounded nested groups and repeated OCCURS elements, emit depth-first
  processing events; recognize declarations, attributes, namespace
  declarations and prefixes, empty elements, the five predefined entities, and
  decimal/hexadecimal numeric character references; and update the XML
  namespace registers for each event. Schema-validation variants outside the
  frozen row form remain licensed campaign inputs and are not inferred from
  these focused cases.

## Durable-state compatibility

Checkpoint schema `mainframe-env.reference-machine-checkpoint@10` preserves
dynamic extents, implicit/special-register values, SEARCH results, SQL cursors,
SORT workspaces, active sort procedures, sort provider progress, bounded heap
allocations, freed allocation identities, and linkage pointer aliases. Schemas
1–9 remain readable with deterministic defaults for fields they did not
contain. Schema 10 additionally preserves the run-unit RANDOM generator state.
Malformed, oversized, duplicated, out-of-range, or trailing checkpoint data is
rejected. Legacy migration fixtures and schema-10 dynamic/sort/heap/RANDOM
restart tests run locally.

## Licensed oracle boundary

The approved portable reference campaign is separate from the licensed oracle.
`conformance/subsystems/cobol/execution/cobol/gnucobol-reference-allowlist.json` contains 16 bounded
cases: three statement/control, two exact small-decimal, four MOVE/string, and
seven portable intrinsic cases. Every case records a portability rationale and
the pinned IBM row/source locator. `cargo xtask cobol-reference --check
--receipt /absolute/external/path.json` verifies and runs the exact installed
GnuCOBOL 3.2.0 compiler and runtime out of process under `-std=ibm-strict`, free
source format, UTF-8 source/output assumptions, and `C.UTF-8`. It compares the
normalized GnuCOBOL result with a separately compiled/executed mainframe-env
observation and the reviewed expected output. Four focused mutants prove that
reference-output, product-output, exit-status, and generic-success bypasses are
rejected.

The external receipt conforms to
`conformance/spec/schemas/cobol-gnucobol-reference-receipt.schema.json` and
records installed/resolved tool paths, compiler/runtime binary and version
output digests, flags, locale, encoding assumptions, source format, fixture and
source digests, compile/runtime/product exit statuses, normalized stdout and
stderr, bounds, comparison results, and the complete candidate identity. The
adapter fails closed on a missing or drifted tool. No production manifest links,
imports, or packages GnuCOBOL or `libcob`.

The adapter policy is
`conformance/subsystems/cobol/execution/oracles/cobol-licensed-differential.json`. It requires IBM
Enterprise COBOL for z/OS 6.5 plus Language Environment, recorded compiler and
runtime options, candidate/spec/fixture digests, normalization rules, and
positive, negative, boundary, condition, and interaction cohorts. Its status is
`pending`. A generated result, historical output, or local reference-runtime
result cannot satisfy the differential gate.

The shared IR contains 153 `licensed-equivalence/differential` cases. The
external receipt must list the exact official row ID, runtime fixture ID,
normalized observation digest, and all five passing cohorts for each case.
Fixture identity is `sha256` over the statement, function, data, and file
runtime fixture files in that order, each prefixed by its big-endian `u64` byte
length. Candidate and spec digests must match the unchanged run. Selecting the
differential gate without the receipt emits 153 explicit failing verdicts; it
cannot pass as an empty selection.

The GnuCOBOL campaign is labeled `reference=gnucobol`, contributes zero
licensed differential credit, and cannot satisfy or alter those 153 cases. The
real Enterprise COBOL 6.5 campaign remains exactly 0/153 pending and is handed
to the certification.licensed release-certification hard gate.

After normally merging accepted racf.security security and dataset.data dataset authorities, the
checked-in shared spec contains 463 rows, 2,273 obligations, 2,621 bindings,
and 1,604 fixture identities. The compiled repository IR contains 2,304
obligations and 2,776 bindings after generated catalog bindings are included.
The integration leaves the COBOL denominator and its 153 licensed cases
unchanged and passes the complete workspace and full-regression suites.
