# CICS — SPI and FEPI progress

Subsystem: **cics**
Phase: **system-api**
Target release: **0.10.0**

Status: **SPI-1001 identity foundation retained; command-body pins registered;
row maps, semantic review and application dependency pending; 0.10.0 remains Proposed**

- Branch: `codex/v010-spi-fepi-completion-20261002`
- Dependency candidate reviewed: `213ed878ec138bdb2914330db6613559bffc5a86`
- Historical identity-foundation base: `5ab706b1dd069e26db7cb9a2b66e921c9001fc39`
- Dependency disposition: 0.9.0 remains Proposed. Its shared CICS authorities are
  available for private 0.10 catalog/code-generation preparation, but no 0.10
  runtime integration or public route is permitted.
- Official baseline: `ibm-cics-ts-6x-2026-08-31`
- Official catalog SHA-256:
  `fccd2a8e5cc24dd08aeb32754daf14ed80e9f1b20b5d9e762a1b0cfe429ceeba`
- IBM topic: `SSJL4D_6.x/reference-diagnostics/eib/dfha8mf.html`
- IBM topic SHA-256:
  `78f90b09987b1a56da7cd9f0a2a36fa43a549966fa7dbeff2ad9608106ef7c25`
- IBM TOC SHA-256:
  `f65c51e52facc390c05f084e1d249ff19e68bf2d7f8d3f32d4d745faf622681a`

## 2026-10-02 continuation: prerequisite review

The manager integration checkout is
`/Users/tore/code/.codex-worktrees/mainframe-env/v010-completion-20261002`,
branch `codex/v010-spi-fepi-completion-20261002`, based on fetched
`origin/main` commit `213ed878ec138bdb2914330db6613559bffc5a86`.
The earlier slice receipts below retain their original candidates. They are
not current semantic or application-dependency acceptance evidence.

CLI preflight verified installed `codex-cli 0.160.0`, model-list availability
of `gpt-6.1-sol` with `high` reasoning, and explicit `features.fast_mode=false`.
Workers use `service_tier="default"`, the current CLI account, pinned host
Python 3.12.13 and isolated branches from the exact fetched candidate. The
initial wave has three bounded read-only review slices, below the eight-worker
ceiling. Workers may write only their named external handoff directory; no
worker owns a shared repository file or may spawn another worker.

| Slice | Exact scope and obligations | Dependencies | Exclusive ownership | Acceptance gate |
|---|---|---|---|---|
| `SPI-1001.spi-source-review` | `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0001`–`0269`; establish row-to-command-topic locators from the pinned TOC and review hash-verified retained bodies; enumerate missing or ambiguous authority without deriving semantics | Frozen catalog and retained EIBFN/TOC pins | External `v010-20261002/spi-source/` handoff only | Offline search/read; exact locator/hash/byte findings; all 269 rows accounted for; no execution or coverage credit |
| `SPI-1001.fepi-source-review` | `ibm-cics-ts-6x-2026-08-31:fepi-commands:0001`–`0039`; establish distinct command-topic locators and hash-verified retained-body availability; preserve shared-EIBFN command distinctions | Frozen catalog and retained EIBFN/TOC pins | External `v010-20261002/fepi-source/` handoff only | Offline search/read; exact locator/hash/byte findings; all 39 rows accounted for; no execution or coverage credit |
| `SPI-1006.dependency-review` | Read complete application progress and verify actual `cics.application-api` acceptance, participant/SAF/resource/condition/recovery owners, selected-route evidence and licensed environment disposition | Fetched candidate and existing accepted receipts | External `v010-20261002/dependency/` handoff only | Independent evidence/path/candidate review; identify unpassed prerequisite and smallest next action; no unrelated 0.9 implementation or re-certification |

The next serialized manager slice is `SPI-1001.command-body-pins`. Its
inventory gap is that the shared offline reader cannot select any SPI/FEPI
command-body scope at the fetched base. It owns only the two target topic
manifests and their index, the existing reader's bounded registry list and
its tests, the shared xtask manifest checker, source-cache documentation,
this status record, a unique changelog fragment and derived documentation.
Its dependencies are the independently reviewed worker locator/body reports
and the exact pinned CICS TOC. Acceptance requires matching retained topic
bytes, committed manifest/registry hash and count consistency, positive and
negative scope-reader tests, shared topic-manifest/schema checks and mandatory
format/docs/changelog/dependency policy. It grants no grammar, execution,
public-route, parent-package or licensed credit. Any unresolved row-to-topic
review remains explicit for the following source-map slice; registering a
bounded command cohort cannot substitute for that review.

The manager exclusively owns the status document, source manifests/mappings,
schemas, generators, registries, shared contract facades and integration.
Semantic obligation IDs and contexts must come from reviewed command bodies;
none are invented for these zero-credit source/evidence reviews. No new public
route or backend mutation is assigned. Waves B/C wait for reviewed command-body
authority, SPI-1001 contracts and the application integration prerequisite;
Wave D waits for one complete unchanged integration candidate. Licensed
`differential=pending` remains binding until actual licensed evidence passes.

## Retained command-body pin slice

`SPI-1001.command-body-pins` extends the existing shared offline reader and
xtask manifest checker with the 0.10 registry. The earlier unknown-scope
inventory gap is repaired. All selected HTML came from the retained CICS
archive run `20260912T054102Z-0a2bbb55`; its product, pinned TOC, exact content
URL, topic paths, byte counts, body hashes, headings and published last-updated
values were checked locally. No network or browser refresh occurred.

| Registered scope | Topic count / bytes | Topic-set SHA-256 |
|---|---|---|
| `cics-spi-command-bodies` | 277 / 12,152,388 | `84b948d0ae566171c52b907939e933ff33bafe43460217a6a17258a5292b7609` |
| `cics-fepi-command-bodies` | 36 / 1,351,861 | `86de6d98be911824c2cccc9933d87d296cb3f77973b6ee1a5762e22910e95bc2` |

The manifests are `conformance/0.10/manifests/cics-spi-command-topics.json`
and `cics-fepi-command-topics.json`, bound by `manifests/index.json` under the
shared zero-credit registry contract. Source-set baselines are
`ibm-cics-ts-6x-spi-command-bodies-2026-09-12` and
`ibm-cics-ts-6x-fepi-command-bodies-2026-09-12`. The source-only body/TOC import
and reader search/read reproduce through the external task cache. This is
bounded pin verification, not whole-archive completeness, fresh browser
reproduction, semantic acceptance or licensed evidence.

The reviewed external worker proposals account for all 269 SPI rows using
267 distinct body candidates, with three qualified PERFORM equivalences still
ambiguous: rows 0201 SECURITY/REBUILD, 0203 SSL/REBUILD and 0204
STATISTICS/RECORD. The SPI manifest retains the whole 277-topic System commands
cohort; its ten additional topics grant no catalog row or behavioral credit.
All 39 FEPI rows have 36 candidate bodies: list rows 0033 NODELIST, 0035
POOLLIST and 0037 TARGETLIST share their respective SET command pages by explicit
body syntax/options, preserving their distinct official identities. Row maps
must still be registered and independently checked through shared source-map
tooling before grammar/semantic work. Publication bodies remain outside Git.

Focused validation passes: 25 offline-reader tests including scope/count and
negative target-version binding, nine module-boundary tests, the shared xtask
manifest test (including Draft 2020-12 validation of every new manifest/index),
`cargo xtask schemas --check`, formatting, dependency policy, generated docs
freshness and changelog validation. The exact artifact allowlist is sealed by
the shared work-package generator before the dependent source-map slice. Source-set counts
never replace **269 SPI / 39 FEPI**; every behavioral gate remains **0/269 and
0/39**, with `differential=pending`.

## Current dependency review

The application progress record was read completely at the exact fetched
candidate. Actual generated registrations remain **260 typed / 0 legacy /
3 unready**: application rows 0027 CICSMESSAGE, 0093 GETNEXT TIMER and 0114
ISSUE COPY. The shared Conformance IR contains no CICS row bindings at this
base, and there is no accepted CIC-906/final application candidate receipt.
Readiness and historical slice tests do not pass `cics.application-api`.
Runtime integration and advertisement remain closed; private source/contract
preparation can proceed without expanding this goal into unrelated 0.9 work.

Licensed CICS environment identity, protected runner key/attestation and full
fixture closure remain unavailable in the checked-in harness disposition.
Synthetic fixtures grant no licensed credit. The dependency audit also found
that inherited completion trailers for `INT-1601.compatibility-tests` at
`d7738831e328de489f0e49645733689be7636643` and
`SPI-1001.generated-registry` at
`a5bfb33090a3b8bf84c4c31d4771afeb836102d2` do not reproduce under the current
shared sealer algorithm; their historical completion claims need reconciliation
before being consumed as acceptance. This is a receipt finding, not a runtime
failure or permission to reopen unrelated work. The current identity bytes
and retained EIBFN table still match the frozen denominator. Existing receipts
below remain historical and are not credited to this candidate.

## Objective and boundary

Seal the dependency-safe SPI-1001 identity foundation for all 269 unique SPI
and 39 FEPI command identities. This slice may establish source identity,
deduplication, schemas, generated catalogs, and a non-routing Rust registry.
It must not infer grammar or behavior from command labels or EIBFN values,
register handlers, advertise routes, mutate CICS state, or claim SPI-1001 or
0.10.0 complete.

The 0.9 application registry, dynamic EIBRESP condition-name authority,
resource/security policies, canonical effects, durable coordinator,
persistence, storage, response mapping, and recovery ownership remain the only
accepted shared authorities. The 0.10 foundation references those authorities;
it does not copy or replace them.

## Stable slices

| Slice | State | Dependency | Acceptance boundary |
|---|---|---|---|
| `SPI-1001.source-authority` | Pass (`e514061dc320566b735490be7a54c8d11d821ef8`) | Frozen 0.2 CICS catalog; exact retained IBM EIBFN topic and TOC | Verified topic/TOC bytes; preserves 273-to-269 SPI label deduplication, all alternate EIBFN identities, 39 FEPI rows, and the exact source normalization; every unproved semantic dimension is blocked with zero coverage credit |
| `SPI-1001.catalog` | Pass (`789c6079c02a6581533431ceaac2e16c74396a76`) | `SPI-1001.source-authority` | Generated exactly 269 SPI and 39 FEPI identity records from the frozen catalog and reviewed source disposition; schema, freshness, malformed/foreign/duplicate/mutation checks pass; no handlers or routes |
| `SPI-1001.generated-registry` | Pass | `SPI-1001.catalog` | Generated and compiled a typed non-routing registry; exact denominator and digest checks pass; every entry is identity-only, unadvertised, unregistered, and zero-credit; the 0.9 registry/runtime surface is unchanged |

The parent `SPI-1001` remains pending. Its required grammar, options, resource
schemas, condition mappings, lifecycle/context matrix, authorized intent,
audit effects, concurrency, lock order, syncpoint, quiesce/drain, restart, and
recovery bindings cannot be closed from the EIBFN inventory table.

## Source review

Offline `ibm_docs.py search` and `read` verified the pinned topic and TOC. The
topic states that EIBFN identifies the most recently issued command and that
Table 3 lists SPI command names/function codes while Table 4 lists FEPI command
names/function codes. The retained HTML at
`/Users/tore/Library/Caches/mainframe-env/ibm-docs-archive/raw/html/sha256/78/78f90b09987b1a56da7cd9f0a2a36fa43a549966fa7dbeff2ad9608106ef7c25.html`
is 265,761 bytes and matches the committed SHA-256. The retained TOC at
`/Users/tore/Library/Caches/mainframe-env/ibm-docs-archive/raw/toc/sha256/f6/f65c51e52facc390c05f084e1d249ff19e68bf2d7f8d3f32d4d745faf622681a.json`
also matches its committed SHA-256.

Table 3 has 273 raw rows and 269 unique normalized command labels. The four
duplicate labels retain both source EIBFN identities: `INQUIRE NETNAME`
(`5216`, `5206`), `INQUIRE SYSTEM` (`5402`, `5412`), `INQUIRE TERMINAL`
(`5202`, `5212`), and `SET TERMINAL` (`5204`, `5214`). The frozen denominator
uses the first table occurrence as its canonical row. Source row
`spi-commands-unique:0192` prints `70 32`; the accepted catalog normalization
removes only that embedded whitespace to retain `7032`. Table 4 has 39 raw and
39 unique command labels. Shared EIBFN values in FEPI remain distinct official
command identities and are never deduplicated by code.

## Historical source gaps and current semantic boundary

The identity-only authority retains its original
`SPI-1001.source-gap.spi-command-bodies` and
`SPI-1001.source-gap.fepi-command-bodies` dispositions; it and the generated
registry were not rewritten to confer semantic authority. The retained-body
pin slice above establishes the new registered source sets separately.
Reviewed 0.10 row maps, qualified-form decisions and explicit semantic context
closure remain pending. EIBFN names/codes and body availability cannot establish
grammar, conditions, resource lifecycle, authorized intent or recovery.

## Results

`SPI-1001.source-authority` is complete at its identity-only boundary. The
focused Python suite passes 7/7 positive, negative, and mutation tests. The
cache-backed verifier passes against the exact retained HTML and TOC paths
above. `cargo xtask schemas --check` compiled the new Draft 2020-12 schema and
accepted the mapped authority instance.

`SPI-1001.catalog` is complete at its identity-only boundary. Its deterministic
generator emits 308 ordered rows with logical identity digest
`sha256:5f4867c8a3973c9344e62288ced4c0be6d45356bcc506638e0a6822fdfc333da`;
the generated JSON file SHA-256 is
`d9045ff97496e2eac27ba0706ab186382e4186d60afb5650d62ffdf12bd2da4d`.
The combined focused source/catalog suite passes 15/15 positive, negative, and
mutation tests. `cargo xtask schemas --check` compiled both new Draft 2020-12
schemas and accepted both mapped instances, then stopped later on the unchanged
0.8 `carddemo-base-batch.json` note exceeding its existing 256-character schema
cap. This is an inherited, unrelated gate failure and is not relabeled as a
pass.

`SPI-1001.generated-registry` is complete at its non-routing boundary. The
generated Rust registry carries the same logical identity digest
`sha256:5f4867c8a3973c9344e62288ced4c0be6d45356bcc506638e0a6822fdfc333da`;
its file SHA-256 is
`7980237b5e2a7a11953d3fc43e247ddb493113194803fe0ea7f450b44488f133`.
It exposes official-row and ambiguous EIBFN lookup only; there is no label-token
dispatcher, handler identity, runtime operation, advertisement, automatic
registration, or public route. The complete `mainframe-env-ir` suite passes
54/54, including the three new registry tests and the unchanged 0.9 application
registry closure. Package Clippy passes with warnings denied. The combined
source/catalog generator suite passes 17/17, including an advertisement
mutation that the Rust generator rejects.

The broader architecture checks expose three inherited candidate failures in
files untouched by SPI-1001: the production hardcode scan finds `CARDDEMO` in
`crates/providers/mainframe-env-dataset/src/replay_index.rs`; the module budget
records 1,321 lines for the now-1,345-line
`crates/tooling/mainframe-env-conformance/src/cics_pilot.rs`; and the public API
documentation ratchet records 1,129 undocumented host-API items while the
candidate emits 1,130. The schema gate's inherited 0.8 receipt-length failure
is recorded above. These unchanged failures are not retried or relabeled as
SPI-1001 passes.

Coverage, semantic, execution, condition, recovery, and differential credit
remain **0/269 SPI and 0/39 FEPI**. The 0.9 public/runtime surface is unchanged.

## Next executable step

Register and independently verify the 269-row SPI and 39-row FEPI maps through
the shared source-map owner, retaining the three SPI PERFORM ambiguities until
resolved from the now-pinned bodies. Declare bounded source-required context
closure, then derive private SPI-1001 grammar/options/resource/condition and
lifecycle contracts without routing or advertisement. Application acceptance
and licensed gates remain prerequisites to their dependent integration waves.
