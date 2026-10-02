# CICS — SPI and FEPI progress

Subsystem: **cics**
Phase: **system-api**
Target release: **0.10.0**

Status: **SPI-1001 identity foundation retained; command-body pins sealed;
source-map candidates and nine FEPI context pins prepared with qualified-row gaps; semantic review,
shared gates and application dependency pending; 0.10.0 remains Proposed**

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

The user explicitly requested resolution and continuation after the blocked
handoff. The resumed audit starts afresh. A targeted retained-cache locator
search found both previously missing sources-a bodies in
`/Users/tore/Library/Caches/mainframe-env/ibm-docs-pinned-0.9`; their committed
hashes/byte counts match and the shared importer restored them to the task
cache without network access. The prior absence finding concerned the two
original retained roots; it is superseded by this matching additional cache.

The resumed bounded slices are:

| Slice | Exact scope / dependency | Exclusive ownership | Acceptance |
|---|---|---|---|
| `SPI-1001.source-cache-recovery` | The two committed sources-a connection/ASSIGN context bodies; exact retained pins | Manager-owned external task cache, this status and derived docs | Shared import, scoped search/read and repaired source gate; no repin or licensed credit |
| `SPI-1001.qualified-identity-resolution` | SPI rows 0201, 0203, 0204; frozen EIBFN identities, three pinned candidate command bodies, relevant registered CICS reference links and retained TOC ancestors | Existing isolated SPI CLI worker; external `resumed-spi-identity/` only | Independent exact source-identity disposition; distinguish row identity from acceptance of a shorter grammar form; preserve any unproved mapping; no network, repository edits or semantics |
| `SPI-1006.module-prerequisite-repair` | Restore the inherited batch service module below its existing 7,402-line ceiling; unchanged behavior | New isolated module CLI worker; `crates/apps/mainframe-env-batch/src/service.rs` and one narrowly extracted internal helper under `service/` only | Behavior-preserving structure, focused existing regressions, formatting and module gate; no ceiling refresh, CICS/v0.9 semantics or public contract change |

Workers use `gpt-6.1-sol`, high reasoning and fast mode off, with no nested
workers. The manager retains serialized shared schemas/generators, status,
changelog, source registration and integration. The user subsequently requested
"Bypass license CICS": licensed CICS runs are omitted for this goal's
implementation/PR handoff, with `differential=pending` and zero licensed credit.
This is not a repository release certification or a fabricated oracle pass.
Local application, semantic, security, concurrency and recovery gates remain
required; the user waiver does not itself accept incomplete v0.9 behavior.

The batch extraction is checkpointed at `7ab58ce982a3e629df04a9634a0cf5afe662341b`.
Its three focused existing regressions pass in the worker checkout; integrated
candidate verification remains pending. A single scoped module inventory found
ten additional inherited overages. The following disjoint structural prerequisite
slices start from that exact checkpoint; they grant no subsystem behavior credit.

| Slice | Exact affected modules / ceilings | Exclusive ownership | Acceptance |
|---|---|---|---|
| `SPI-1006.module-host-kernel` | Host API canonical/generated.rs 3070/3046 and request.rs 2336/2300; application/package_v2.rs 1498/1314; interpreter/machine.rs 11942/11940 | Isolated CLI worker; those four modules and cohesive private helper children only | Unchanged wire bytes and execution/package behavior; focused existing positive/negative regressions, formatting, independent moved-body review; all extracted modules <=1200 |
| `SPI-1006.module-server-conformance` | Server/product.rs 6291/6008 and conformance/carddemo.rs 13621/13617 | Isolated CLI worker; those two modules and cohesive private helper children only | Existing selected product-route and affected report regressions; no new CICS routing, receipts or CardDemo campaign; unchanged behavior and formatting |
| `SPI-1006.module-ims-mq` | IMS/service/generic.rs 2186/1200, service.rs 2113/1959, tm/service.rs 1264/1200; MQ/service.rs 1477/1331 | Isolated CLI worker; those four modules and cohesive private helper children only | Existing affected provider success/negative/recovery regressions, unchanged subsystem semantics and formatting; every new/non-exempt module <=1200 |

Manager-only integration will lower exact inventory counts after reviewed
extractions, never increase ceilings or create exemptions to hide growth.
Workers cannot edit inventories, schemas, generators, registries, facades outside
their listed modules, changelog or this status. Pure module relocation needs no
unrelated IBM lookup; any semantic change is forbidden. Global module and required
integration gates run after these inputs are repaired. The five separately pinned
source supplements now match in the explicitly authorized existing Chrome session;
their external-cache export/import is still being completed.

The authorized five-topic Chrome retrieval/import is now complete: all 292,725
bytes match the existing pins; shared supplement heading/date checks pass.
No identities or target-product authority changed. The qualified identity
worker's first continuation inspected only 27/224 sources-c topics because the
cache restoration overlapped its search. The manager subsequently restored all
224 exact pins. A further same-session, read-only continuation may repeat the
three bounded sources-c identity searches against that materially repaired input;
if unresolved it may run at most three similarly bounded searches in sources-a/b.
Its rows, ownership and acceptance remain unchanged. New results stay separate
from the partial-cache receipt and still confer no executable grammar credit.

The subsequently authorized CICS TX TRACE supplement also matches its existing
20,495-byte pin (`28e56eef7556509f9a473ae573f6295a56c170ae9a1dd79041c18b15fafd9c83`).
All three repaired application source-review gates now pass. These are source
freshness reviews, not acceptance of application behavior. The structural
prerequisites are checkpointed at `8094971d` after serial review/integration:
batch 3, host/kernel 35, server/conformance 11 and IMS/MQ 79 focused existing
worker tests passed (128 total; zero ignored). Moved bodies and unchanged
expectations were independently compared in integration. Exact module counts
were lowered after reviewed extraction; the package and MQ oversized exemptions
were removed after dropping below 1,200. Combined candidate policy, architecture,
module, format/schema/docs gates and focused integration regressions are pending.
No feature package, application prerequisite or parent is sealed by these
structural checkpoints, and no public SPI/FEPI route is enabled.

The combined candidate at `0c0bfd7d5382c42019bdfa0cb7f9bcf577b6a918` passes
46 focused integration regressions, dependency policy, format, schema, docs,
changelog, license notices and source-input policy. Architecture-fast stops at
the effect-encoding guard because a new test-only IMS import precedes production
code and its legacy scanner truncates there. The manager owns a narrow follow-up
to place that import inside the existing test module, lower the honest IMS
inventory count and rerun the affected regression/architecture/module checks;
no scanner relaxation or subsystem semantics are assigned.

The user explicitly extended scope with "Ok finish v0.9 first" in response to
the CICS application prerequisite question. The manager will complete the
`cics.application-api` v0.9 gaps and local integration/recovery acceptance first,
then resume v0.10 SPI/FEPI. This supersedes the earlier prohibition on completing
those prerequisite gaps; the licensed-run waiver remains `differential=pending`,
zero licensed credit. Other subsystem/release work is not implicitly added.

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
body syntax/options, preserving their distinct official identities. The following
source-map candidate section records the new shared-tooling projection and its
remaining gaps. Publication bodies remain outside Git.

Focused validation passes: 25 offline-reader tests including scope/count and
negative target-version binding, nine module-boundary tests, the shared xtask
manifest test (including Draft 2020-12 validation of every new manifest/index),
`cargo xtask schemas --check`, formatting, dependency policy, generated docs
freshness and changelog validation. The shared work-package generator sealed
the exact artifact allowlist at `385bd7b3b82329009ff4b16a8ad6f6430e17af87`,
and its committed seal check passes with evidence digest
`sha256:ed52073dfa0879feaa6cfe13d23ec8684d7572795a435456bbb51cc83f2a64ff`.
This source-only slice receipt does not satisfy the aggregate module,
architecture, application or licensed gates recorded below. Source-set counts
never replace **269 SPI / 39 FEPI**; every behavioral gate remains **0/269 and
0/39**, with `differential=pending`.

## Declared source-map continuation

`SPI-1001.command-source-maps` is the next serialized manager slice, dependent
on the sealed command-body pin slice. It owns only the existing shared CICS
source-map generator and schema owner (with a bounded helper and focused
tests), generated 0.10 SPI/FEPI row maps and TOC projections, the shared xtask
freshness binding, source-cache documentation, this status, a unique fragment
and derived documentation. The demonstrated gap is that the generator accepts
only the three 0.9 application batches, with no registered 0.10 row maps.
Acceptance requires all 269/39 frozen rows exactly once, exact TOC and body-pin
bindings, explicit label/form review, canonical digests, independent retained
body verification and mutation rejection; existing application outputs must
remain byte-identical. No routing, semantics or coverage is assigned.

The resumed `SPI-1001.qualified-form-review` worker owns external
`v010-20261002/qualified-forms/` only, on an isolated branch at the sealed pin
candidate. Its exact rows are SPI 0148, 0158, 0181, 0182, 0189, 0200, 0201,
0203, 0204, 0238, 0242, 0259, 0260, 0265 and FEPI 0033, 0035, 0037.
It must search/read the now-pinned topics and verify body anchors, options and
EIBFN markup for each non-exact mapping. Its gate is an independent
source-backed locator disposition, preserving any unresolved equivalence;
it cannot edit repository paths, derive runtime semantics or spawn workers.
The manager retains exclusive shared-contract ownership.

The same review worker may extend its read-only gate to
`SPI-1001.qualified-cross-reference-review`: search only the 277 already pinned
SPI topic bodies for explicit links naming the shorter PERFORM labels. Read
the bounded matching SET DB2CONN, Threadsafe SPI commands, INQUIRE STATISTICS
and SET STATISTICS topics plus the three candidate pages; no whole-cache or
network expansion. Only an explicit pinned link/identity statement can resolve
rows 0201, 0203 and 0204. Findings stay in the external qualified-form handoff.

## Prepared source-map candidates and open gates

The existing `tools/generate_cics_source_map.py` now accepts `--family spi`
and `--family fepi` through a bounded helper, with the same shared schema owner
and xtask bindings. Its default application batches and their committed bytes
remain unchanged. Generated maps preserve all official labels, EIBFN alternates
and distinct FEPI shared-code rows. They do not modify the identity-only Rust
registry or install/advertise any handler.

| Family | Accounted rows | Reviewed mappings / unresolved candidates | Distinct body pages | Mapping SHA-256 |
|---|---|---|---|---|
| SPI | 269/269 | 266 / 3 | 267 | `8192b67692ba0724738a8e024e8f83d72670794dff46bdabbbabd77fb241d70d` |
| FEPI | 39/39 | 39 / 0 | 36 | `e9a0cd2343023a2d2f334eb2ac1eeaec856b79d7acc14ae6c96164e1d49be666` |

TOC/body projection digests are
`sha256:338b3c1cbff4b2424cf0e6b2a0873ab6d813be8b422c0cbaf3fc1afb0bd548f0`
(SPI) and
`sha256:ae30436aebc6600384f5a7f20f6d6fa7e9baed06871f40cb9a9fad2c6f45a150`
(FEPI). The 17 non-exact row reviews retain 96 bounded hash/byte/anchor locators
under `conformance/0.10/cics/command-form-locators.json`; no publication bodies
or semantic rules are embedded. Every selected body, H1 and recorded fragment
reproduces against retained pinned bytes through `--toc` plus `--cache`.

The qualified candidates remain **unresolved**: SPI 0201 PERFORM SECURITY
(`dfha8_performsecurity.html`, SHA-256
`830d0abe3e41d8ded0a29d9692afa52c2466dc4a70f2dccdb5af8b62aabf86f2`),
0203 PERFORM SSL (`dfha8_performssl.html`,
`5179c5ec352f12fd973f0e702bfd8cafa4c70b2b73ac9f69b4182638805daff9`),
and 0204 PERFORM STATISTICS (`dfha8_performstatistics.html`,
`3732a301a401c5720c300be074876fe7f1b1392de3efb0011f6c19259b6b6130`).
All three paths are below
`SSJL4D_6.x/reference-system-programming/commands-spi/` in the registered SPI
source-set baseline. Their H1 and syntax diagrams include REBUILD/RECORD;
the shorter EIBFN row provides no joining link. An explicit pinned
cross-reference or identity statement must establish equivalence before these
rows can supply grammar authority. Prefix/filename similarity grants no credit.
The bounded cross-reference follow-up is complete: five pinned reference topics
and 22 markup fragments yielded no shorter-label bridge. Two statistics links
target the RECORD page but their `#dfha81l` fragment is absent from the retained
body. All three candidate dispositions remain unresolved; no extra unpinned
source dependency or network refresh was introduced.

Nine focused source-map tests pass, including rehashed mutation rejection and
forbidden promotion of a qualified candidate. The existing application mapper
and offline reader tests exercised alongside this change preserve their
expectations. Formatting, Draft 2020-12 compilation/validation of all five new
source artifacts, both retained-body reproduction checks, and unchanged
application-batch freshness pass. The required architecture run passes the
production execution-route, participant, effect, persistence, storage, SAF,
retention, descriptor and all source-map guards before stopping in the existing
application source-review gate. Aggregate acceptance remains open:

- The module gate fails on unchanged
  `crates/apps/mainframe-env-batch/src/service.rs`: 7,404 production lines
  versus its recorded 7,402 non-growing ceiling. File and inventory bytes
  match fetched main; no unrelated budget refresh or batch change was made.
- The architecture source-review gate initially lacked `dfhp37p.html` in its
  configured cache. Its matching archive body was recovered locally. A bounded
  sources-a repair verified/imported 171/173 selected pinned topics and found
  two absent from both retained roots: `SSJL4D_6.x/fundamentals/connections/dfht1c0079.html`
  (SHA-256 `cb6ff139ab0b7bb2832dc03dd4f53672181657f11d0740a417aab84819aaf254`,
  3,031 bytes), and `SSJL4D_6.x/reference-applications/commands-api/dfhp4_codesassign.html`
  (`ed02eedd2e152ff18c4fdc0a0d678cd53ed94e7cd198d369cba71224f3832359`,
  25,657 bytes; application catalog row 0011 ASSIGN). The repaired sources-a
  checker still reports the first missing topic. Restore those exact pinned
  HTML bytes externally, or explicitly authorize a refresh under the cache
  runbook; no refresh or review-receipt regeneration occurred. Both missing
  topics belong to baseline `ibm-cics-ts-6x-application-api-sources-a-2026-09-10`.

`SPI-1001.command-source-maps` is prepared but **unsealed** while these gates
remain open. No generated `Work-Package=pass` trailer is claimed for this
checkpoint. Parent SPI-1001 and Waves B–D remain pending; all behavioral and
licensed credit stays zero.

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

## Declared bounded FEPI context registration

The serialized manager slice `SPI-1001.fepi-context-pins` depends only on
sealed command-body pins and the independent nine-topic context inventory.
The demonstrated gap is that the shared reader cannot select those retained
reference bodies by a registered scope. Exact identities remain FEPI rows
0001–0039; registering context topics adds no command or obligation.
The manager owns only `conformance/0.10/manifests/cics-fepi-context-topics.json`,
the existing registry index, shared reader regression tests, source-cache
documentation, this status, a unique changelog fragment and derived docs.
Acceptance requires the nine archive body/TOC identities, shared manifest
and registry validation, offline scope search/read, preserved command scopes,
schema/docs/changelog checks and the existing policy/format results. Known
aggregate source/module failures remain open; no passing work-package seal
or parent completion can be issued while required gates fail. This is private
source registration with zero semantic, execution or licensed credit; all
fragment and out-of-cohort gaps stay explicit.

## Prepared bounded FEPI context pins

The independent `SPI-1001.fepi-context-source-inventory` completed on the
private checkpoint `d9a29b48c9e13218f1470efc4b32a3cfe5b62609`, tree
`c88dbca6e5d3c892526ce217a6830f495ffb272b`, with all 39 FEPI identities
preserved. Its external JSON reproduces with SHA-256
`7b00649162bd924d80bb6b6bd60ec603cf817061fc3d59f59c43bdd32e46253a`.
Nine retained context bodies match the pinned TOC, exact archive metadata,
H1, hashes, bytes and last-modified dates. The host topic-path copies are
absent; the matching archive bodies remain available without refresh.

`SPI-1001.fepi-context-pins` registers them through the existing shared reader
and `conformance/0.10/manifests/index.json`, scope
`cics-fepi-context-candidates`, baseline
`ibm-cics-ts-6x-fepi-context-candidates-2026-09-12`. The manifest is
`conformance/0.10/manifests/cics-fepi-context-topics.json`: 9 topics,
135,647 bytes, topic-set digest
`0b8515c30d5e18747b00dab9853bed716b568fe3a079604611861a0a6af39594`.
This private, unsealed checkpoint retains zero semantic/execution/licensed
credit and changes no command denominator, route or product behavior.

The bounded scan records 54 command-to-context links (47 distinct document
pairs) from the 36 command pages and 69 outbound context links. Ten additional
document locators remain unfollowed; 23 edges target six absent fragments:
`dfhp73i.html#dfhp73i`, `dfhp73u.html#dfhp73u`, `dfhp743.html#dfhp743`,
`dfhp74k.html#dfhp74k`, `dfhp74l.html#dfhp74l` and `dfhp74m.html#dfhp74m`,
under the FEPI topic prefix below. Named anchors provide no alternative.
Byte availability does not establish fragment identity, full reference closure
or any mandatory semantic context. The archive run remains `in-progress`;
this bounded inventory is not corpus/browser reproduction certification.

Focused validation passes: all 26 offline-reader tests (including the new
nine-topic/disjoint-command regression), the shared xtask topic-manifest test,
Draft 2020-12 schemas, formatting and dependency policy. Documentation and
changelog checks are recorded in the external candidate receipts. The unchanged
aggregate module, required-source, application and licensed blockers remain open.

## Declared FEPI source inventory (completed)

The next independent source-only review is
`SPI-1001.fepi-context-source-inventory`, based on sealed command-body pins.
It accounts for FEPI rows 0001–0039 and reads only the nine previously located
context candidates under `SSJL4D_6.x/reference-applications/commands-fepi/`:
`dfhp708.html`, `dfhp73i.html`, `dfhp73u.html`, `dfhp743.html`, `dfhp74k.html`,
`dfhp74l.html`, `dfhp74m.html`, `dfhp7k4.html` and `dfhp7kq.html`.
The existing FEPI CLI worker owns external
`v010-20261002/fepi-context/` only on an isolated branch at the private
checkpoint. It must verify exact TOC locators, retained hashes/bytes/provenance,
use offline search/read with pinned Python, and enumerate bounded references
from the 36 command bodies to these nine candidates. Out-of-cohort references
are reported as locators without retrieval or presumed closure.
Acceptance is a reproducible source-only inventory with all 39 identities
preserved and an explicit closure boundary. No grammar, conditions, execution
contexts, operation semantics or mandatory-obligation waivers are derived.
The manager retains exclusive manifest/index, reader, generator, schema,
status and integration ownership; any later source registration remains private
and grants zero semantic, execution or licensed credit. This inventory does
not depend on accepting the unsealed row maps or the application integration.

The user explicitly extended the goal on 2026-10-02 to finish the CICS v0.9
application API prerequisite before resuming SPI/FEPI. The first bounded wave is
declared in `application-api-status.md`: remaining command authority (0027,
0093, 0114), installed same-level target admission (0097/0263), and task-wide
BTS SET storage review (0086). The manager owns shared contracts, generated
authorities, status, conformance bindings and integration; workers have disjoint
server implementation or external review ownership. The licensed-run waiver
keeps differential pending with zero credit and does not waive local acceptance.

### Declared v0.9 integration prerequisites after the UOW repair

`CIC-902.program-task.frames.same-level-claim` is the next serialized manager
slice for rows 0097/0263. It extends only the existing synchronous task claim
and top program loan in `handlers/host_boundary.rs` and a bounded private child.
The caller must independently attest the canonical source Transfer, frozen
selection, source/target instances and existing pending CALL before executing.
Admission preserves root task, logical level, invoking/return program and shared
resources, rebinds only the exact non-widening target actor, and fences unknown
outcomes without reconstructing a cold loan. Focused wrong actor/thread,
selection/control/context, nested replacement, shared state and uncertain-result
checks plus existing logical-frame regressions are required. Durable CALL/instance
handoff remains a separate prerequisite; no full frame or application acceptance
is granted by this helper.
The exact manager allowlist includes the host-boundary replacement module/tests,
the existing program-control frozen-selection validator/export, one test helper
visibility change in `service.rs`, ADR-0027, this status, a unique fragment,
derived documentation and the two added handler paths in the reviewed module
inventory. No module ceiling or exemption is increased.

`SPI-1006.host-api-doc-prerequisite` owns documentation only for non-generated
Rust modules below `crates/contracts/mainframe-env-host-api/src/`, excluding
`lib.rs`, `generated/` and `canonical/generated.rs`. An isolated CLI worker must
measure the current missing-doc diagnostics once, document existing contracts
accurately, preserve all non-comment Rust tokens and wire semantics, and reduce
the host API count to its existing 1,128 ceiling or below. The manager exclusively
owns any downward policy update, generated artifacts, facade, status and integration.
Acceptance is a retained diagnostic inventory, meaningful documentation review,
non-comment token comparison, focused rustdoc/format gates and mandatory policy.
Execution API semantic/ABI changes remain a separate manager lane. No ceiling increase, lint suppression,
semantic expansion, source refresh or subsystem credit is permitted.

`SPI-1006.execution-api-doc-prerequisite` assigns a second isolated worker
comments only in `mainframe-env-execution-api/src/context.rs`, `machine.rs`,
`effects.rs` and `participant.rs`, if present; all generators, generated code,
facades, schemas and ABI changes remain manager-owned. Measure the actual
missing-doc inventory, document existing controls/lifecycles/participant behavior
without changing tokens, and reduce the execution API to its existing 176 ceiling
or below. The same focused rustdoc/token/format/policy and cleanup gates apply;
any lowered ratchet is integrated only by the manager. Neither documentation
lane changes semantics or grants CICS command coverage.

The bounded `CIC-902.program-task.frames.same-level-claim` prerequisite is sealed
at `7e0bb0816c48cea67bd7db599efca0b7aaa868fc`: five new helper regressions,
19 existing frame regressions and four selected pending-route tests pass. Four
ignored PostgreSQL/staging selectors earn no credit. Architecture, formatting,
documentation, changelog and dependency policy checks pass; target artifacts are
removed. Durable CALL/instance handoff and runtime wiring remain pending.

The manager's documentation candidate integrates the two comment-only worker
patches and lowers the existing policy to the measured counts: host API 2,074 to
1,119 and execution API 303 to 32. Independent lexical review preserves every
non-comment token in all 15 affected modules. The remaining diagnostics stay
explicitly ratcheted; no ceiling increase, ABI change or subsystem credit is
introduced. The documentation exposed a fixed module-size failure in the host
request parent: the manager moves the existing dataset request/result declarations
unchanged into `request/dataset.rs`, preserving stable exports and lowering the
parent ceiling from 2,283 to 2218. The private child remains below 1,200
lines. Integration uses the full existing public-API checker and required
format/documentation/module/dependency gates, without runtime campaigns for
comments alone.

`CIC-902.program-task.frames.versioned-source-handoff` is the next serialized
manager prerequisite, owning the existing interpreter coordinator and a bounded
private child plus focused selected-store regressions. Before any terminal or
journal mutation it must reject an absent, stale, foreign or non-suspended source,
including exact source selector/artifact/run/principal/attempt. Existing journal
and core CAS remain authoritative. This method alone grants no target admission,
CALL receipt, instance lease or automatic redispatch. Its exact allowlist and
source-backed durable CALL/instance contract are reviewed separately before
runtime wiring; workers may submit read-only designs only for those shared owners.

`CIC-902.program-task.frames.transfer-state-authority` continues the same isolated
frame worker session as a read-only prerequisite review for rows 0138/0263 and
source program lifecycle state. Its exact obligation is to identify target-pinned
COBOL/CICS authority for ordinary last-used state and INITIAL disposal after XCTL,
and propose the smallest pure transfer snapshot contract without asserting normal
GOBACK/EXIT PROGRAM. It owns external metadata/design receipts only; all runtime,
coordinator, original CALL/instance schemas, registries, generators and status are
manager-owned. Review at most twelve relevant retained topics through pinned
search/read and verified HTML fallback; no network refresh or broader audit.
Acceptance is explicit baseline/topic/row citations, preserved prior patch bytes,
applicable static state fields/resource exclusions and precise unresolved authority.
Source review earns zero execution/licensed credit and reserves no codec/API.

The API documentation/module prerequisite is sealed at
`d53aca908f63c62883e7b6cffccf0284f5d19924`: the full documentation ratchet,
104 host contract tests and required architecture/format/docs/changelog/dependency
gates pass. The source core handoff prerequisite is sealed at
`3f46b954751832336645da7e550a8b6713cfa6fc`: five local negatives/race/quota/SQLite
reopen tests, one actually executed PostgreSQL reopen test and ten existing
coordinator regressions pass, with required gates and cleanup. It reads only the
exact pinned suspension event; quota rollback is checked at the core commit.
Neither prerequisite grants installed transfer execution or parent acceptance.

`CIC-902.program-task.frames.attestation-prerequisite` is the next serialized
integration slice for rows 0138/0263. It reuses the worker's private canonical
source revalidation, exact inherited constructor proof and read-only instance CAS
snapshot. The manager binds the warm existing instance Lease to its acquiring
invocation so another execution/attempt/context cannot borrow that token. The
allowlist is the seven reviewed server instance/replay implementation/test paths,
ADR-0027, this status, a unique fragment and derived documentation. The worker's
ignored future target-completion diagnostic stays external and earns no credit.
Acceptance includes forged source/constructor/control/lease identity and immutable
selection regressions, existing compiled pending routes and required gates.
Snapshot tokens confer no source retirement, target execution, cold lease or
receipt completion. Durable CALL phases and source-state authority remain pending.

Two additional dependency-ready CLI lanes are declared for the current wave.
`CIC-904.bts-container.root-lifetime-regressions` resumes the task-storage worker
in a fresh checkout of `3f46b954751832336645da7e550a8b6713cfa6fc`. It owns only
`mainframe-env-server/src/product/tests/bts_set_lifetime.rs` and its single test-only
module registration inside the existing inline test module in `product.rs`,
plus external receipts. `product/tests.rs` is absent; no such file is required. For row 0086, derive
independent compiled root SET/dereference, different-container expiry,
INTO/NODATA preservation and failed subsequent SET regressions from the verified
BTS/channel sources. Begin with the actual selected route; keep deliberately red
cases explicitly ignored diagnostics with their actual failure receipts. No
runtime repair, allocation ABI, checkpoint codec, provider semantics, CALL schema,
coverage or readiness change is authorized in this lane. Manager reviews the
minimal root-policy repair and all shared contracts later. Acceptance is a
reviewable disjoint regression patch, independently seeded observations, exact
baseline failures/control successes, focused format/module checks and cleanup;
diagnostics grant zero command or recovered credit.

`SPI-1001.fepi-retained-context-closure` resumes the FEPI source session read-only
in its existing isolated checkout. It owns external receipts only, reviewing the
six CICS TS locators previously enumerated outside the 45-body cohort: developing
FEPI `dfhp73r.html`, configuring FEPI `dfhp76q.html`, programming reference,
EIB `dfha8me.html` and `dfha8mf.html`, and Passticket security. Preserve all 39 FEPI
identities and prior receipts. Resolve committed pins first; for unregistered
retained candidates check the pinned TOC and archived provenance and propose
bounded hashes without silently accepting them. Explain the six absent fragments
from the existing nine context bodies without substituting anchors. No z/OS,
external book, network/browser refresh, whole-cache audit, manifests, grammar,
runtime, denominator or coverage changes. Acceptance is exact retained provenance,
applicable command/context linkage and explicit missing or unresolved identity;
all new pins and semantic decisions stay manager-owned and private. Source review
grants zero execution/licensed credit. These lanes are independent of the manager's
server transfer-attestation validation and source-state authority review.

The attestation prerequisite is sealed at
`38f08bb22f9ea5b000a09a586756453edb1ae54c`: 25 focused tests pass and four
ignored backend/staging selectors earn no credit. Required architecture,
format/docs/changelog/dependency gates pass; default target is removed.

`CIC-902.program-task.frames.transfer-context-pins` is the next manager-only
source prerequisite for rows 0138/0263. The twelve-body worker review identifies
four unregistered retained contexts: CICS `dfhp3_concepts_storeprog.html`,
`dfhp3_cobol_prog.html`, `dfhp3_cobol_subprog_calling.html`, and COBOL 6.5
`pg/tasks/tppgm07.html`. Matching archived metadata/body bytes are present.
Verify each against its existing product TOC, exact content locator and H1;
register two bounded zero-credit candidate manifests under the existing 0.9
registry, then read all four through the shared pinned reader. Exact ownership is
those two new manifests, the registry, a bounded metadata-only verification
receipt, this status, a unique fragment and derived documentation. No browser,
network, publication text in Git, last-used tuple reuse, grammar, runtime,
coverage or codec disposition is introduced. Acceptance is identity/hash/reader
validation and focused source-reader/schema/docs/policy gates. All four bodies have now been fully read through the pinned reader. CICS program
variable storage lasts for program execution, and the subprogram rules depend on
LINK versus static/dynamic COBOL CALL; INITIAL documents reentry initialization.
These contexts do not justify treating XCTL as normal GOBACK. Last-used tuple
reuse and exact source disposition remain fenced pending the applicable calling
rules and manager lifecycle review.

`CIC-902.program-task.frames.calling-context-review` is an independent read-only
continuation of the command-authority worker. It owns external provenance/design
only for the two literal links from the newly retained subprogram context:
`SSJL4D_6.x/applications/developing/cobol/dfhp3_cobol_subprog_flow.html` and
`dfhp3_cobol_subprog_rules.html`. It may consult the four exact retained context
identities from the manager pin candidate, explicitly keeping candidate identity
separate from accepted semantics. Resolve existing pins first, then exact retained
archive/TOC/H1 provenance for an unregistered candidate; no network refresh or
further outbound bodies. At most six bodies. Acceptance is baseline/hash/row
0138/0263 linkage and a precise lifecycle distinction between CICS LINK/XCTL and
static/dynamic COBOL CALL, including working storage and INITIAL without guessed
file, random or task-memory ownership. No code/API/schema/registry/coverage edit,
normal-return equivalence, disposition verdict or execution credit is authorized.
The manager serializes any resulting pins and runtime contract after review.

The four retained program contexts are sealed as an identity-only prerequisite at
`c77892ba26974159cb28ac0323a93e55328505dc`. The initial reader/schema checks and
repaired final corpus/architecture/docs/changelog/dependency gates pass. All
source data remain external, and target artifacts are removed.

`CIC-904.bts-container.explicit-root-lifetime` resumes the task-storage session
in its existing regression checkout at `3f46b954751832336645da7e550a8b6713cfa6fc`.
The relevant interpreter/root fixture owners are unchanged by later manager
prerequisites. Own only interpreter `machine.rs` (private CICS pending metadata
and response hook), `machine/typed_cics.rs` (its local preparation call),
`machine/typed_cics/container_set.rs`, `machine/typed_cics/response.rs`, the new
server `product/tests/bts_set_lifetime.rs`, and its existing test-only registration
in `product.rs`. Shared schemas, checkpoint codecs, execution ABI, facades,
provider routing, task/frame memory and coverage remain manager-owned.
Repair the two observed red cases for explicitly resolved BTS selectors. Keep
BTS loans distinct from channel pair loans under the existing interpreter memory
authority; no shadow allocator. A known observed BTS SET response expires the
previous BTS loan even on the reviewed handled missing-container condition;
INTO/NODATA and channel-pair loans remain distinct. No guessed pre-issue,
SAF/infrastructure failure or cross-variant expiry rule. Admission/preflight
failure must not publish a successful loan. Account metadata/address bounds and
preserve unknown outcomes. Turn the two independent compiled diagnostics into
ordinary tests only after they pass. Acceptance includes all six root route
tests, focused channel/metadata/control/boundary regressions, checkpoint purity
and restore observations, required policy/format/module gates and cleanup.
Task-wide child RETURN/XCTL, active legacy untyped loans, cold ownership and
recovery closure remain pending; this root prerequisite grants no parent or
recovered credit. If a serialized/public contract change is required, return the
smallest proposal for the manager rather than reserving one.

`CIC-901.cicsmessage.source-applicability-design` assigns the now-idle frame
worker a disjoint read-only shared-conformance proposal for official row 0027.
Own external design/fixtures only. Verify the internal-only diagnostics footnote
and immutable EIBFN identity through pinned sources; consult the existing shared
Conformance IR/spec/coverage owners and current all-six-gates catalog bindings.
Design the smallest explicit source-backed distinction between inadmissible public
commands and executable commands, preserving all 263 identities and accounting
for every mandatory obligation without calling rejection executed or recovered.
No invented not-applicable verdict, lowered acceptance, catalog removal, alternate
ledger or row-specific bypass. Provide exact shared schema/driver/validator
changes for manager review, independent rejection/mutation fixtures and whether
any required authority remains unresolved. No repository/schema/registry/status/
coverage edit or readiness/sealer/completion claim. At most two pinned source
bodies; no network or unrelated lifecycle investigation. This lane can progress
independently of root BTS implementation and the calling-context source review.

`CIC-902.program-task.frames.calling-context-pins` is a manager-owned bounded
identity prerequisite for the two reviewed flow/rules bodies, 17,228 bytes.
Own `conformance/0.9/manifests/cics-cobol-calling-context-topics.json`, the existing
0.9 registry, a bounded metadata-only verification receipt, this status, unique
fragment and derived documentation. Verify worker proposed hashes against retained
metadata/TOC/H1 and read both through the shared pinned reader. Do not change
existing pins, official rows, source-map routing, runtime, tuple eligibility or
coverage. Source-only acceptance uses exact identity/reader/schema/corpus/docs/
policy checks. The reviewed rules distinguish fresh LINK storage from subsequent
same-level COBOL CALL storage; runtime instance ownership and XCTL disposition
remain manager-reviewed prerequisites.

After the calling-context pins are sealed,
`CIC-902.program-task.frames.link-storage-regressions` assigns the same source
session a fresh exact-candidate test checkout. Own only server
`product/tests/program_identity.rs`, whose existing test module already reaches
compiled product fixtures; no shared module registration or production change.
Use rules lines 93–102 for row 0138 and distinguish repeated LINK fresh working
storage from same-level native CALL retained working storage, fresh local storage,
and INITIAL reentry. Include a native CALL/LINK/native CALL sequence for the same
program name so a linked lower-level copy cannot overwrite retained higher-level
CALL state. Start with actual compiled selected routes, use independent VALUE
constants and assertions, and retain explicitly ignored red cases with actual
failure receipts. Existing identity/replay/reopen fixtures stay intact; no
runtime, schema, coverage, routing or status edit. Acceptance is disjoint reviewable
test patch, control passes and exact diagnosed baseline failures, source citations,
focused format/module checks and cleanup. These are diagnostics and grant no
runtime, parent, recovered or licensed credit. The manager owns any resulting
instance-scope and normal-return eligibility contract.

The two calling-context identities are sealed at
`c12ee5032ebf5ba8c9f3216dd60412f30d49beb3`; source reader/schema/corpus/architecture/
format/docs/changelog/dependency checks pass and target is removed. The declared
LINK storage diagnostics have started in a fresh checkout of that exact candidate.

`SPI-1001.fepi-retained-context-pins` is a manager-only identity slice for the
three independently reproduced retained candidates: begin-session handler,
sysplex workload routing and application programming reference. Own only
`conformance/0.10/manifests/cics-fepi-retained-context-topics.json`, its existing
registry entry, a bounded metadata-only verification receipt, this status,
unique fragment and derived documentation. Verify the 17,582-byte three-topic
proposal against actual metadata/body/TOC/H1, register and read locally through
the pinned reader. The existing three reused diagnostics/security pins and
all 39 FEPI rows stay unchanged. Keep the prior six absent fragments and all
23 literal edges explicit; do not substitute anchors or claim whole/prescriptive
source closure. Acceptance is exact pin/schema/reader and required metadata/docs/
policy gates with cleanup. This source-only slice earns zero behavioral/licensed
credit and does not accept the private source map, v0.9 dependency or parent SPI.

Keep the source-map checkpoint private and unsealed until the three SPI row equivalences, required
cache bodies and module gate are resolved. Then seal its exact allowlist before
declaring source-required context closure and deriving private SPI-1001
contracts. Application acceptance and licensed gates remain prerequisites to
dependent runtime integration waves. Do not refresh sources or widen into
unrelated application/batch implementation merely to clear these prerequisites.
Independent source-context inventories may still be prepared from the sealed
command-body pins, without consuming these candidates as semantic authority.

### Bounded LINK run-unit ownership design (2026-10-02)

`CIC-902.program-task.frames.link-run-unit-design` assigns the idle frame worker
a read-only proposal for the reproduced repeated-LINK and native-CALL/LINK/CALL
storage failures. It owns only external proposal artifacts, reads the sealed
`cics-cobol-calling-context` and `cics-program-storage-context` pins and the
existing invocation, CALL protocol, instance, retention, cursor and selected
route contracts. The manager owns every shared type, schema, codec, namespace,
registry, generator, status and runtime edit. The worker must preserve its eight
prior dirty paths and retained handoffs, use no nested workers or refresh, and
propose exact context inheritance, fresh LINK entry, native CALL level-local
retention, owner/row binding, CANCEL/end/ABEND, replay and unknown-outcome fences.
It must not invent a shadow store or equate the transaction UOW with a COBOL run
unit, and must state backward compatibility and source gaps explicitly.
Acceptance is a reviewable design and preservation receipt; no implementation,
new API reservation, test, execution, recovered or licensed credit is assigned.
The regression worker owns only `product/tests/program_identity.rs`; the BTS
worker owns its separately declared six paths. Parent CIC-902 and v0.9 remain
unaccepted.

### Bounded explicit BTS root SET repair (2026-10-02)

`CIC-904.bts-container.explicit-root-lifetime` integrates the exact six-path worker
patch after independent source and diff review. Two previously red compiled
ordinary LINKAGE expiry tests now have their ignore attributes removed; the four
controls retain their independent seeds. The private response boundary expires
only the captured BTS area on a matching observed response, including real
CONTAINERERR 110/10, and keeps INTO/NODATA, channel loans and unobserved host
failures separate. Existing bases/freed entries carry bounded private labels,
with no public codec/schema or task memory authority added. ADR-0028 records
compatibility, downgrade and unresolved legacy unmarked/cross-frame/task-end/
cold-recovery obligations. Manager integrated regressions pass: 6 compiled root and 17 lifecycle tests
plus 1 channel64 control, with no ignores. Required policy gates accompany the
exact candidate receipt outside disposable targets.
No official row, Recovered, licensed or parent completion credit is assigned.

The user retained CICSMESSAGE's internal execution requirement on 2026-10-02.
Its 263-row identity and all six required pending gates remain unchanged; the
source-only public-admission proposal does not grant execution or recovery.

### Bounded live LINK entry attestation (2026-10-02)

`CIC-902.program-task.frames.link-entry-attestation` assigns the finished BTS
worker a fresh checkout at `195f3fb5` for the missing live entry trust boundary.
It owns only CICS `handlers/host_boundary.rs` (private command-origin and selected
loan bookkeeping plus helper registration), new `host_boundary/link_entry.rs`
and `link_entry/tests.rs`, and one public re-export in `handlers/mod.rs` and
`lib.rs`. The manager owns every server scope/instance/CALL/retention writer,
public schema/codec, registry, generator, documentation and integration edit.
Before semantics, read pinned LINK row0138 and calling flow/rules. Preserve prior
BTS worktree/handoffs unchanged, use no nested workers or source refresh.

Freeze a nonserializable, private-field `CicsLocalLinkEntryAttestation` and a
read-only `CicsService::attest_local_link_entry(source, target, selection)` result.
The existing thread-confined task claim must prove the currently active canonical
CICS operation is LINK, its selected program loan is top/current, exact parent
source actor and frozen selection, exact target artifact/program/parent/root run
and principal, existing CICS logical level, matching control/resource envelope,
no outstanding child command and no foreign actor or uncertain task. Capture
typed command origin in the existing claim; never infer it from EIBFN, names,
payload schema or an idempotency prefix. Capture the actual selected tuple in the
existing loan; an artifact alone is insufficient. Direct test-only acquisition
without origin/selection must not gain attestation. Keep prior command/loan test
call shapes compatible and all existing admission/restoration/unknown fences.

The token exposes documented read-only source/target/root invocations, selection
and logical level; it has no constructor, serde, durable token, mutation, actor
admission, target execution, source retirement, cleanup, cached reply or coverage
permission. Getter creation must leave task/claim/row bytes unchanged. Expected
negative proofs include wrong source/target/context/selection/thread/depth/origin,
unknown/ended task, unselected/manual loan, child command outstanding and expired
loan. Source references and focused old frame/claim/restoration/uncertainty tests
plus module/format/dependency checks are required; all targets cleaned. This is
a bounded live authority prerequisite, not the LINK storage repair: both compiled
red diagnostics, durable scope binding, native CALL inheritance, cursor/cleanup
atomicity, recovered and licensed/parent acceptance remain pending.

### Bounded executed native-return witness (2026-10-02)

`CIC-902.program-task.frames.normal-program-return` assigns the finished read-only
storage-design worker a fresh checkout at `195f3fb5`. It owns interpreter
`machine.rs` only for a private volatile return marker, constructor/restore/drive
hooks and re-export; `machine/completion.rs` for the successful terminal-step
helper; new `machine/normal_return.rs` and `normal_return/tests.rs`; and `lib.rs`
for the public type re-export. The live LINK worker's provider paths are disjoint.
The manager retains all snapshots/codecs, server scope/instance/CALL writers,
retention, shared contracts, status, generators, registries and integration.

Freeze `InstalledProgramReturnKind::{Goback, ExitProgram}` and a nonserializable,
private-field `InstalledProgramReturn` with read-only invocation, kind,
program-counter and executed-step getters. `ReferenceMachine::attest_installed_program_return`
must require the actual successful drive terminal step, exact live machine marker
and supported closed retained-state/resource shape. PC alone, an unexecuted
constructor/quantum boundary, fallthrough/halt, STOP RUN, EXIT METHOD/FUNCTION,
CICS RETURN, abnormal/cancel/timeout/host results and restored checkpoints do not
create this witness. Successful restore invalidates any prior live marker; no
snapshot/MECP version or durable exit witness is written. Getters preserve bytes.
This witnesses only an executed normal language return; manager must separately
validate exact core Completed/journal/attempt/checkpoint and atomic scope close.
Before semantics read hash-verified calling flow/rules; use focused success,
pre-execution/stale/restore/control/resource negatives and existing affected
lifecycle tests, format/module/API/dependency gates and clean targets. No router,
instance cleanup, dispatch, parent/recovered/licensed credit or ceiling increase.

### Manager-owned storage-entry codec (2026-10-02)

`CIC-902.program-task.frames.storage-entry-codec` owns new server
`cobol/storage_scope.rs` and its tests, one `cobol.rs` module registration,
ADR-0029, its unique fragment and manager-generated documentation. It freezes
immutable scope creation separately from transient native CALL entry. The bounded
canonical `mainframe-env.cobol.storage-entry@1` binding retains the actual core
run, root execution and principal; root/LINK scope and member digests never become
core run IDs. Exact actor/parent/CALL occurrence, selector/artifact/attempt and
frozen selection are validated with no name-derived entry classification.
Canonical syntax and hashes provide integrity only: live entry/source lease,
root core authority, reservation/exit/close and retention must be independently
validated by future serialized writers. No active row/protocol is upgraded,
no writer/routing/storage behavior is enabled and both compiled red tests remain
pending. Reserve RunState3, Instance3, CallProtocol4 and ordinary scope Receipt5
for the later atomic writer; existing Transfer3/4 stay unchanged. These are
manager reservations, not accepted migration or recovered evidence. Focused
canonical vectors, rehashed foreign/phase/selection/context mutations, immutable
creation/native inheritance and bounded/pure binding checks plus mandatory gates
must precede sealing. Source calling flow/rules baseline and LINK row0138 apply.

The storage-entry codec prerequisite is sealed at `1715bf60` with eight focused
checks and six independent canonical vectors, mandatory gates passing and no
runtime writer enabled. Immutable creation also retains creator selector,
artifact and attempt when native CALL changes the entry actor.

The LINK-entry worker returned exact five-path patch `708b50de…a69b8e9`.
The manager reviewed the full production/test diff and pinned LINK body, then
applied it serially plus its two exact module-inventory registrations with no
ceiling changes. Manager integrated checks pass: 37 focused tests (10 new and 27 existing), no ignores, plus mandatory module/API/schema/format/docs/changelog/dependency gates. Its exploratory Rust 1.98
Clippy findings are unchanged host MQ/CICS code outside this bounded API;
aggregate lint stays pending, without new suppression or policy change.

The executed-return worker returned exact five-path patch `3859c652…2ff6660`.
Its 17 focused tests and three existing compiled installed-call tests pass in
the worker checkout. The manager reviewed the full patch and restore prechecks,
retained its exact artifacts and applied it serially. Manager integrated checks pass: 17 new witness tests, three checkpoint/storage64
regressions and three existing compiled installed-call tests, with zero ignores,
plus mandatory module/API/schema/format/architecture/docs/changelog/dependency
gates. Core completion currently persists its terminal events without a final
machine checkpoint; the future atomic scope writer must independently bind
the live terminal state and core authority. No writer or acceptance is enabled.

### Scoped storage runtime and independent regression wave (2026-10-02)

The return witness is sealed at `a86065a1` with 17 new and six affected existing
checks, mandatory gates passing. `CIC-902.program-task.frames.scoped-storage`
is the manager's serialized runtime lane. It owns existing server instance,
CALL replay, selected LINK construction, retention and storage-entry authorities;
all schema/registry/facade/generator/status edits stay with the manager. Extend
existing root namespaces and the reserved generations; preserve core task/UOW/run,
root-wide member bounds, exact source lease/CAS adoption, pending/unknown fences
and atomic scope reservation/normal close. No shadow dispatcher or owner ledger.
No runtime implementation or recovery acceptance is claimed by this declaration.

`CIC-902.program-task.frames.scoped-storage-regressions` assigns the existing
compiled diagnostic worker an isolated checkout from exact sealed `a86065a1`.
It owns only new `product/tests/program_storage_scope.rs`; the manager owns
its test-only `product.rs` registration. Use the existing actual compiled
selected CICS route with artifact-bound programs and SQLite, no host-result mocks.
Independently assert two LINK entries to MID, each making two pure native CALLs
to LEAF: native working storage persists within each MID scope, local storage
refreshes each entry, and the second LINK starts fresh. Also assert a higher
native LEAF before/after those lower scopes retains its own working bytes.
These extend the two retained simple diagnostics instead of duplicating them.
A nested LINK case may be added only if existing route support permits an actual
proof. Start with executable red diagnostics, retain expected failures accurately
and add no runtime, schema, coverage, source refresh or ignored-test credit.
Read hash-verified calling flow/rules plus LINK row0138 before expectations.
Run exact selected tests with fault-injection feature required by registration,
module/format/dependency checks; clean targets and retain external receipts.
No nested workers, other edits, shared authorities, commit bypass or acceptance
changes. The manager integrates only after the runtime contract makes them pass.

### Scoped terminal checkpoint runtime lane (2026-10-02)

`CIC-902.program-task.frames.scoped-terminal-checkpoint` assigns the finished
return-witness worker an isolated checkout from sealed `a86065a1`. It owns
execution-api `machine.rs` for one default optional `completion_checkpoint`
method; interpreter `coordinator.rs` plus new `coordinator/completion.rs` and
its tests for completion checkpoint construction/publication; interpreter
`machine.rs` and `machine/completion.rs` for the opt-in reference implementation.
The manager owns all store/schema/server/status/generator changes. The checkpoint
must accompany the exact Completed event in the existing core atomic journal
commit, preserving the preceding Completing transition. Generic machines keep
the default None behavior. ReferenceMachine opts in only after a successful
live native-return observation with the typed scoped storage-entry binding.
No terminal checkpoint substitutes for a live source lease, admits actors,
restores the volatile witness or makes terminal execution resumable.
The manager's future scope close must fence missing/unavailable checkpoints.
No checkpoint schema/version or shared store contract change is authorized.
Tests prove default compatibility, actual GOBACK/EXIT PROGRAM captured bytes,
constructor/restored/unscoped/abnormal exclusions, publication failure atomicity
and selected SQLite reopen. PostgreSQL is mandatory at runtime acceptance;
retain an explicit pending gate if unavailable rather than crediting an ignore.
Use source search/read before semantics, affected existing coordinator tests and
module/API/format/schema/docs/dependency gates; clean targets and external receipts.
No ceiling increase, suppression, nested workers, source refresh or acceptance
change. Internal CICSMESSAGE and licensed requirements remain pending.

The manager's scoped row reader child remains private and unsealed. It extends
the existing root retention reader with RunState3/Instance3 integrity, immutable
scope creators, exact member version/payload index, root-wide 256-member/16-scope
and monotonic original-CALL receipt charges. Reader hashes provide no live source
or execution authority. Independent Python root/member vectors accompany focused
mutations; the earlier ten scoped, eight entry and four retention checks passed
before the later original-CALL identity/creator-member changes. Current-input integrated checks pass: ten scoped, eight entry and four existing
retention tests, zero ignores, plus mandatory module/API/schema/format/architecture/
docs/changelog/dependency gates; historical receipts remain separate. Runtime writers remain off.
