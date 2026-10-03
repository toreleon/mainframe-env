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

The manager's scoped row reader child is sealed at `4d508ad9`, remaining private. It extends
the existing root retention reader with RunState3/Instance3 integrity, immutable
scope creators, exact member version/payload index, root-wide 256-member/16-scope
and monotonic original-CALL receipt charges. Reader hashes provide no live source
or execution authority. Independent Python root/member vectors accompany focused
mutations; the earlier ten scoped, eight entry and four retention checks passed
before the later original-CALL identity/creator-member changes. Current-input integrated checks pass: ten scoped, eight entry and four existing
retention tests, zero ignores, plus mandatory module/API/schema/format/architecture/
docs/changelog/dependency gates; historical receipts remain separate. Runtime writers remain off.

### Manager live source fence (2026-10-02)

`CIC-902.program-task.frames.scoped-live-source` owns new scoped instance
`live.rs` and its tests plus one private module registration. The existing
manager-owned scope lane keeps registration private to known atomic admissions.
A thread-confined weak lookup stack observes only the current exact router and
invocation. An owned lease revokes lookup on exit even while an observation is
retained or a registry borrow is active. Readers and cold busy rows cannot mint
these tokens. The source fence revalidates full stored bytes and version, stages
a byte-preserving CAS, requires a coupled root member-index CAS and adopts the
new source version only after known atomic success. A store error retains the
old observed version and fences further admission, including lost acknowledgement
after commit. Current-core/control/LINK origin, target/pending CALL preparation
and actual scoped runtime wiring remain manager obligations, with no recovery
or official credit from this bounded source primitive. Focused tests require
cold/thread/context/lease-exit rejection, stale observation rejection, coupled
root conflict rollback, known retokening and actual after-commit ack loss; source
flow/rules pins apply and targets must be cleaned. No shared ledger or new root
identity is introduced.

### Terminal checkpoint PostgreSQL follow-up (2026-10-02)

The existing scoped terminal-checkpoint worker resumes from its local unsealed
`a5d0d1a6` candidate for a bounded backend evidence follow-up. It owns only its
external test harness, task-specific PostgreSQL data/server and receipts; no
tracked file edits are authorized. It must use local PostgreSQL 18.6, an isolated
loopback port/database, the exact current worker libraries or verified retained
binary, and stop the server and clean its own Cargo target after the sequence.
Required selectors cover actual GOBACK and EXIT PROGRAM atomic terminal images,
close/reopen, Completed/Completing no redispatch, invalid image and configured
payload/capacity publication failure preserving Completing without partial
checkpoint/events. Earlier ignored selectors remain historical zero credit.
No source refresh, licensed run, nested worker, manager checkout write or parent
completion is authorized. These worker receipts require later integrated-candidate
verification and cannot replace live scope admission or close authority.

Integrated live source primitive: six focused tests pass with zero ignores;
mandatory policy gates must pass before its bounded seal. Actual scope routing
and application acceptance remain pending.

The checkpoint worker's six-path implementation is reviewed and reconciled
serially after the sealed live source guard. The manager adds repository backend
regressions under recovery_tests/terminal_checkpoint.rs with one parent module
registration; these own test fixtures only. Memory/SQLite compatibility, failure
atomicity and physical reopen are mandatory; four isolated PostgreSQL selectors
are executed explicitly, with ignored defaults carrying no credit. Current-input
integration gates and the bounded seal remain pending. No scoped writer or
application/parent acceptance is enabled by checkpoint capture.

### Scoped member reservation preparation (2026-10-02)

`CIC-902.program-task.frames.scoped-member-reservation` delegates a bounded
private preparation helper from sealed `344472e7` in an isolated checkout.
It owns only new instance/scoped/reservation.rs and reservation/tests.rs. The
manager supplies and freezes one private parent module registration, retaining
all row schemas, factories, replay/status/docs/generator and publication ownership.
Dependencies are the sealed Entry1 and RunState3/Instance3 readers; terminal
capture is unnecessary for admission preparation. Exact obligations: validate
the trusted root limits, complete member set and original source/target entry
identity; native CALL retains its scope, a LINK entry creates a fresh level with
a busy creator member; reject an existing busy target or changed selected
artifact/INITIAL metadata; preserve idle working state for non-INITIAL native
calls and clear it for INITIAL; reserve root-wide member/scope/frame/monotonic
CALL charges and exact target/root CAS writes without mutating inputs.
No helper may dispatch, mutate a store, mint a live lease, migrate generations
or declare a known reply/close/recovery. The manager must validate indexed CALL
rows and couple the prepared writes with original pending receipt/source CAS
and current core/control/LINK origin before actual admission. Acceptance requires
independent exact keys/levels/state expectations, negative rehashed identity/
member/capacity/CAS mutations, no state changes on failure, focused Rust tests
and module/format/dependency checks; mandatory integrated gates and a bounded
seal follow manager review. Source search/read uses the pinned calling flow/rules
and LINK row0138. No nested workers, other edits, fresh source/licensed runs or
parent completion is authorized.

Integrated terminal checkpoint checks: 39 kernel/coordinator and ten Memory/
SQLite selectors pass. Four PostgreSQL 18.6 selectors are explicitly executed
and pass on four fresh databases, including both native returns, invalid image
and payload-limit failure with physical reopen and no redispatch. Default
registration ignores earn zero credit; the four explicit runs remain separate.
Mandatory gates must pass before this bounded seal. Scoped routing/close and
application acceptance remain pending.

Integrated terminal checkpoint checks: 39 kernel/coordinator and ten Memory/
SQLite selectors pass. Four PostgreSQL 18.6 selectors are explicitly executed
and pass on four fresh databases, including both native returns, invalid image
and payload-limit failure with physical reopen and no redispatch. Default
registration ignores earn zero credit; the four explicit runs remain separate.
Mandatory gates must pass before this bounded seal. Scoped routing/close and
application acceptance remain pending.

### Manager scoped terminal observation (2026-10-02)

`CIC-902.program-task.frames.scoped-terminal-observation` is the serialized
manager-owned consumer of sealed terminal capture `5726a469`. It owns new
instance/scoped/terminal.rs and its tests, one private module registration and
one read-only live lease invocation accessor. It must combine an exact current
thread-confined managed lease, full stored instance bytes/version, actual live
GOBACK/EXIT PROGRAM marker and fresh captured image with the exact stored core
Completed state/event/version/attempt/tick and checkpoint identity/generation/
interfaces/bytes/size/digest. Fresh control must reject cancellation, deadline
or time regression. Cold images and an earlier checkpoint beside Completed
remain insufficient. The result is a private non-serializable read observation,
never permission to close, redispatch or reconstruct a lease; the publisher
retains member/root/CALL CAS and quiescence ownership. Actual native-return unit
fixtures and rehashed stale/context/checkpoint/control failures must prove these
bindings without mutation; source calling flow/rules, mandatory guards and clean
targets apply. Scope close and application acceptance stay pending.

The stronger checkpoint PostgreSQL follow-up passed six selected cases against
sealed `5726a469`, with real server stop/start, fsync on, actual GOBACK row-quota
publication failure and malformed digest/size rejection. The worker's earlier
loopback-bind denial remains a separate zero-execution receipt; the manager
performed these new current-library executions and stopped its owned server.

This observation also owns the existing kernel normal-return witness and its
completion hook for one immutable captured return_code plus read-only getter.
It records the code from the actual successful Completion, preserving volatile
reset/restore rules and every snapshot codec. This avoids cloning a full machine
snapshot and lets the consumer reject a Completed event whose return code was
produced by another capture publisher. Kernel normal-return/completion regressions
must run again for these changed inputs; prior checkpoint receipts remain bound
to `5726a469` and are never relabeled.

Terminal observation current-input checks: five exact live/store/control tests
plus seventeen native-return and ten capture regressions pass with zero ignores.
The private observer writes no state; mandatory policy gates precede its bounded
seal. Full scoped admission/close and application gates stay pending.

The reservation worker's full production and twenty-test diff has been reviewed.
The manager reconciles its two owned files while preserving the later live and
terminal module registrations. It remains pure postimage preparation: runtime
writers are off, and complete combined source-index CAS validation plus original
CALL/core/control/selection authority remain manager factory obligations. Current
integrated tests and mandatory gates precede its bounded seal.

### Actual scoped factory context review (2026-10-02)

`CIC-902.program-task.frames.scoped-factory-context-review` delegates read-only
analysis in an isolated checkout at sealed `e850db51`. The worker owns only an
external bounded handoff; all repository paths, schemas, routing, status and
factory implementations remain manager-owned. It must trace actual online and
batch selected-program Invocation construction, core admission, original native
CALL and CICS LINK source preservation, child limits/grants/audit/deadline/control,
and interpreter storage/handler cleanup. Identify concrete hooks and authority
checks for the existing live/reservation/terminal primitives; do not invent a
shadow dispatcher or claim execution. Cite exact current file/line and pinned
flow/rules/LINK source review, preserve repository bytes, and return a bounded
plan with remaining blockers. No build, edits, nested workers, source refresh,
licensed run, commit, push or parent completion is authorized.

Reservation current-input checks: twenty focused tests pass with zero ignores.
The helper writes no store state; mandatory gates precede its bounded seal.
Actual admission/close and all application acceptance gates remain pending.

### Scoped runtime dependency wave (2026-10-02)

The manager owns original CALL5/protocol4 codecs, shared registrations, the
actual selected root context, source/control factories and serial integration.
Reservation preparation is now sealed at `05b482c6` with twenty current-input
checks and all mandatory gates; no runtime or parent credit follows.

`CIC-902.program-task.frames.link-call-origin-proof` owns only CICS provider
handlers/host_boundary.rs, host_boundary/link_entry.rs and a new dedicated test
module. It must retain the actual selected EffectRequest and original outer
CICS effect key in the existing live LINK loan and expose an immutable read-only
attestation for that exact request. Keep the prior entry attestation compatible;
manual fixture loans lacking actual effect provenance cannot pass the new proof.
Validate current same-thread/top live loan, original unmodified source, actual
selection/target controls, full effect identity/payload/deadline and outer key;
no durable bytes, dispatch, store writer, fabricated loan or shadow journal.
The manager remains responsible for matching the outer key to the current core
Intent and source authority. Tests require actual selected LINK success and
changed/rehashed key, request/payload/sequence/deadline/selection/source failures,
manual loan rejection and expiry/exit/cold rejection. Pinned LINK row0138 plus
flow/rules apply; isolated CLI worker, no nested workers/refresh/license/push.

`CIC-902.program-task.frames.scoped-return-preparation` owns only new instance/
scoped/closing.rs and closing/tests.rs, using a manager-frozen registration.
From a current managed lease and exact terminal observation it prepares native
idle state or deletion of just a quiescent LINK scope, with exact root/member
CAS proposals and complete member postimage validation. The actual fresh machine
must match the observation; INITIAL drops retained bytes, other native entries
retain their actual working state. LINK siblings must be idle and child scopes
absent. Preserve every higher scope/member byte, index version and root CALL/
receipt charge. Return exact observed rows for the manager-owned bounded close
proof. No publication, lease creation, original CALL DTO, cleanup permission,
CANCEL/ABEND/task-end/recovery or runtime enablement is delegated. Require actual
GOBACK/EXIT fixtures and stale/current-lease/quiescence/capacity/CAS negatives,
zero mutation on failure, independent counters and exact sibling preservation.
All shared schemas/factories/status/docs remain manager-owned. Focused tests and
mandatory worker module/format/dependency checks precede unsealed handoff; manager
integrated gates and sealer follow. No nested workers or unrelated work.

### Manager inherited CALL controls (2026-10-02)

`CIC-902.program-task.frames.inherited-call-control` repairs the actual native,
installed batch and compiled batch child factories' audit correlation and live
cancellation probe inheritance. The manager owns cobol.rs and one new private
hardening regression module/registration, with a unique fragment and status/docs.
Current factories construct distinct traces but must retain the caller's audit
identity; both batch factories also need the same live cancellation signal.
Start with compiled regressions for the three routes and a request made during
child control observation; prove controls/pins/grants/limits remain inherited
and requested cancellation wins. No schema, scope writer, source refresh or
application gate closure is included. Focused regressions and mandatory gates
precede the bounded seal; all actual scope factory obligations remain pending.

The initial control regression sequence compiled but rejected all calls before
child observation: its generation-17 fixture conflicted with the actual host
registry generation 1. That failure gives no child execution/bug credit. The
fixture now carries the exact selected host generation 1. The manager repairs
the demonstrated factory inventory gaps by explicitly retaining caller audit
correlation in all three constructors and the live probe in both batch paths.
Current-input compiled regressions and mandatory checks remain required.

The repaired cancellation regression passes for all three compiled routes. The
success harness also records the later parent retention-time observation; that
parent record is now excluded from child-only inheritance assertions. The prior
mixed receipt remains failed and is not relabeled. Current-input checks use two
new regression selectors and the three existing installed-call tests, rather
than treating route-loop cases or filtered tests as extra passing selectors.

Both new regressions and the three installed-call compatibility selectors now
pass with zero ignores. The hardening parent is already at its frozen 1,364-line
ceiling: its new registration is placed in an explicit terminal cfg(test) module
using the same module path and included owned test file, keeping the production
ceiling unchanged. Current gates must verify this test-only registration; no
inventory ceiling or module guard is relaxed. Earlier failed gate receipts remain
separate from the repaired candidate.

Inherited-control current-input checks pass for all three compiled factories,
including cancellation requested during child observation, plus existing selected
installed-call compatibility regressions. No scoped writer is enabled; mandatory
gates precede this bounded seal and all application acceptance remains pending.

## Atomic core guard review and LINK call provenance integration

Manager declares `CIC-902.program-task.frames.atomic-core-guard-review` before
CLI dispatch. This read-only dependency slice reviews exact existing execution,
event, effect and checkpoint guards within Memory/SQLite/PostgreSQL provider-row
transactions. It owns only external `atomic-core-guard-review/handoff.md` and
`handoff.json`; no tracked edits, tests, schema, facade, ceilings or store writes
are assigned. Manager exclusively owns the shared contract and serialized runtime
admission/close implementation. Acceptance is precise lock-order, race, existing
DTO/API, SQL boundary and backend test recommendations, with zero execution credit.
Base is the sealed inherited-control candidate `337da5038474167ac3743b675e891ed30674c018`.

Manager serially integrates `CIC-902.program-task.frames.link-call-origin-proof`:
the three reviewed provider host-boundary files, manager facade exports, exact
one-path handler inventory registration, ADR-0029, unique fragment, this status
and derived documentation manifest. Worker tests passed 45 (eight new and 37
existing), without ignored tests. Its shared-Git index write was unavailable; the
reviewable bytes are retained without a sandbox bypass and are not a sealed worker
candidate. Current integrated focused and mandatory gates must pass before seal.
The observation retains the full actual nested LINK request and original outer
CICS effect key; matching current canonical core Intent and a current source lease
remains the manager's responsibility. No scoped writer, cold loan reconstruction,
parent acceptance or licensed credit is enabled.

Integrated LINK call provenance checks pass: 45 focused tests, zero failures
and zero ignored. Manual loans, changed requests/sources, cold or exited loans,
cancellation, expiry and nested top-loan ownership are covered. Mandatory gates
precede the bounded seal; no runtime writer or parent acceptance is enabled.

## User priority change: begin v0.10 now

On 2026-10-02 the user instructed: “3 unready typed v0.9 có thể skip, nhanh
chóng bắt đầu hoàn thiện v0.10 đi”. This supersedes the earlier finish-v0.9-first
implementation priority, including the retained internal CICSMESSAGE requirement
as a current task prerequisite. Application rows 0027 CICSMESSAGE, 0093 GETNEXT
TIMER and 0114 ISSUE COPY remain unready and pending; they are deferred from this
implementation run, without removing their catalog identities or claiming passes.
The existing 260 typed registrations are the development compatibility baseline.
Application acceptance is not declared. Remaining scoped-return and atomic-guard
review handoffs are preserved; no additional v0.9 frame work is the next lane.
Licensed differentials remain pending with zero credit under the earlier waiver.

Manager declares these dependency-safe SPI-1001 contracts before CLI dispatch:

| Slice | Exact catalog rows | Exclusive ownership | Dependencies and acceptance |
|---|---|---|---|
| `SPI-1001.spi-program-contract` | SPI 0026 CREATE PROGRAM, 0084 DISCARD PROGRAM, 0155 INQUIRE PROGRAM, 0241 SET PROGRAM | `conformance/0.10/cics/families/spi-program.json` and external worker handoff only | Exact four mapped command-body pins; frozen shared family schema. Source-reviewed options, constraints, response/condition, lifecycle/security/effect/recovery facts and independent case expectations; schema validation. No handler/runtime/coverage claim. |
| `SPI-1001.fepi-pool-contract` | FEPI 0001 ADD POOL, 0007 DELETE POOL, 0009 DISCARD POOL, 0018 INQUIRE POOL, 0021 INSTALL POOL, 0034 SET POOL | `conformance/0.10/cics/families/fepi-pool.json` and external worker handoff only | Exact six mapped command-body pins and necessary retained context; frozen shared family schema. Source-reviewed options, constraints, response/condition, lifecycle/security/effect/recovery facts and independent case expectations; schema validation. No handler/runtime/coverage claim. |

Manager exclusively owns `SPI-1001.family-contract-schema`, existing catalog/
contract generators, all shared facades, IR/HIR/MIR, host ABI, command/resource/
condition/SAF/effect/UOW/retention authorities, route registration, status, ADRs,
fragments and derived docs. Workers do not alter these owners, spawn workers or
refresh sources. Private source-ready cohorts may proceed under the user's new
priority; unresolved SPI source equivalences 0201/0203/0204 and FEPI context gaps
remain explicit and cannot be inferred from names or EIBFN. No public capability
is advertised before its selected-route and compatibility gates pass.

The family contract is a bounded private product catalog with source-review
case candidates, not an executable Conformance IR or verdict ledger. Cases
will bind through the existing shared IR owner; product generators consume
semantic contract facts rather than test outcomes. No runtime or official
coverage is claimed. The pinned Python lacks jsonschema; the repository's
existing Rust Draft 2020-12 schema checker owns validation instead.

## Shared family validation owner

Manager declares `SPI-1001.family-contract-validation` before implementation.
The demonstrated gap is that `xtask schemas` compiles the new family schema but
no existing gate checks its artifact instances against source-map/manifest rows
or verifies option-constraint and case-reference closure. This slice exclusively
owns `xtask/src/cics_system_families.rs`, its parent module/CLI registration, this
status, a unique fragment and derived docs. It extends the existing xtask schema
validator, rather than adding a source reader, executable Conformance IR,
dispatcher or result ledger. The two family workers retain exclusive normative
JSON ownership; no active worker input is changed. Acceptance includes exact
cohort identity/source linkage, malformed/foreign/missing/duplicate row and
constraint rejection, independent validation cases and mandatory policy gates.
The validator grants zero execution/recovery/differential or parent credit.

Shared family validation passes eight focused tests with zero ignored cases.
Synthetic fixtures test authority/constraint rejection only, without IBM
behavioral credit. The existing Rust offline Draft 2020-12 schema owner checks
every present family instance; the explicit family command requires its file.
Mandatory gates precede this bounded validator seal.

## PROGRAM source contract integration

Manager integrates `SPI-1001.spi-program-contract` from the terminal CLI worker's
exact single-file handoff. Four PROGRAM rows carry 97 options, 47 response
triggers and 89 independent source-review case candidates. The manager checked
all four contracts against their pinned bodies and corrected SET PROGRAM's
INVREQ/17 trigger to include SHARESTATUS, with a remote-SHARED negative case
from dfha8_setprogram.html lines 190 and 237-239. Primary authority is
ibm-cics-ts-6x-spi-command-bodies-2026-09-12, catalog rows 0026/0084/0155/0241;
common command-format and CVDA sources are cited in each contract.

Exact command-body pins and constraints are checked through the existing Rust
Draft 2020-12 owner; Python jsonschema availability is not an acceptance bypass.
General CREATE/ATTRIBUTES/DISCARD/browse source closure, conditional operand
typing, SAF configuration, lock order and unspecified recovery remain explicit
gaps. This seal covers the bounded private contract input only; it does not
accept complete grammar, handlers, routes, recovery, licensed evidence or parent
SPI-1001. Denominators remain 269 SPI and 39 FEPI, with zero behavioral credit.

## Shared administrative grammar projection and binding reviews

Manager declares `SPI-1001.family-grammar-projection` before implementation.
The current IR exposes only administrative identities; it cannot carry the
reviewed PROGRAM option/direction/byte/constraint facts. The manager exclusively
owns the existing generate_spi1001_catalog.py owner and its focused tests,
cics_administrative.rs and IR facade, a derived Rust grammar projection, this
status, unique fragment and derived docs. It reuses existing CICS option and
constraint types. Source cases, verdicts and lifecycle prose never generate
handlers or routes; complete grammar status stays Pending until browse, CVDA
and conditional source closure are structured and reviewed. Acceptance proves
exact source/row binding, deterministic output, rejection of changed pins and
public promotion, preserved 269/39 denominators and application separation.

Manager also declares two independent read-only CLI review slices before
resuming the retained worker sessions:

| Slice | Rows and frozen input | Ownership and acceptance |
|---|---|---|
| `SPI-1001.fepi-pool-contract-review` | FEPI 0001/0007/0009/0018/0021/0034; frozen worker input bb414037950c2a0f6bcd37cc3757b6ffac72e7428ba0c66820d286fe08046f2c | External SPI worker fepi-pool-review handoff only; independent full six-row source/grammar/lifecycle/case review, exact defects and bounded repair recommendations. No tracked edits or runtime credit. |
| `SPI-1001.family-runtime-binding-review` | Four PROGRAM and six POOL rows, existing CICS state/resource/UOW/SAF/store owners | External FEPI worker runtime-binding-review handoff only; precise shared runtime/state/persistence/ABI/compiler ownership, first dependency-ready implementation slice, source gaps, and independent selected-route tests. No alternate dispatcher/database/mapper or tracked edits. |

Both use existing isolated worktrees and retained CLI sessions, gpt-6.1-sol high
with fast mode off and no nested workers. Offline pinned search/read applies;
no refresh, licensed execution, source inference, builds or semantic passes are
authorized for these reviews. Manager retains one serialized shared owner.

Shared IR grammar projection checks pass: seven focused Rust tests and
17 generator tests, with no ignored cases. Four PROGRAM source contracts
project 97 typed operand facts through existing CICS operand types. Every
contract retains Pending completeness; source candidates/response/lifecycle
prose and verdicts do not generate product behavior. Identity catalog bytes and
269/39 denominators are unchanged. Mandatory gates precede this bounded seal.

## Required alternative groups in shared family contracts

Manager declares `SPI-1001.family-alternative-constraints` before implementation.
The frozen family schema supports exclusions and dependencies but cannot state
that one selector from a group must be present. FEPI named/list selection thus
remains only prose, even though the existing CICS IR already has a bounded
CicsApplicationOptionAlternative type. This infrastructure slice adds optional
alternative_groups (members plus required) to the family schema, validates
declared unique members/groups, and projects them through that existing type.
Manager exclusively owns the schema, xtask validator, current generator/tests,
administrative IR module/derived projection, fragment and status/docs. Active
reviewer frozen schemas and contract bytes are unchanged. No command-specific
alternative is inferred or inserted by this slice; future reviewed contract
repairs supply actual groups. Acceptance includes backwards-compatible absent
groups, required/optional round trip, stale/dangling/duplicate group rejection,
Pending completeness and mandatory shared gates. No conditional expression DSL,
new parser or runtime admission is introduced.

Alternative-group infrastructure passes ten focused validator tests,
19 generator tests and seven IR compatibility tests, with no ignores.
Old family inputs remain valid and no command-specific alternatives are
invented. Dangling/duplicate/nonboolean groups fail validation; required
flags survive the common IR projection. Pending completeness, private
binding and zero behavioral credit remain. Mandatory gates precede seal.

## POOL source contract integration after independent review

Manager integrates `SPI-1001.fepi-pool-contract` after a complete independent
CLI review of all six command bodies, 77 options, 96 responses and 132 original
case candidates. The unchanged worker input is retained with its original hash.
R1 corrects five universal TD-routing statements using dfhp73i.html lines 91-147:
common CSZX versus pool EXCEPTIONQ follows the event type. R2 supplies concrete
ADD/DELETE node selections and SET OUTSERVICE for nine deny/later-function/
unknown-outcome cases; exact late error injection stays pending. R3 replaces an
unisolated duplicate-connection fixture with a supported mixed duplicate/new
node list case; the 175 mapping remains, but its isolated fixture is pending.
An independent SETFAIL/115-to-CSZX case raises the candidate count to 133.

SET POOL now represents required POOL-or-POOLLIST selection through the shared
alternative type, retaining POOLLIST/POOLNUM pairing and exclusions. The existing
IR generator projects these six rows alongside PROGRAM, with 174 operand facts
across ten partial contracts. Primary baseline is
ibm-cics-ts-6x-fepi-command-bodies-2026-09-12, rows
0001/0007/0009/0018/0021/0034; overview/TD authorities are
ibm-cics-ts-6x-fepi-context-candidates-2026-09-12 (dfhp7k4/dfhp73i).
Browse/value forms, exact SAF policy, syncpoint/rollback, detailed asynchronous
work, concurrency and restart remain explicit. No runtime or official verdict
is installed; all ten grammar completeness statuses remain Pending. Mandatory
source/instance/generator/IR/policy gates precede this bounded input seal.

## First PROGRAM state implementation slice

Manager declares `SPI-1001.program-status-observation` before CLI dispatch, from
sealed POOL/family candidate 6732a26979cd060862993581a43325cb62e2e298. The independent
runtime-binding review selects named PROGRAM STATUS as the smallest next path,
but trusted issuer/namespace/SAF/output/selected-route admission remain pending.
This dependency-ready implementation first supplies its private state observation
primitive inside the existing ProgramControl/CICS State owner.

Worker exclusively owns new program_control/administrative_status.rs and its
owned tests.rs. Manager owns and seeds the parent module registration and exact
handler inventory path, and later integrates facades/status/fragment/docs/gates.
Frozen API returns a private ProgramStatusObservation enum: Definition containing
one complete owned existing CicsProgramDefinition, NameOnly for compatibility
registration without a typed definition, or NotCatalogued for absence from this
program catalog only. It uses the existing normalization and one State mutex,
selects the same latest generation as current LOAD/transfer, and fails closed on
corrupt key/name/generation metadata. It introduces no second resource state,
persistence codec, status mutation, host operation, permission, response mapping,
public route or globally authoritative missing-resource result.

Acceptance requires true/false availability and exact generation/identity
snapshots, no host/load/store/UOW mutation, name-only distinction, bounded project
name policy, owned snapshot independence, corrupt metadata rejection, concurrent
registration coherence and unchanged-store service reconstruction. These are
private primitive tests, not selected-product-route, physical-restart or IBM
command execution credit. SPI row 0155 dfha8_inquireprogram.html establishes
STATUS and no-load behavior; existing immutable catalog codec/state contracts
remain authoritative. A later serialized manager slice binds real policy/context,
typed compiler/plan/host/output and selected-route proofs; full grammar, all
command gates and licensed acceptance remain pending. No v0.9 work is resumed.

## Source-ready family cohort enrollment

Manager declares `SPI-1001.family-cohort-enrollment` before implementation.
Enrollment adds three bounded private cohorts to the existing schema/validator/
generator owner: SPI FILE rows 0012/0072/0127/0224; FEPI resource rows
0008/0010/0011/0017/0019/0020/0022/0023/0024/0032/0033/0036/0037; and separate
FEPI SET POOLLIST row 0035. All have reviewed mapped command-body pins. The
existing six-row POOL contract remains a partial cohort; row 0035 is SET
POOLLIST, not SET PROPERTYSET, and must not be deduplicated into row 0034.
Likewise NODE/NODELIST and TARGET/TARGETLIST retain separate official rows
while sharing their reviewed command bodies. Source mappings supply locators,
never command behavior, and enrollment creates no new runtime authority.

Manager exclusively owns the family schema enum, xtask cohort tables, existing
generator cohort tables and synthetic validation expectations, a unique fragment,
this status and derived docs. The current IR and identity bytes remain unchanged
while new inputs are absent. Explicit selected-family checking requires its
file; the general schema gate validates only actually present inputs, without
claiming that all enrolled families or SPI-1001 are complete. Default explicit
family checking requires every enrolled cohort and will remain pending until
their files exist and pass. Acceptance covers exact mapped/pinned distinct
identities, declared-cohort synthetic shape validation, malformed/missing input
rejection and mandatory infrastructure gates, with zero semantic credit.

The next disjoint CLI source slices, dispatched only after this enrollment
commit passes, are `SPI-1001.spi-file-contract` (only families/spi-file.json)
and `SPI-1001.fepi-resource-contracts` (only families/fepi-resources.json and
families/fepi-pool-list.json). Source-reviewed option/constraint/condition/
lifecycle/SAF/audit/UOW/recovery facts and independent case candidates must be
derived through pinned offline search/read; explicit unresolved contexts stay
pending. Both own only their new normative JSON and external handoffs; all shared
facades, schemas, generators, state, dispatch, status and registration stay with
the manager. No source refresh, handler/readiness/route, generic success, licensed
execution or parent coverage is delegated. The independent PROGRAM state worker
continues owning its two private implementation files without shared edits.

Cohort enrollment passes ten focused validator tests (including all five
synthetic cohorts), 19 current generator tests and seven unchanged IR contract
checks, with no ignores. Current two inputs pass exact linkage; absent future
inputs remain pending. Identity/grammar output bytes do not change until a
reviewed input is added. Mandatory gates precede this infrastructure seal.

PROGRAM observation worker completed its two owned implementation/test files.
Manager independently reviewed the entire 46-line primitive and 535-line tests
and preserved their exact handoff hashes. It returns an owned whole highest
generation definition under one State mutex; validates every selected-name
generation key/name; distinguishes legacy membership and catalog-only absence;
and performs no host/store/load/UOW call. Nine new primitive regressions and two
existing LOAD compatibility checks passed in the worker. Same retained MemoryStore
reconstruction and same-owner thread coherence are limited primitive proofs,
not physical restart, selected-route or public namespace acceptance.

Worker module inventory reported its exact nested test path as unknown; manager
adds that path through the existing inventory owner, with no ceiling/guard waiver.
Worker offline dependency check stopped at a read-only advisory lock and normal
Git staging at its shared index sandbox boundary. These failure receipts remain
external; integrated mandatory module/dependency/Git gates must pass afresh on
the manager candidate. The frozen parent module registration is the only facade
change. Current command/source pins, immutable codec and all Pending coverage
remain unchanged. No command handler, CVDA/output mapping, authorization grant,
resource namespace, mutation, public route or v0.9 acceptance is introduced.

Integrated primitive gates pass nine new observation tests plus two exact LOAD
compatibility regressions, no failures/ignores. Actual module inventory and offline
dependency policy now pass in the manager checkout. All command and licensed
gates remain Pending; these checks earn zero administrative execution credit.

## Second source-ready parallel wave

Manager declares `SPI-1001.source-wave-two-enrollment` before changing shared
cohort tables. Existing five cohorts and active FILE/FEPI resource workers retain
their exact ownership. The next five bounded private cohorts contain 90 additional
mapped rows, raising declared distinct scope to 79 SPI and all 39 FEPI identities
(118 total). This is declared scope, not completed facts or behavioral coverage.
Three unresolved SPI equivalences and all remaining unmapped grammar facts remain
pending. No identity/EIBFN/name can substitute for a verified command body.

Cohorts and exact official row suffixes:

- spi-csd-definition: 0037/0038/0040/0041/0042/0053/0054/0055/0056/0060/0061.
- spi-csd-browse: 0039/0043/0044/0045/0046/0047/0048/0049/0050/0051/0052/0057/0058/0059.
- spi-monitoring-control: 0002/0095/0116/0139/0148/0159/0164/0173/0174/0175/0177/0195/0218/0234/0238/0243/0244/0253/0254/0255/0257.
- spi-region-lifecycle: 0004/0065/0096/0100/0101/0113/0157/0160/0161/0163/0165/0166/0183/0184/0185/0186/0199/0202/0205/0208/0215/0245/0246/0261/0262.
- fepi-session-data: 0002/0003/0004/0005/0006/0012/0013/0014/0015/0016/0025/0026/0027/0028/0029/0030/0031/0038/0039.

After the shared enrollment commit passes, four disjoint CLI slices may start
from that exact candidate: `SPI-1001.spi-csd-contracts` owns only
families/spi-csd-definition.json and families/spi-csd-browse.json;
`SPI-1001.spi-monitoring-contract` owns only families/spi-monitoring-control.json;
`SPI-1001.spi-region-contract` owns only families/spi-region-lifecycle.json;
`SPI-1001.fepi-session-contract` owns only families/fepi-session-data.json. Paths
are relative to conformance/0.10/cics. Workers also own their external handoffs
and receipts. Together with the current two source workers this is six CLI
workers, within the requested ceiling of eight. No nested workers are authorized.

Shared schema/validator/generator, IR/compiler/host/provider facades, resource
state, security, dispatch/registration, status/docs and fragments remain the
manager's serialized lane. Enrollment must prove exact mapped/pinned rows,
disjoint cohorts and the existing 32-command artifact bound, synthetic validation,
unchanged identity/current grammar bytes and mandatory infrastructure gates.
Workers must use pinned offline search/read for complete selected bodies and
necessary contexts, preserving exact source versions and distinct variants.
They derive bounded option/constraint/condition/resource/lifecycle/security/audit/
UOW/quiesce/timeout/failure/recovery facts and concrete independent case candidates.
Missing authority stays explicit. All six command acceptance gates remain pending;
contract shape, source review and optional private primitives earn zero execution
credit. Artifact limits, generated owners and the existing shared ConformanceIR
authority remain unchanged. No source refresh, runtime routing, blanket syncpoint/
rollback, public capability, licensed credit or parent completion is delegated.

Cohort enrollment passes eleven focused validator tests (including all ten
synthetic cohorts), 20 current generator tests and seven unchanged IR contract
checks, with no ignores. Current two inputs pass exact linkage; absent future
inputs remain pending. Identity/grammar output bytes do not change until a
reviewed input is added. Mandatory gates precede this infrastructure seal.

## Form-specific private operand projection

Manager declares `SPI-1001.family-form-constraints` before implementation. The
existing union option projection cannot represent PROGRAM/POOL/NODE receiver
direction changes between named inquiry and NEXT or bare resource keywords in
START/END. This shared slice adds optional bounded forms using the same existing
CICS operand/alternative/dependency types. Each form has a stable local fact ID,
positive selector options, its own closed option/constraint set and primary-source
line locators. Form options must be a subset of the parent source union; selector
options must be required and declared in that form. Forms are source facts, not
a parser, dispatch selector, case predicate DSL, runtime capability or ConformanceIR.

Manager owns schema, actual xtask validator, existing sole generator, IR source
fact type/facade and generated projection, focused structural regressions, one
fragment, status and docs. Current six workers keep their frozen schemas and
exclusive family paths; optional fields preserve their inputs. No existing
contract receives guessed form facts. Absent/empty forms preserve the previous
product-fact digest; nonempty forms affect it while case/lifecycle/source-line
metadata stays excluded. All parent and form completeness remain Pending. No
execution identity, plan codec, compiler admission, provider/host route, resource
state, permission, response mapping or public capability changes. Numeric CVDA
domains, supplemental citations, value-dependent/context/version rules and full
grammar completeness remain explicit later obligations.

Acceptance proves old input compatibility, bounded and sorted unique form IDs,
closed per-form constraints/selectors, direction-preserving deterministic product
projection and digest sensitivity, unknown-field/invalid-reference/oversized-form
rejection, plus all mandatory gates. Synthetic forms exercise infrastructure,
not IBM behavior. Full source reviewed forms are a later independently gated
input enrichment through the same owner, not implied by this infrastructure seal.

Form infrastructure passes thirteen focused actual-validator tests, including
fourteen malformed form payloads, 23 generator checks and seven IR regressions,
without ignores. Current inputs retain their prior product-fact digest; no form
facts are invented. Synthetic form projection preserves direction and excludes
source-line/case metadata. Mandatory policy/shape/architecture/docs gates precede
this bounded infrastructure seal; all administrative execution credit stays zero.

## Independent FILE and FEPI resource input review

Two source CLI workers completed and cleaned their targets, retaining unsealed
owned inputs after normal shared Git index denial. FILE contains four rows,139
operand facts,91 condition records and166 case candidates; FEPI resource/list
contains fourteen distinct rows,178 operand facts,210 condition records and213
case candidates. These are review candidates, not accepted command behavior.

Before cross-worker review dispatch, manager declares
`SPI-1001.spi-file-contract-review` and `SPI-1001.fepi-resource-contract-review`.
Each owns only its external review handoff/receipts and a clean isolated checkout
from the form-infrastructure candidate; no tracked writes or source input repair
is delegated. FILE review covers rows0012/0072/0127/0224 and all166 candidates;
FEPI review covers rows0008/0010/0011/0017/0019/0020/0022/0023/0024/0032/0033/0035/0036/0037
and all213 candidates. Each retained reviewer session is different from that
input's producer. Exact owned input hashes and complete source inventories are
frozen by the external launch contract. Dependencies are terminal producer
handoffs and the passed shared schema/form gates; source bodies must again be
consulted via pinned offline search/read for independent expectations.

Review requires every operand/constraint/response/lifecycle/security/audit/gap
and concrete case input/expectation, including all fixed-size arrays, to be
accounted for. Compare primary and contextual authority, identify overclaims,
wrong bounds/directions/conditions/ordering/rollback/event routing, variant
identity collapse, unsafe incomplete fixtures and unsupported source claims.
FILE's exact syncpoint and ordered/deferred/ignored attribute effects, retained
locks/BUNDLE constraints and restart/IOERR preservation must remain precise.
FEPI's named/list and browse directions, per-item successes, async request versus
attainment, CSZX/EXCEPTIONQ loss, and SET NODE174 versus index176 conflict stay
explicit. Structural schema pass is not semantic proof. No guessed form/CVDA/SAF
closure, selected-product route, runtime/license credit or parent completion is
allowed. Manager reviews full independent reports and repairs/integrates one
input slice at a time; all common authority and candidate sealing stays serial.

Manager also declares dependency-ready `SPI-1001.spi-program-forms` before CLI
dispatch, based on the passed form-infrastructure candidate. Exclusive tracked
ownership is the existing families/spi-program.json only (relative to
conformance/0.10/cics); original four rows0026/0084/0155/0241 and all reviewed
primary pins remain unchanged. The worker may add source-reviewed form-specific
constraints/directions and independent grammar candidate cases supported by the
new optional bounded form fields, keeping unreviewed AT/browse/generic-context
facts explicit. It must not guess forms, collapse source variants, infer a
synthetic runtime status profile as the full IBM command, or promote readiness.
Full grammar/CVDA/context/authorization/selected-route/recovery remain Pending.
The current private PROGRAM observation has no handler/route, and no shared
IR/compiler/host/provider/schema/generator/status changes are delegated.
Acceptance binds complete offline source reads, exact inherited fact/pin
compatibility, source-backed per-form closed constraints/selectors/directions,
independent malformed/boundary cases, current schema/generator/IR and mandatory
manager integration gates. The worker owns only its input and external handoff;
all integration and generated products stay serial. This is the seventh live
CLI slot after two producers ended and two independent reviewers took their slots.

Manager declares bounded read-only `SPI-1001.spi-row-link-review` before dispatch
for the three still-unresolved SPI equivalences0201/0203/0204. Its only ownership
is an external review report/receipts and read-only isolated checkout. Earlier
wide qualification reviews remain preserved and must not be rerun. This review
tests a distinct safe alternative: inspect the exact retained official catalog
row's raw HTML links and the pinned candidate body's syntax/cross-reference
roles, preserving hash/product/version. A direct published catalog-row link to
the exact command topic or explicit pinned identity equivalence could establish
a join; names, prefix similarity, EIBFN, body keywords or mandatory-action guesses
alone cannot. No shared source-map/locator/manifest/code edits, repins, refresh,
licensed execution or mapping acceptance is delegated. If this evidence is
absent, retain unresolved status and exact smallest external authority needed.
Manager independently verifies any proposed bounded proof through the existing
source-map/form-locator owners and gates before integration. This eighth CLI
slot remains within the requested ceiling and earns no behavioral credit.

Manager declares source-repair followups SPI-1001.spi-file-review-repair and
SPI-1001.fepi-resource-review-repair before retained CLI session dispatch.
FILE owns only families/spi-file.json, rows0012/0072/0127/0224 and existing
166 candidate cases; repair independent findings SPI-FILE-R1/R2/R3/R4. FEPI
owns only families/fepi-resources.json and fepi-pool-list.json, fourteen
previously declared rows and213 cases; repair FEPI-R1/R2/R3. Each writes only
its isolated terminal producer workspace and new external repair handoff.
The original handoffs and source hashes are frozen; no shared schema, generator,
IR/compiler/provider/security/status or generated authority is delegated.
Dependencies are terminal producer and independent review, reviewed retained
primary/context source pins, and passed common constraints. Require pinned
Python search/read before any repair, concrete independent fixtures and bounded
source facts; preserve unresolved contradictions, timing, variants, SAF/UOW/
recovery, licensed and selected-route gates. Manager and separate reviewers
verify changed facts before serial integration and required gates. All
308 identities remain; no execution acceptance or v0.9 acceptance is claimed.

SPI-1001.spi-program-forms manager review preserves all97 inherited union
operands,47 condition records and89 existing cases exactly. Four named forms
retain93 form operand facts, with17 new independent grammar/boundary candidates
(106 PROGRAM cases). Only the named INQUIRE application-context operands refine
union input-output to output; OPERATION remains a64-character receiver. PROGRAM
is required within named forms; STATUS remains optional. START/AT/NEXT/END forms
are absent and unresolved. Source baseline ibm-cics-ts-6x-spi-command-bodies-
2026-09-12, official rows0026/0084/0155/0241, exact dfha8_createprogram,
dfha8_discardprogram, dfha8_inquireprogram and dfha8_setprogram topic pins remain
unchanged. Common-format dfhp4_apiformat and CVDA dfha80x are supplemental
reference only. All constraints stay Pending and behavioral/license credit0.
Two retained CLI producers now repair four FILE and three FEPI independent
findings; the other producer handoffs await independent review and integration.

Manager declares four dependency-ready read-only cross-worker reviews before
CLI dispatch: SPI-1001.spi-csd-contract-review (25 CSD rows,475 cases),
SPI-1001.spi-monitoring-contract-review (21 monitoring/control rows,768 cases),
SPI-1001.spi-region-contract-review (25 region/UOW rows,868 cases), and
SPI-1001.fepi-session-contract-review (19 FEPI conversation/data rows,779 cases).
Exact row sets are the existing enrolled family cohorts; final producer workfile
hashes are frozen by external launch manifests, not their possibly stale indexes.
Each owns only its external review handoff and a separate clean read-only
checkout. No tracked repair or shared schema/IR/generator/registry/status write
is delegated. Every property, operand, constraint, response, lifecycle/security/
audit/UOW/recovery fact, gap and concrete candidate fixture/expectation requires
independent pinned search/read and relational review; no silent case sampling.
Source conflicts and insufficient fixtures become actionable pending findings,
not runtime passes. Exact source/hash/line coverage, scope preservation and
changed-input recommendations are acceptance requirements for these reports.
Separate retained reviewer sessions differ from each input producer. Manager
owns serial repair/integration, actual schema and mandatory gates and sealing;
all source/execution/recovery/licensed/parent and application acceptance stays
pending. Together with two repair workers this uses six actual CLI slots.

Named PROGRAM forms pass23 generator checks, seven IR regressions and the
actual Rust Draft202012 family instance checker on this integration candidate.
The generated grammar digest changes for four explicit named forms; all parent
and form readiness remains Pending. Union174 operand facts remain unchanged,
and239 source-review candidates are not execution evidence. Mandatory gates
precede this bounded slice seal.

Manager source-wave-three enrollment declares two disjoint dependency-ready
source slices before dispatch: SPI-1001.spi-web-resource-contract owns only
families/spi-web-resources.json (25 exact SPI rows), and
SPI-1001.spi-queue-storage-contract owns only families/spi-queue-storage.json
(30 exact SPI rows). Full exact rows and committed primary pins are frozen
by source-wave-three/cohorts.json outside Git and by the sole generator/validator
cohort inventories. Each row has a reviewed source-map join; four TSQUEUE/TSQNAME variants retain
their distinct reviewed combined-page form locators; no unresolved
0201/0203/0204 equivalence is included. Dependencies are this enrollment's passed
shared schema/cohort/linkage/policy gates. Workers independently search/read all
primary and needed pinned contexts, derive complete bounded option/constraint/
response/security/audit/lifecycle/UOW/recovery facts and concrete independent
candidates, preserving unregistered contexts and contradictions pending.
Each owns only its isolated family file and external handoff; no shared
facade/schema/generator/IR/compiler/provider/state/status edits are delegated.
Manager owns actual Draft202012 and mandatory integration gates, independent
review, repairs, generated output and serial sealing. Existing source contracts
and generated facts remain unchanged by enrollment; absent future inputs stay
pending. All308 identities and all application/route/recovery/license/parent
gates remain pending. Six current repair/review workers plus these two source
workers use at most eight actual CLI slots, gpt-6.1-sol/high/default/fastOFF.

Cohort enrollment passes thirteen focused validator tests (including all twelve
synthetic cohorts), 23 current generator tests and seven unchanged IR contract
checks, with no ignores. Current present inputs pass exact linkage; absent future
inputs remain pending. Identity/grammar output bytes do not change until a
reviewed input is added. Mandatory gates precede this infrastructure seal.

SPI-1001.fepi-resource-contracts manager integration consumes the independently
reviewed and repaired final resources/list inputs, fourteen distinct FEPI rows
0008/0010/0011/0017/0019/0020/0022/0023/0024/0032/0033/0035/0036/0037.
These retain178 union operand facts,210 condition records and229 source case
candidates (213 originals plus16 reviewed journaling/device candidates). Exact
original fields, all2048 fixed-array entries and all POOLLIST bytes are preserved
except the five independently reviewed lifecycle fields and sixteen new cases.
FORMAT refers to outbound/inbound character attributes; MSGJRNL has its four
direction-specific meanings, and DEVICE retains twelve sourced mode/model/line
mappings. INSTALL stores policy for later data, not installation-time data
journaling. Baseline ibm-cics-ts-6x-fepi-command-bodies-2026-09-12, catalog
ibm-cics-ts-6x-2026-08-31:fepi-commands and exact dfhp737/dfhp73b command pins;
formatted context dfhp74l uses ibm-cics-ts-6x-fepi-context-candidates-2026-09-12.
All other primary/context pins and the reviewed combined-page variant identities
are unchanged. SET NODE/NODELIST OPEN ACB174 versus reference-index176 remains
unresolved with both pins; no runtime mapping is selected. CSZX/EXCEPTIONQ
event/loss and product-audit distinctions remain. Grammar unions are partial;
named/NEXT directions, CVDA compatibility, source contexts, precise SAF, async
admission/attainment, UOW/lock/restart/concurrency and product-route/licensed
obligations stay pending. The sole generator projects options only, never
fixture-driven execution, lifecycle prose or verdicts. All308 command identities
and260 advertised application typed registrations are preserved; runtime credit0.

Before dispatch manager declares bounded read-only SPI-1001.spi-file-repair-review
for final repaired FILE rows0012/0072/0127/0224. It depends on terminal producer
repair and freezes its exact final hash/change inventory before launch. Review
all changed fields/case relationships (currently104 fields across29 cases) and
verify exact preservation of the137 unaffected cases, all139 operands,91
conditions and source/security/gap records against the original fully reviewed
input. Reuse unchanged earlier review coverage; do not repeat whole unchanged
source campaigns. Consult pinned source authority for changed CVDA fixtures,
file-kind/state/boundary isolation, unresolved EMPTYREQ competing outcomes and
raw-syntax default CSDL logging. Its only write ownership is external review
receipts in an isolated clean checkout; no tracked repair/shared authority or
acceptance seal is delegated. Manager serially fixes residual findings and
integrates with actual schema/projection/policy gates. No execution or licensed
credit; the retained independent reviewer differs from FILE's producer.

FEPI resource/list integration passes23 current generator tests, seven IR
regressions over every generated fact, and the actual Rust Draft202012 instance
checks for both files. The integrated private source projection now has24 rows
(4 SPI and20 FEPI),352 union operand facts and468 source case candidates.
These remain partial source contracts with Pending constraints and no handlers,
application/selected-route/recovery/license acceptance or execution credit.
Mandatory current-candidate gates precede this bounded source slice seal.

Manager serialized slice SPI-1001.family-cvda-domains addresses the demonstrated
absence of structured symbolic CVDA domains in common family/form projection.
Only manager owns shared schema, actual validator, sole generator/tests, common
CICS operand type, administrative IR facade/projection, fragment/status/docs.
Optional bounded domains tie reviewed symbolic values to a declared fullword
valued operand, including form-local direction. No numeric CVDA table, implicit
aliases, predicate DSL, compiler admission, handler or response mapper is added.
Absence/empty metadata must preserve the old fact preimage/digest; existing
family facts are unchanged and no reviewed source values are invented. Scope
requires malformed-domain regressions, generator/digest/source-line exclusion,
actual Draft202012/schema/module/API/format/architecture/docs/changelog/deny
gates and cleanup before this infrastructure seal. Existing live source workers
retain their frozen old-schema inputs; manager enriches values only after source
review. Generic dfha80x source-b pin81f101e030365400b431ecf68250dfcabc5673e1acbf05010c9285bf590e3b25
lines3-15 distinguishes finite named values, fullword storage and direction;
lines16-38 keeps aliases/DFHVALUE/numeric reference separate. All administrative
readiness and execution/recovery/differential/parent/application gates pending.

Symbolic CVDA domain infrastructure passes15 focused actual-validator tests
(including11 malformed domain payloads),26 generator regressions and seven
IR source/compatibility regressions without ignores. Parent and form domain
projection preserves scoped symbols and excludes source-line metadata; absent
and empty domains preserve the previous product-fact preimage/digest. Current
24 source rows retain their exact prior grammar digest and no domain facts are
invented. All constraints stay Pending; numeric codes/aliases/context predicates
and compiler admission remain separate unfulfilled obligations. Mandatory
current-candidate gates precede this bounded infrastructure seal.

Before dispatch, manager declares three bounded dependency-ready source slices:
SPI-1001.spi-monitoring-review-repair owns only spi-monitoring-control.json,
all21 declared rows/768 original cases, resolving MON-R1 through MON-R5 from
terminal independent review. Preserve all primary pins/operand/response/case
identities; isolate CVDA and eight-byte RESID fixtures, and retain opposed ADD
defaults and TABLESIZE preservation as pending. SPI-1001.fepi-session-review-repair
owns only fepi-session-data.json, all19 rows/779 original cases, resolving FSR-01
through FSR-08: exact variant selectors/bytes/outputs, negative-case isolation,
temporary ownership, current named/browse form vocabulary and ambiguous formatted
error precedence. Both depend on their completed read-only reviews; workers
freeze originals and supply complete changed-field/case preservation inventories.
SPI-1001.spi-program-cvda-domains owns only spi-program.json, four existing
named rows0026/0084/0155/0241. It depends on sealed common family-cvda-domains
and independently reads pinned complete primary bodies to add finite symbolic
operand/form domains only. Preserve97 union operands/47 conditions/106 cases
and all preexisting form facts; no numeric code/alias/context resolution is
delegated. All three use isolated exclusive workspaces, actual retained CLI
sessions gpt-6.1-sol/high/default/fastOFF, no nested workers. Shared schema,
generator, IR/facades/registry/status/resource/security/effects remain solely
manager owned. Manager owns separate changed-fact review, actual Draft202012,
projection and mandatory policy gates and serial sealing. At most eight workers;
all administrative/application/selected-route/recovery/license/parent gates
remain pending and execution credit stays zero. Three deferred v0.9 identities
retain pending gates. These declarations do not admit or advertise commands.

Manager slice SPI-1001.source-wave-four-enrollment closes the demonstrated
cohort inventory gap for132 remaining reviewed SPI row joins. Six future bounded
source slices are declared before dispatch: spi-network-connections24,
spi-terminal-sessions27, spi-database-messaging21, spi-platform-programs19,
spi-event-policy19 and spi-transaction-resources22. Each SPI-1001 family owns
only its matching families/<family>.json in a separate checkout; exact rows are
the serialized generator/validator cohorts and frozen external source-wave-four
locators. No worker dispatch occurs before this manager infrastructure seal or
beyond the eight CLI cap. Current live workers keep their frozen schema/base.
These six depend on this enrollment's disjoint/pinned/cohort/schema policy gates
and then complete independent pinned search/read before semantics. Manager owns
schema, sole generator, actual validator, IR/facades/security/resource/effects,
status, independent review, integration gates and all serial acceptance seals.
Future files remain absent/pending; existing facts and generated bytes must
remain unchanged. All266 reviewed SPI and39 reviewed FEPI joins are now assigned
to18 disjoint bounded cohorts; SPI0201/0203/0204 remain unenrolled pending actual
source identity authority, with all269/39 denominator identities preserved.
No CVDA domain, grammar completeness, handler/route/application/recovery/license
acceptance follows from family enrollment. Independent lifecycle, security,
quiesce, timeout, failure and recovery bindings remain required. Numeric wire
values and version-qualified sources remain separate review obligations.

Source-wave-four infrastructure passes15 actual-validator tests, covering all18
synthetic cohorts, and26 generator checks including exact complete equality to
all305 reviewed row joins and exclusion of allthree unresolved identities.
Current generated identity and grammar bytes remain unchanged; seven prior IR
regressions from41033cc apply to these unchanged generated/IR inputs and are
not relabeled current execution evidence. Present private family instances pass
exact source linkage; future absent inputs stay pending. Mandatory gates precede
this bounded enrollment seal.

Before dispatch manager declares SPI-1001.spi-csd-review-repair, exclusive to
spi-csd-definition.json and spi-csd-browse.json in the original now-idle producer
checkout,25 rows/475 original case identities. It repairs allnine CSD findings:
INSTALL selector form, LIST ownership/lookup/locks, COPY duplicates, precise
option citations, sourced fullword CVDA widths and explicitly pending unpinned
character fixtures. SPI-1001.spi-region-review-repair owns only
spi-region-lifecycle.json,25 rows/868 original case identities, repairing all13
terminal findings: UOW CVDA domains/phase fixtures, isolated range/encoding,
named TASK selector, actual ASSOCIATION outputs/layouts, bundle transition
preconditions, exact citations and opposed duplicate/KILL source outcomes.
Dependencies are completed independent reviews and frozen latest workfile hashes;
all unaffected properties/cases/source pins/gates must be preserved. Existing
CSD staged historical draft is untouched; working bytes are authoritative.
Both use existing shared schema/form/CVDA types read-only and retained actual
CLI sessions gpt-6.1-sol/high/default/fastOFF, with at most eight workers and no
nested workers. Manager owns all shared schema/generator/IR/resource/security/
status paths, separate changed-fact review, actual instance/policy gates and
serial seals. Neither is runtime/route/application/recovery/license acceptance.

SPI-1001.spi-file-contracts integrates four exact FILE rows0012/0072/0127/0224,
139 union operands,91 source conditions,18 obligations and166 candidate cases.
Independent original and changed-fact reviews preserve all137 unaffected cases
and every source/grammar/response/authorization/gap record, with104 bounded
field changes across29 cases. Fullword DFHVALUE(DPLSUBSET) wrong-domain inputs
are independently sourced; only CREATE explicitly seeds FULLAPI. Sixteen SET
fixtures isolate actual VSAM/BDAM/CFDT kind/state/prior values; BUSY requests
CLOSED. Ten numeric boundaries distinguish definition storage from next OPEN
validation and dataset/server-table effects. EMPTYREQ no-load/already-loaded
ignore versus INVREQ16/57 stays explicitly unresolved with both source passages.
CREATE raw SVG's LOG default is a bounded retained-source fact; explicit diagram
notation promotion and CSDL delivery/payload remain pending, not product audit.
Baseline ibm-cics-ts-6x-spi-command-bodies-2026-09-12, catalog
ibm-cics-ts-6x-2026-08-31:spi-commands-unique, exact dfha8_createfile f1c4d6c,
dfha8_setfile ec8a39c and unchanged DISCARD/INQUIRE pins are retained.
CVDA dfha80x uses source-b baseline2026-09-10 pin81f101e. Manager corrects the
low external handoff coverage overstatement in provenance-preserving copies;
the reviewed normative JSON stays byte-identical. Grammar/form/numeric ABI,
precise SAF, effects/UOW/locks, async completion, restart/concurrency and actual
selected-route/application/license gates all remain pending. No handlers or
public routing/admission are added; all308 identities and260 existing advertised
application registrations remain unchanged. Current actual instance/projection
and mandatory gates are required before this bounded private source slice seal.

The first FILE integration attempt passed26 generator tests,7 IR regressions
and the actual family instance check, then stopped at the mandatory module gate:
the generated monolithic grammar reached1316 lines. Cleanup completed; no seal
or successful-candidate receipt was issued. Manager extends this same bounded
integration ownership to the sole generator/tests and generated grammar chunks,
packing complete commands below1000 lines with one ordered facade and unchanged
global product-fact digest. No exemption or module ceiling is increased. Scale
and missing/extra/modified chunk regressions must verify no lost/reordered facts
and exact freshness. Workers retain exclusive source-file ownership; this
structural generator repair changes no source, readiness or runtime authority.

Before dispatch manager declares three independent external-only review slices:
SPI-1001.spi-program-cvda-review freezes the terminal four-row enrichment and
reviews all eight added parent/form domain arrays, every finite symbol/direction/
width/citation and exact restoration of97 union operands/47 responses/106 cases.
SPI-1001.spi-monitoring-repair-review freezes the terminal21-row repair, reviews
all73 property changes/29 affected cases for MON-R1..R5 and verifies739 unaffected
cases plus source/grammar/response/authorization preservation. Reuse unchanged
earlier full review, without repeating its source campaign. SPI-1001.spi-web-contract-review
freezes the terminal25-row input and independently reviews every336 operand,
134 condition, six named form and549 candidate input/expectation, lifecycle/
authorization/audit/UOW/recovery/gap and primary source relation. Each depends
on terminal producer handoff and exact frozen hashes, owns only external
reports in a clean isolated checkout and consults pinned search/read before
semantic evaluation. Reviewers differ from each input's producer. All shared
schema/generator/IR/resource/security/status and serial integration gates remain
manager-owned; no tracked edits or acceptance seals are delegated. At most
eight actual retained CLI workers, gpt-6.1-sol/high/default/fastOFF; all execution/
application/selected-route/recovery/license/parent gates remain pending.

FILE integration passes28 current generator regressions, seven IR source
regressions without ignores and the actual Draft202012 FILE instance gate.
Current private source projection contains28 rows (8SPI/20FEPI),491 union
operand facts and634 source case candidates. These are pending contracts,
not accepted typed execution or actual selected-route/recovery evidence.
Mandatory current-candidate gates precede the bounded source seal.

Before dispatch manager declares three independent changed-fact review slices:
SPI-1001.fepi-session-repair-review: 19 rows,222 operands,380 conditions,779 preserved identities; every678 changed/new/deleted property and82 repair actions,41 changed cases and exact738-case preservation. Review all FSR-01..08 pointers, concrete extent/actual receivers and isolated malformed inputs. Independently review all five new INQUIRE forms in full including NODE/TARGET direction and browse constraints; no numeric aliases or unsupported precedence.
SPI-1001.spi-csd-repair-review: 25 rows,443 operands,250 conditions,475 preserved identities; all352 changed properties,49 changed cases and426 exact unchanged cases. Review every CSD-R01..09 pointer including232 corrected citation arrays,14 actual CVDA widths/eleven notes,27 pending character predicates and precise command-selected locks. Final working bytes are authoritative; historical staged draft MUST NOT be reset or used.
SPI-1001.spi-region-repair-review: 25 rows,509 original operands,156 conditions,868 preserved identities; all41 changed property groups/165 atomic changes,17 changed cases,851 unchanged cases and16 unchanged commands. Review all13 finding resolutions, TYPE/REASON finite domain arrays and values, TASK required selector, EXCI packed receiver, ASSOCIATION layouts/defaults and isolated bundle/state/range fixtures. LOGDEFER encoding, END-before-START, duplicate CREATE phases and prior-PURGE-only KILL outcomes remain explicitly pending with both opposed authorities.
Each freezes terminal producer working bytes and depends on the original full independent review, verified unchanged source pins and complete repair inventories. Reviewers differ from producers, own external reports only in clean isolated worktrees and consult pinned search/read before semantic assessment. Manager owns current schema/generator/IR/resource/security/status, actual instance/policy gates and serial integration. Original repairs were declared before dispatch in the status workfile and are now retained in sealed f1c4481; older53068c lacks their follow-up paragraphs. At most eight actual retained CLI workers use gpt-6.1-sol/high/default/fastOFF, with no nested workers. All runtime/application/route/recovery/licensed/parent gates stay pending, credit0.

Before dispatch manager declares SPI-1001.spi-queue-storage-contract-review, independent external-only full review of30 rows,337 union operands,204 conditions and903 source case candidates including four TSQUEUE/TSQNAME forms. It freezes terminal producer SHA3f4edc8e and depends on reviewed row joins/current common schema. Every normative property and case input/expectation relationship, lifecycle/authorization/audit/UOW/recovery fact and gap must be reviewed with complete primary pinned search/read. Reviewer differs from producer, owns only external reports in an isolated clean checkout and may not repair or edit shared/tracked paths. Manager retains all schema/generator/IR/security/status, actual instance/mandatory gates and serial seals. At most eight actual retained CLI workers; execution/application/route/recovery/license/parent gates remain pending,credit0.

SPI-1001.spi-program-cvda-domains integrates exactly eight added arrays in the four existing PROGRAM rows0026/0084/0155/0241. Thirty parent domains and thirty named-form domains each retain100 source spellings; DISCARD arrays are empty. Independent complete review and manager removal proof restore every original byte,4211 inherited nodes and106 cases. CREATE LOGMESSAGE is input; INQUIRE domains are outputs; SET has seven inputs and conditional VERSION output without a COPY presence dependency. INQUIRE COPY/RUNTIME differ from SET; PLI/PL1 and MAP/MAPSET retain source spellings with numeric/alias acceptance pending. Exact primary baseline ibm-cics-ts-6x-spi-command-bodies-2026-09-12, catalog ibm-cics-ts-6x-2026-08-31:spi-commands-unique and dfha8_createprogram76455ab,discardprogram93e34c,inquireprograme3d8ed,setprogram6775356 pins are unchanged. CVDA context dfha80x81f101e uses application-api-sources-b2026-09-10. Source grammar completeness/context/numeric ABI/SAF/effect/recovery/dependency/runtime/route/license and parent gates remain pending; no compiler admission or handlers are added. Manager owns generated chunks, focused compiled value-distinction regression and actual instance/mandatory gates before serial source seal.

The first PROGRAM domain integration stopped at the existing absent-versus-empty compatibility test: its purported absent fixture now inherited the new nonempty domains. The generator was correct; manager makes the test remove both parent and form arrays explicitly before comparing absent/empty inputs. This preserves the compatibility assertion and includes the fixture repair in this slice's exact allowlist. Original failed receipt and successful cleanup are retained; actual gates must rerun after this changed input.

PROGRAM CVDA source integration passes28 generator regressions,eight IR regressions
and the actual Draft202012 PROGRAM instance gate. Thirty finite scoped domains
are retained in both parent and named forms, with100 symbols per scope; numeric
ABI/aliases/context completeness and all execution gates remain pending.

Before dispatch manager declares SPI-1001.spi-monitoring-response-review, an independent external-only follow-up to terminal monitoring repair review MON-R4-OBS. Manager's proposed external repair changes only ten input properties in two cases: set-sysdumpcode.invreq-7-10 and set-trandumpcode.invreq-7-8, naming actual fullword sender/storage and RESP/RESP2 receivers while preserving conditional outcomes and all other fields. The reviewer freezes SHAfd707eff3, reviews every old/new value and relation, restores the exact65a355b original and all766 other cases, and reuses verified unchanged original full/73-property reviews without repeating their campaign. Primary/common-format implications require pinned search/read. Reviewer owns external reports only in a clean isolated worktree; source producer workfile is untouched. Manager retains shared schema/generator/IR/status, actual instance/policy gates and serial integration. At most eight actual retained CLI workers gpt-6.1-sol/high/default/fastOFF; all execution/application/route/recovery/license/parent gates stay pending,credit0.

Before dispatch manager declares SPI-1001.spi-web-review-repair, source-only ownership of spi-web-resources.json in its idle original producer checkout,25 rows/549 original cases. It repairs five terminal independent finding groups: unsupported URIMAP DESCRIPTION keyword6, mandatory COPY/NEWCOPY choice plus one selector-only rejection candidate, enabled and available redirection/alias fixtures,33 exact semantic citation arrays, and isolated CICS-file authorization for ATOMSERVICE628. Original IDs/pins/unaffected facts/gates remain preserved. Current shared schema/form/domain vocabulary is read-only; stale schema provenance wording and unresolved LOG notation/default advisory are retained precisely. The reused CLI reviewer is terminal and becomes a bounded repair author; original author is busy in a distinct queue review checkout. Manager owns separate changed-fact review, schema/generator/IR/status/resource/security, actual instance/policy gates and serial seals. At most eight actual retained CLI workers gpt-6.1-sol/high/default/fastOFF, no nested workers. All execution/application/route/recovery/licensed/parent acceptance stays pending,credit0.

Manager slice SPI-1001.spi-monitoring-source-contracts integrates21 exact monitoring/control rows,635 union operands,142 source conditions,101 obligations and768 candidate cases. Original complete independent review, all73 producer repair operations and separate ten-property receiver repair review preserve739 unaffected original cases and all source/grammar/response/authorization facts. Obsolete COLLECT selector/form remains pending; sixteen RESRC001 fixtures have exact eight-byte IBM037 storage with specific record/READ prerequisites. TABLESIZE current16/request1/result16 keeps deletion/preservation/suspension pending rather than inferring comparison ordering. Valued ACTION(A) REMOVE candidates have actual four-byte A/M senders and distinct writable R/R2 response receivers; bare REMOVE plus MAXIMUM remains separate static rejection. Omitted ADD MAXIMUM/DAE retains command-body999/NODAE against dfhs14a new-or-added SIT authorities, with explicit opposed outcomes pending. Primary baseline ibm-cics-ts-6x-spi-command-bodies-2026-09-12 and catalog ibm-cics-ts-6x-2026-08-31:spi-commands-unique source pins are unchanged; SET SYSDUMPCODEffd4eadd,TRANDUMPCODEb1bd0e8a,TRACEDEST and COLLECT/EXTRACT remain pinned. Context dfha80x81f101e/formatd88e2001 uses sources-b2026-09-10 and dfhs14a1852991d uses sources-a2026-09-10. Manager reverse proof restores the complete original values. Existing shared source grammar projection stays Pending and grants no compiler admission, handler, namespace/security/audit/UOW/recovery or selected route. Actual current-instance/projection and mandatory gates must pass before the bounded source slice seal; application and all runtime/licensed/parent acceptance remains pending.

Monitoring source integration passes28 generator regressions,eight IR regressions
and the actual Draft202012 monitoring/control instance gate. Private source
projection now has49 command rows,1126 operands and1402 case candidates.
All runtime/application/selected-route/recovery/licensed gates remain pending.

### Serialized CSD source-contract integration

SPI-1001.spi-csd-source-contracts integrates the two declared CSD families after
complete producer repair and independent review. Baseline SSJL4D_6.x, official
SPI catalog rows retained in each command, and hash-verified dfha8_csd_* bodies
bound the definition and browse contracts; dfha80x and dfhp4_argumentvalues
provide fullword CVDA and language argument context. All 25 rows, 443 union
operands, 250 response clauses and 475 case identities remain source-only.

The reviewed corrections isolate INSTALL list/resource selectors, LOCK/UNLOCK
and INQUIREGROUP namespaces, per-command lock ownership and protected targets,
COPY duplicates, exact syntax citations and fullword CVDA width. Undefined
character validation and CVDA numeric domains remain explicit pending gaps.
The nine appended CVDA gap notes and fourteen width changes are independently
counted; the producer report's earlier eleven-note prose is corrected only in
external provenance-preserving copies. Existing staged worker drafts were not
used. All 352 property changes reverse to the complete frozen originals; 49
cases change and 426 cases are identical. Source-reference credit is zero.

Administrative compiler admission and all runtime, authorization, recovery,
selected-route and licensed acceptance gates remain pending. No CSD handler,
resource mutation, alternate grammar or route is enabled by this integration.

CSD source integration passes 28 generator regressions, eight IR regressions
and both actual Draft202012 family instance gates. Private source projection
now has 74 command rows, 1569 operands and 1877 case candidates. All runtime,
application, selected-route, recovery and licensed gates remain pending.

### Serialized FEPI session and data source-contract integration

SPI-1001.fepi-session-source-contracts integrates the declared session/data
family after complete repair and independent changed-fact review. Baseline
SSJL4D_6.x FEPI command bodies and context candidates, the official FEPI catalog
rows retained per command, dfhp748/749/74b/734/74f/74g/74i/73d, and
dfhp74l/74m/7k4/7kq bound the reviewed forms, data formats and state obligations.
The argument/CVDA common contexts remain hash-verified reference authority.

All 19 rows, 222 union operands, 380 response clauses and 779 case identities
remain private source contracts. Eight finding groups are repaired through 82
property actions, accounting for 678 recursive property changes. Reversing the
complete action set restores the frozen original; 41 cases change and 738 cases
are identical. Five INQUIRE CONNECTION forms distinguish named inputs, START,
NEXTNODE/NEXTTARGET receivers and END in the existing common grammar vocabulary.

Temporary POOL selection is separate from allocated CONVID ownership; RECEIVE
uses inbound presentation rules. Requested outputs have explicit caller storage.
Exact one-byte SEND data is concrete, while 4096/4097-byte fixture representation
and null-AID/ESCAPE/final-attention error precedence remain precise pending
obligations. All 21 syntax negatives retain their identities: 18 isolate their
structural constraint and three explicitly retain coupled constraints. No
undefined byte domain, numeric alias, omitted output or remote completion is
asserted as executed. Source, runtime, security, recovery, selected-route and
licensed acceptance remain pending; no public handler or route is enabled.

Independent full reviews of the terminal and network candidate artifacts were
declared externally before dispatch into distinct exact-base worktrees. The
retained terminal author now reviews network and the retained network author
now reviews terminal. Each review owns only external reports and accounts for
all selected source bodies and cases; neither candidate is yet integrated.

The first manager generator attempt correctly rejected the five FEPI INQUIRE
forms' unsorted IDs. Its failed receipt is preserved and target cleanup passed.
Manager repair orders the array by existing form ID, as the common generator
and xtask validator require. Every complete form object is byte-fact-identical
and reversing that sole order change restores the reviewed family bytes.
No selector, operand, direction, constraint, source citation or case changes.

FEPI session source integration passes 28 generator regressions, eight IR
regressions and the actual Draft202012 family instance gate. Private source
projection now has 93 command rows, 1791 operands and 2656 case candidates.
All runtime, application, selected-route, recovery and licensed gates remain
pending.

### Serialized region lifecycle source-contract integration

SPI-1001.spi-region-source-contracts integrates the declared region lifecycle
family after complete thirteen-finding repair and independent review. Baseline
SSJL4D_6.x SPI command bodies, original catalog identities and exact source pins
remain attached to every row. The reviewed dfha8_createbundle,
dfha8_inquireassociation/task/uowdsnfail/uowenq and
dfha8_setbundle/dispatcher/system/task topics, dfha817 diagnostics and common
argument/CVDA contexts bound the changes. No source was repinned or refreshed.

All 25 command rows, 509 union operands, 156 response clauses and 868 case
identities remain source contracts with private unregistered runtime binding.
Forty-one property actions account for all 165 atomic changes and reverse to
the complete frozen original. Seventeen cases change and 851 are identical;
sixteen whole commands are unchanged. TYPE and REASON finite symbolic domains
exclude table headings and retain the source-qualified non-RLS DEADLOCK value.

START UOWENQ fixtures no longer request NEXT receivers, SCANDELAY range failures
are isolated from TIME comparison, and named INQUIRE TASK requires TASK.
PHTASKID is explicitly requested packed-decimal storage; association layouts,
origins, previous hops, parent identifiers and output defaults remain distinct.
LOGDEFER halfword encoding, END-before-START authority, CREATE BUNDLE failure
phase/500-versus-612, PURGE-only KILL outcome, CLIENTLOC socket bit meanings and
actual injected bundle enable/availability failures remain precise pending
obligations. No numeric alias, completion, rollback or execution credit is
assigned by these source corrections. Admission, runtime mutation, selected
route, authorization, recovery and licensed acceptance remain pending.

Web repair is now independently reviewed in a distinct exact-base worktree.
The full queue review's twelve P2 semantic/fixture findings and one P3 shared
contract citation finding are assigned to a single retained CLI repair author.
Both tasks were declared externally before dispatch, own exact frozen inputs
and preserve all previously declared identity and pending-gate obligations.

Region source integration passes 28 generator regressions, eight IR regressions
and the actual Draft202012 family instance gate. Private source projection now
has 118 command rows, 2300 operands and 3524 case candidates. All runtime,
application, selected-route, recovery and licensed gates remain pending.

### Pre-dispatch independent review enrollment on the 52-commit candidate

The manager declares these four bounded reviews before launching their CLI
processes. All producers have verified terminal exit zero; each reviewer uses a
different retained thread and a new isolated worktree at `12fca1ca26155df445b9ae7e5ef8d24e3790931b`.
Repository write ownership is empty. Each owns only its named external report
root under the retained v010-20261002 worker-run directory.

- `SPI-1001.spi-queue-storage-repair-review`: 30 exact rows, all 903 cases; changed source/fact review. Frozen SHA-256 `05496d90cdca6b16cf52e5fdeacf85961a57a63f4adee65e15b0f65e1e32dc7c`. IDs: `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0011`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0014`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0017`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0029`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0033`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0071`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0074`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0075`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0086`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0090`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0107`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0117`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0118`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0132`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0133`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0134`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0162`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0170`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0171`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0179`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0180`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0181`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0182`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0219`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0228`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0229`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0250`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0251`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0259`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0260`.
- `SPI-1001.spi-event-policy-contract-review`: 19 exact rows, all 705 cases; full source/fact review. Frozen SHA-256 `21383295d4773ced14a9a8626b2df03d1effc9b8f27530b57856a20442bfb131`. IDs: `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0103`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0104`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0105`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0106`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0119`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0120`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0121`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0122`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0123`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0126`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0151`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0152`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0158`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0200`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0220`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0221`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0222`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0223`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0242`.
- `SPI-1001.spi-database-messaging-contract-review`: 21 exact rows, all 550 cases; full source/fact review. Frozen SHA-256 `13e14bc09070ba4424a5e942b898b2e08168f7e6b9ec041c05e6f368bd6b8849`. IDs: `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0006`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0007`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0008`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0019`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0020`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0067`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0068`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0069`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0078`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0079`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0109`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0110`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0111`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0140`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0141`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0142`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0211`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0212`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0213`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0235`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0236`.
- `SPI-1001.spi-platform-programs-contract-review`: 19 exact rows, all 562 cases; full source/fact review. Frozen SHA-256 `205459653198416048ba4175150eac58a5b9825b58ab5f59327194c449a3ab55`. IDs: `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0015`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0016`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0062`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0076`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0077`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0093`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0094`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0125`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0135`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0136`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0137`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0143`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0145`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0146`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0147`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0197`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0230`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0231`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0232`.

Dependencies: frozen producer bytes and current source/schema authorities.
QUEUE reuses the complete original review only after exact byte/pin/receipt
verification, then independently reviews all 59 actions, 80 properties and 17
changed cases and proves all 886 other cases identical. The other three reviews
cover every command, option, condition, form, CVDA member, obligation and case;
no sampling. Search/read of relevant matching pinned sources is mandatory;
unknown linked contexts and disputed semantics remain precise pending gaps.

The manager alone owns schema, generator, registry/facade, IR, state/security
authorities, status, actual family-instance and mandatory integration gates,
sealing and serial reconciliation. Review gates require complete accounting,
source-linked findings and compatibility/preservation proof. Review completion
earns zero runtime, selected-route, recovery or licensed credit. No public
admission or route is authorized by these reviews. All 269 SPI and 39 FEPI
identities and applicable mandatory obligations remain pending.

Earlier review follow-ups were recorded externally before dispatch and copied
into this status afterward; that chronology is preserved as a process deviation.
These new declarations precede dispatch in this document.

### Serialized web resource source-contract integration

SPI-1001.spi-web-source-contracts integrates the already declared 25-row WEB
family after complete repair and independent changed-fact review. Baseline
SSJL4D_6.x SPI command bodies, ibm-cics-ts-6x-spi-command-bodies-2026-09-12,
original catalog rows and exact per-command topic hashes remain authoritative.
The reviewed dfha8_createatomservice/createdoctemplate/createpipeline/createurimap/
createwebservice, dfha8_setdoctemplate/seturimap, dfha8_inquireurimap,
dfha8_performpipeline/setwebservice/setxmltransform and dfha817 topics, plus
common API format and CVDA contexts, bound the repaired facts. No refresh.

All 25 rows, 336 union operands, 134 response identities and six named forms
remain private unregistered source contracts. All 95 property changes replay
to the complete final bytes and reverse to the entire original. All 549
original case identities remain: 28 change and 521 are identical. Exactly one
selector-only SET DOCTEMPLATE missing-action candidate is added, giving 550
cases. Independent review found no actionable defect in the bounded repairs.

SET DOCTEMPLATE requires COPY/NEWCOPY in its source grammar. URIMAP redirection
candidates use an enabled, available SERVER definition and virtual host without
a concurrent disable; simultaneous disable precedence remains pending. The
ATOMSERVICE 628 diagnostic isolates the CICS execution principal's authorization
for an existing valid file from issuing-task NOTAUTH and missing/invalid files.
The URIMAP table omits a DESCRIPTION keyword number; no sibling-resource number
is imported. Generic low-halfword diagnostic candidates remain conditional on
complete attributes and failure isolation. All 33 syntax/syncpoint citation
arrays are corrected; five LOG geometry observations retain unresolved omission
and default meaning. Finite numeric encodings, conditional fixture applicability,
unknown linked contexts and full security/UOW/recovery bindings remain pending.

No public handler, compiler admission, resource mutation or selected route is
enabled. Application dependency, all runtime/authorization/recovery/selected-route
and licensed acceptance, and parent SPI-1001 remain pending with zero credit.

Web source integration passes 28 generator regressions, eight IR regressions
and the actual Draft202012 family instance gate. Private source projection now
has 143 command rows, 2636 operands and 4074 case candidates. All runtime,
application, selected-route, recovery and licensed gates remain pending.

### Pre-dispatch network repair and transaction-resource review

- `SPI-1001.spi-network-connections-review-repair`: 24 exact rows, all 271 existing case identities; frozen SHA-256 `fe8650de5657c907949c2789cd13f63db9dcdc89d1330d1567baf211cb7699ab`. IDs: `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0005`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0013`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0028`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0066`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0073`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0085`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0108`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0112`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0124`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0128`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0129`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0130`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0131`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0168`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0169`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0194`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0196`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0210`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0214`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0225`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0226`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0227`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0248`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0249`. Repository ownership: `conformance/0.10/cics/families/spi-network-connections.json`.
- `SPI-1001.spi-transaction-resources-contract-review`: 22 exact rows, all 737 existing case identities; frozen SHA-256 `beba8a363fc8dbec514100d7906700f2987a195fa893216dfd9b4b5378921394`. IDs: `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0010`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0024`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0031`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0032`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0082`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0088`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0089`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0115`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0153`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0156`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0167`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0176`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0178`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0188`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0192`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0217`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0240`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0247`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0256`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0258`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0264`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0268`. Repository ownership: none.

Network repair depends on the complete independent 12-finding report: R001–R012,
six high and six medium, including its hash-bound 126-pointer citation inventory.
All existing rows/options/response identities/case IDs and pending obligations
are preserved; exact source-required clauses, finite domains, eligible fixtures
and source citations are repaired in the sole owned family. Any genuinely new
source-negative case must be adjacent, source-bound and separately counted.
No inferred standalone form, harmless OPEN action, numeric error or alternate
schema is permitted. Reviewer contradictions are resolved against pinned bodies,
not copied as semantic authority. A different terminal retained CLI thread owns
repair; the original source workspace has no live writer.

The transaction-resource reviewer uses a new isolated worktree at the 53-commit
candidate and accounts every option/condition/form/domain/obligation/case/property
against complete pinned command bodies. It owns only external reports.
Both declarations precede CLI dispatch. The manager alone owns all shared
authorities, status, actual Rust family-instance/mandatory integration gates and
serial sealing. Complete source-linked repair/review accounting is required.
All six gates, 269 SPI/39 FEPI acceptance, application dependency, selected-route,
recovery and licensed evidence remain pending and credit zero.

### Pre-dispatch terminal/session source repair

SPI-1001.spi-terminal-sessions-review-repair depends on the terminal full
review's verified terminal report and frozen family SHA-256
`b6abebb5c265b0592079b2d0e555788151d6c24f12c769a8f07bb907247d3e64`. All 27 exact rows, 505 operand names, 200 numeric
conditions plus two unmapped NOTFOUND clauses, 145 domains, ten forms and all
1126 existing case identities are preserved. Assigned scope is R1–R16, exactly
516 reported property/case instances, including 420 CVDA observation fixtures.
Exact row IDs: `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0001`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0018`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0021`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0022`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0025`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0027`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0030`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0034`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0064`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0080`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0083`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0087`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0098`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0099`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0102`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0138`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0144`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0149`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0154`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0172`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0189`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0207`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0209`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0233`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0237`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0252`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0265`.

The retained reviewer thread becomes the single repair author in the terminal
source workspace after verified terminal exit. Sole repository ownership is
`conformance/0.10/cics/families/spi-terminal-sessions.json`; reports stay external.
Every changed source assertion requires pinned search/read. Independent state,
concrete request operands, required versus optional source syntax, distinct
receiving storage, protected-prefix/surrogate targets, interval validation and
pool failure distinctions must be repaired. Source conflicts remain precise
pending; omitted-length/default delivery and licensed behavior are not guessed.
R16's above-line LOG geometry is recorded separately; any default interpretation
requires exact pinned notation authority and cannot be imported from a reviewer.
All changes need an exact ledger and whole-original reverse/equality accounting,
then review by a different thread. Shared schema/generator/IR/state/security,
actual Rust family-instance and mandatory integration gates/status/sealing
remain exclusively manager-owned. All six gates and 269 SPI/39 FEPI obligations
remain pending, zero credit. This declaration precedes CLI dispatch.

### Serialized common numeric CVDA source bindings

SPI-1001.program-cvda-numeric-bindings is manager-owned and dependency-ready
for private Wave A metadata: existing reviewed PROGRAM source domains, shared
CICS operand/constraint types, and retained hash-verified numeric reference
dfha80c (baseline ibm-cics-ts-6x-misc-tail-cvda-2026-09-23, SHA-256
5b95b620971d42a9f57511b362f9a12dc04e9ad4b9c26f42cc8e7be943221381).
Common dfha80x keeps fullword storage and symbolic/DFHVALUE usage distinct from
reference numeric representations. Catalog rows0026/0084/0155/0241 remain four
separate PROGRAM identities. Every old symbolic domain and existing case stays
intact; PL1 has no numeric reference row and remains unresolved, while PLI is
explicitly named. Equal MAP/MAPSET numbers do not establish semantic aliases.

The manager alone owns the optional bounded numeric facet in the existing
family schema, sole generator, actual xtask validator, shared CICS IR types,
PROGRAM source data, generated projections/chunks, fragment/status/docs.
All live workers retain their frozen schema and disjoint owned family files;
additive compatibility and provenance are preserved. No executable mapper,
compiler admission, new dispatcher, route, state owner or response mapping
is introduced. Runtime/application/selected-route/recovery/licensed gates stay
pending with zero credit. Required checks are meaningful numeric source/member/
32-bit-bound/pin negatives, old-preimage compatibility, shared IR scope/alias
separation, actual family instances and mandatory integration/policy gates.

### Pre-dispatch numeric-facet review and bounded ENQ source-conflict repair

SPI-1001.program-cvda-numeric-bindings-review depends on the manager's complete
source-pinned common numeric facet and frozen code/family snapshots. A distinct
retained CLI thread owns only external review reports and a new isolated
worktree at the 53-commit candidate. Review all common IR/facade exports, schema,
sole generator, xtask validator, regressions, generated chunks and all 196
scoped numeric values in 60 PROGRAM parent/form domains, rows0026/0084/0155/0241.
Every original fact/case and legacy product preimage must be equal after removing
only the additive numeric facet. Fullword bounds and pin/member/order negatives,
partial-domain representation and distinct same-number names need review; no
mapper, alias equivalence, admission or execution credit. Shared writes/gates
and final sealing remain manager-owned.

SPI-1001.spi-queue-enq-conflict-repair depends on the verified-terminal QSRR-01
report. Sole repository ownership remains spi-queue-storage.json and only
/commands/11/gaps/15 may change. Preserve both hash-pinned ENQ and UOWENQ
diagrams' opposed filter-omission requiredness, their settled alternative
branches and RESOURCE/RESLEN input pairing, and independent NEXT output roles.
No union rejection, complete browse form or omission precedence is invented.
All 30 rows and 903 cases remain exact; all other properties/bytes must reverse
to the frozen repaired candidate. A different thread must re-review this one
paragraph before actual instance/generator/integration gates.

Both declarations precede dispatch; all shared authorities are manager-owned,
all six gates, application/selected-route/recovery/license and 269 SPI/39 FEPI
acceptance remain pending with zero credit.

### Pre-dispatch database, event and platform source repairs

- `SPI-1001.spi-database-messaging-review-repair`: exact 21 rows and 550 original case identities, findings DBMQ-R01–R08, frozen SHA-256 `13e14bc09070ba4424a5e942b898b2e08168f7e6b9ec041c05e6f368bd6b8849`. Sole repository ownership `conformance/0.10/cics/families/spi-database-messaging.json`. Exact rows: `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0006`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0007`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0008`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0019`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0020`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0067`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0068`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0069`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0078`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0079`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0109`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0110`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0111`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0140`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0141`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0142`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0211`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0212`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0213`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0235`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0236`.
- `SPI-1001.spi-event-policy-review-repair`: exact 19 rows and 705 original case identities, findings EVT-R1–R3, frozen SHA-256 `21383295d4773ced14a9a8626b2df03d1effc9b8f27530b57856a20442bfb131`. Sole repository ownership `conformance/0.10/cics/families/spi-event-policy.json`. Exact rows: `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0103`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0104`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0105`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0106`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0119`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0120`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0121`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0122`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0123`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0126`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0151`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0152`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0158`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0200`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0220`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0221`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0222`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0223`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0242`.
- `SPI-1001.spi-platform-programs-review-repair`: exact 19 rows and 562 original case identities, findings PP-01–PP-17, frozen SHA-256 `205459653198416048ba4175150eac58a5b9825b58ab5f59327194c449a3ab55`. Sole repository ownership `conformance/0.10/cics/families/spi-platform-programs.json`. Exact rows: `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0015`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0016`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0062`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0076`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0077`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0093`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0094`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0125`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0135`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0136`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0137`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0143`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0145`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0146`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0147`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0197`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0230`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0231`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0232`.

All three lanes depend on verified terminal full independent reviews. Retained
reviewer threads become single repair authors in the already terminal source
workspaces; neither writer is the original author of its assigned family. Each
finding, full affected pointer/case inventory and source implication requires
exact offline pinned search/read. Existing identities, source pins, response
identities, old case IDs/order and all pending obligations remain intact.

DATABASE/MQ fixes singleton output choices, valued/bare authorization exclusions,
value-dependent stop prerequisites, all 36 selected-receiver fixtures/47 receivers,
CREATE attribute phases and syncpoint fixtures, 100 bare-selector citations and
the external search-provenance mismatch. Above-line graphical default meaning
needs pinned notation authority; layout/default/source conflicts stay pending.
EVENT fixes five source-settled START dependencies, all 38 condition receiver
fixtures, and the FEATUREKEY diagram/example VALUE conflict. PLATFORM fixes
17 findings covering missing operands/required actions, incompatible global/TRUE
and JVM contexts, selected receiver storage and SET LIBRARY mutation isolation.
No new browse form, guessed ABI/numeric alias or response, generic success or
licensed interpretation is introduced. Preserve contradictory pinned facts.

Each final artifact needs an exact property ledger, whole-original forward and
reverse proof, unchanged case accounting and a different thread's changed-fact
review. Only external receipts/reports may accompany the owned source file.
Shared schema, generator, registries, IR/state/security/UOW, status and all actual
Rust family-instance/mandatory integration gates/sealing remain manager-owned.
All six gates, application dependency, selected-route/recovery/license and
269 SPI/39 FEPI acceptance remain pending, credit zero. This declaration is
written before CLI dispatch; no parent is sealed from partial children.

### Pre-dispatch final ENQ source-conflict review

SPI-1001.spi-queue-enq-conflict-review depends on verified terminal one-property
QSRR-01 repair, final SHA-256
`a7a4a7a270cf4a768d46fba275c253cf06598445b809157396ce0a450e5e4ecc`.
A fresh independent CLI thread in its isolated review worktree owns external
reports only. Scope is /commands/11/gaps/15 against both pinned ENQ/UOWENQ
authorities and every preservation/provenance implication of that change.
All other prior repaired facts and complete cases are reused only after exact
whole-byte/source/report equality; all 30 rows and 903 case identities remain
intact. No original repair writer runs in the review checkout. Shared authorities,
actual family-instance/mandatory gates and serial sealing remain manager-owned.
All runtime/parent/dependency/recovery/license gates remain pending, credit0.
This declaration precedes dispatch.

### Pre-dispatch terminal/session changed-fact review

SPI-1001.spi-terminal-sessions-repair-review depends on terminal repaired
SHA-256 `75289e71d56d5501b3ea141f0a649463394df0a9763869a52f20c2e3cd81577d`.
A distinct retained thread owns external review only in a new isolated checkout.
Scope is all 523 repair actions, 516 assigned-instance dispositions, 473 amended
original cases and seven additional ATTRLEN-omission candidates across the same
27 predeclared rows. All 653 untouched complete cases and original unrelated
facts may be reused only with exact byte/hash/whole-original reverse proofs.
420 CVDA observations require independently seeded state or explicit unresolved
DEVICE/NOTAPPLIC mappings; symbolic membership is no output oracle. Both BMS
source outcomes remain unresolved, four CHANGEAGREL digits do not establish
four bytes, and pool failure retention or omitted LOG defaults are not invented.
Every adjacent edit and preserved source conflict is included in review.
Ownership is no repository writes; shared authorities, actual family-instance,
generator/mandatory gates and serial sealing remain manager-owned. All runtime,
parent, application/selected-route/recovery/license gates remain pending, zero
credit. This declaration precedes retained CLI dispatch.

### Pre-dispatch network changed-fact review

SPI-1001.spi-network-connections-repair-review depends on terminal repaired
SHA-256 `ab61a5de1ba08dc79baddddbfa9d15cb5e944e4c77566a21382816a97e5e5ee7`.
A fresh independent CLI thread owns external reports only in an isolated review
checkout. Scope covers all 244 property actions, four new closed source forms,
38 semantically changed cases and 113 citation-only changes, including all 126
assigned citation pointers, across the same 24 predeclared rows. All 120 untouched
whole cases, IDs/order, 332 operand names/directions/widths, 188 response identities
and original unrelated facts require exact equality and whole-original reversal.
Review source-required action choices, independent status/certificate/security
fixtures, phase-local CREATE operands and preserved source conflicts without
inventing combined-action precedence, numeric mapping or harmless status changes.
Shared authorities, actual Rust instance/generator/mandatory gates and serial
sealing stay manager-owned. All six/runtime/parent/application/selected-route/
recovery/license gates remain pending, credit0. Declaration precedes dispatch.

Numeric CVDA source integration passes 31 generator regressions, nine shared IR
regressions and 17 actual family-validator regressions. Independent changed-code
and source review found no actionable defect: all 196 records in 60 PROGRAM
parent/named domains, 106 original PROGRAM cases and 143 generated declarations
are accounted, and removing only the additive facet restores every original
source fact and the old product digest. PL1 numeric binding remains unavailable;
PLI and equal-number MAP/MAPSET names remain distinct. No runtime encoder,
compiler admission or execution behavior changes.

All 11 currently integrated family instances pass the same actual Draft202012
validator with --family, plus module/public-API/schema/format/architecture/docs/
changelog/dependency gates. The global 18-family gate remains pending because
seven enrolled source files are not yet integrated; its failed receipt is kept
and no acceptance criterion is relaxed. Current private source projection still
has 143 command rows (104 SPI, 39 FEPI), 2636 operands and 4074 case candidates.
Runtime acceptance and official credit stay zero. Application, selected-route,
recovery/licensed and parent SPI-1001 remain pending.

### Pre-dispatch transaction-resource source repair

SPI-1001.spi-transaction-resources-review-repair depends on verified terminal
full independent review TXR-R01–R15 and frozen family SHA-256
`beba8a363fc8dbec514100d7906700f2987a195fa893216dfd9b4b5378921394`.
Sole repository ownership is conformance/0.10/cics/families/spi-transaction-resources.json;
the retained original author thread has completed its disjoint NETWORK repair
and is the only writer in this terminal source worktree. All 22 predeclared exact
rows, 311 option identities, 148 numeric response identities, 64 domain sets/178
symbols, five forms and all 737 original case IDs/order remain intact.

Repair all reported receiver directions, optional MAXIMUM and expiry AT grammar,
class-choice exclusions, precise retained-lock/timestamp/default predicates,
28 contradictory authorization seeds, 21 negative-state seeds, actual class
sender and prior mutation values. Registered FORMATTIME supplies eight-byte
ABSTIME extents for ten receivers and copied forms; pointer/encoding/storage
binding remains pending. Preserve raw REQID grouping/body opposition and all
other pinned conflicts or unknown contexts. No invented browse recipe, numeric
precedence, ABI alias, new DSL or runtime authority. Any source-backed paired
class or unchanged-health additional case is separately counted. Every repair
requires pinned search/read, complete action ledger/whole-original reversal and
a different thread's changed-fact review before actual manager-owned family
instance/generator/mandatory gates and serial sealing. Shared schema/IR/state/
security/UOW/status/generators remain manager-owned. All six and 269 SPI/39 FEPI
parent/dependency/selected-route/recovery/licensed acceptance remain pending,
credit0. This declaration precedes dispatch.

Exact repair rows: `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0010`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0024`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0031`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0032`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0082`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0088`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0089`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0115`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0153`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0156`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0167`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0176`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0178`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0188`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0192`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0217`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0240`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0247`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0256`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0258`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0264`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0268`.

### Pre-dispatch WEB symbolic CVDA domains

SPI-1001.spi-web-cvda-domains depends on the sealed WEB source repair and common
finite-domain/numeric facet vocabulary at the 54-commit candidate. Demonstrated
inventory gap: the 25-row WEB family has zero finite CVDA domain arrays despite
source-described CVDA value-shaped operands. One retained terminal thread owns
only conformance/0.10/cics/families/spi-web-resources.json in a new isolated
checkout. Exact frozen source SHA-256
`40bc41026bbf213a26957f785a979cea2af25bb9cdbdec12647e0758868d39e9`.

Derive all source-settled symbolic membership for parent and existing named form
scopes, sender/output separately, with four-byte values only where pinned common
CVDA authority applies. Every existing 25 row identity, 336 option identity/role,
134 response identity, six form identity and 550 complete case objects/IDs/order
remain unchanged. Only additive cvda_domains, directly supporting option source
citations and precise unresolved-domain gaps may change. No new form, option,
case, alias normalizer, numeric encoding, runtime route or shared schema edit.
Conflicting or unstated finite membership stays an exact pending source gap.
All additions and any citation/gap edits need whole-original reverse proof and
different-thread independent source review before manager-owned actual instance/
generator/focused/mandatory integration gates and serial sealing. Shared schema,
generators, registries, IR/state/security/UOW/status remain manager-owned. All
runtime/parent/application/selected-route/recovery/license acceptance remains
pending, credit0. This declaration precedes dispatch.

Exact WEB rows: `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0003`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0009`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0023`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0035`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0036`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0063`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0070`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0081`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0091`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0092`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0097`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0114`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0150`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0187`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0190`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0191`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0193`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0198`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0206`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0216`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0239`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0263`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0266`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0267`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0269`.

### Serialized queue/storage source-contract integration

SPI-1001.spi-queue-source-contracts integrates the already predeclared 30-row
QUEUE/storage family after its 13 source repairs and separately reviewed QSRR-01
source-conflict repair. All 59 original repair actions plus one gap replacement
reconstruct the whole final bytes and reverse to the entire original. All 903
original case IDs/order remain: 17 amended, 886 identical; the final gap changes
no case. 337 option identities, 204 condition identities and 24 form identities
remain intact. SYSOUTCLASS's one-character source establishes its one-byte
ceiling in parent/form scopes. Independent review has no remaining actionable
finding; all runtime/source closure gaps remain pending.

Pinned SSJL4D_6.x SPI bodies (ibm-cics-ts-6x-spi-command-bodies-2026-09-12)
include createtdqueue/createtsmodel/discardtsmodel/inquiretsmodel/inquiretsqueue,
inquirejournalmodel/inquirejournalname/inquirestreamname/inquiretdqueue,
inquirecfdtpool and inquireenq/inquireuowenq. Common API-format receiver authority
keeps response writes separate from source-preserved data areas. Exact catalog
rows, per-topic pins and all 13 repair clause matrices remain unchanged. Fresh
manager offline search/read covers 13 relevant topics/40 commands, all exit0.
No refresh or execution credit.

ENQ retains the opposed no-empty-filter-bypass diagram against UOWENQ's empty
bypass/unfiltered START prose under both exact pins. Filter omission applicability,
complete browse syntax/storage and precedence stay pending; NEXT output slots
do not become START filters. Surrogate denial requests an actual USERID, TSMODEL
inputs remain separate from TSQUEUE historical outputs, pool/certificate/stream
observations use independent state, and platform lock-order ownership cites
TRANSACTION-PARTICIPANT-V1. Eight obsolete JOURNALNUM references remain explicit
unresolved source candidates. No runtime resource owner, route or generic
handler is introduced. Application/selected-route/recovery/licensed, all six
command gates, 269 SPI/39 FEPI acceptance and parent SPI-1001 remain pending,
credit0. Manager alone owns sole generation, actual instance/mandatory gates and
serial child sealing.

Queue source integration passes 31 generator and nine shared IR regressions and
the actual Draft202012 family-instance gate. Current private projection has
173 command rows (134 SPI, 39 FEPI), 2973 operands and 4977 case candidates.
Six of the 18 enrolled family files remain outside integration; the full wave
gate stays pending. No source case adds runtime/official acceptance credit.

### Pre-dispatch database and event changed-fact reviews

- `SPI-1001.spi-database-messaging-repair-review`: verified-terminal SHA-256 `bc66dddf5afca257819da8ce70372f394e142b79a76f4343d575c54d8c9cfcde`, all 164 property operations, 37 amended original cases/513 untouched whole cases, 550 original identities across the same 21 exact rows. Reviewer owns external reports only in an isolated 55-commit checkout. Exact rows: `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0006`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0007`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0008`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0019`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0020`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0067`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0068`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0069`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0078`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0079`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0109`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0110`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0111`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0140`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0141`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0142`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0211`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0212`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0213`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0235`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0236`.
- `SPI-1001.spi-event-policy-repair-review`: verified-terminal SHA-256 `7d630da49d4564008dac31e992cfbde6cb45471b70304bfb50bb14366e8d4e44`, all 424 property operations, 38 amended original cases/667 untouched whole cases, 705 original identities across the same 19 exact rows. Reviewer owns external reports only in an isolated 55-commit checkout. Exact rows: `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0103`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0104`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0105`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0106`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0119`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0120`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0121`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0122`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0123`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0126`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0151`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0152`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0158`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0200`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0220`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0221`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0222`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0223`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0242`.

Reviewers differ from both original and repair authors. Scope is every actual
changed property/case/source implication and all complete assigned findings
(8 DATABASE/MQ, 3 EVENT); exact untouched material is reused only after hash/byte
equality and whole-original reversal. Database required output alternatives,
valued/bare exclusions, 47 selected receivers, CREATE phases/syncpoint and 100
citations need independent facts; unknown layout/default/early-failure/source
conflicts stay pending. Event five START dependencies, all 38 separately
initialized condition receivers and coherent independent authorization denials,
plus FEATUREKEY VALUE conflict, require source review without generic success.
Shared authorities/status, actual Rust family-instance/sole-generator/mandatory
integration gates and serial sealing stay manager-owned. All six gates and
269 SPI/39 FEPI parent/application/selected-route/recovery/license acceptance
remain pending, credit0. These declarations precede retained CLI dispatch.

### Pre-dispatch platform/JVM changed-fact review

SPI-1001.spi-platform-programs-repair-review depends on verified terminal
PP-01–PP-17 repair SHA-256
`f42ba9273c5178ac3bcfee56916325bf1689085086825f1ac9c9345dcee42151`.
A different retained thread owns external reports only in an isolated review
checkout. Scope is all 221 property changes, 17 finding dispositions/99 assigned
case records, 62 amended complete cases and all source implications across the
same 19 exact rows. 500 untouched complete cases (including 37 ENABLE requests),
all 562 IDs/order, 330 option identities, 150 response identities, 19 forms and
75 scoped domains/221 members require exact preservation and whole-original
reversal. Review source-required receiver/action choices, coherent global/TRUE/
JVM contexts, explicit triggering operands, distinct receiver storage and SET
LIBRARY mutation isolation. Source conflicts, missing scalar/pointer/layout/
ABI and all 87 output fixture gaps remain pending without fabricated results.
Shared schema/generators/IR/state/security/UOW/status and actual Rust instance/
mandatory integration gates/sealing stay manager-owned. All six and parent/
application/selected-route/recovery/licensed acceptance remain pending, credit0.
Declaration precedes retained CLI dispatch. Exact rows: `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0015`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0016`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0062`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0076`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0077`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0093`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0094`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0125`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0135`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0136`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0137`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0143`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0145`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0146`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0147`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0197`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0230`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0231`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0232`.

### Serialized REGION numeric CVDA source bindings

SPI-1001.region-cvda-numeric-bindings is manager-owned and depends on the sealed
common numeric facet and REGION source contracts. Demonstrated gap: two existing
output domains lack reference numeric metadata. Scope is all 15 REASON members
on INQUIRE UOWDSNFAIL row0184 and all six TYPE members on INQUIRE UOWENQ row0185.
Only two additive numeric_encoding facets in spi-region-lifecycle.json, sole
generated projection/chunks, fragment/status/docs may change. All 25 REGION rows,
509 option identities, 156 response identities and 868 whole case objects remain
exact; existing symbolic membership/source/conditions/lifecycle/security/UOW
facts and every other family are preserved. No schema/shared-type/mapper/
route/runtime change or alias inference is introduced.

Fresh pinned search/read covers both exact primary bodies, dfha80x receiver
fullword/direction/version guidance and dfha80c global numeric reference: 25
commands, all exit0, hash-verified retained bytes, no refresh. Numeric reference
is SSJL4D_6.x/reference-applications/commands-api/dfha80c.html, baseline
ibm-cics-ts-6x-misc-tail-cvda-2026-09-23, SHA-256
5b95b620971d42a9f57511b362f9a12dc04e9ad4b9c26f42cc8e7be943221381.
Primary baseline ibm-cics-ts-6x-spi-command-bodies-2026-09-12: UOWDSNFAIL
e94b8a83be512a54b42b0fe7dd00c4ef65e6f526d178520f1ee93b853946a5f4
parser111–194 and UOWENQ
3a7368c2114aff7f355bdfd74fc092dffa898019e4759e270254a1cb40c106d1
parser194–235 preserve command-specific qualifiers. Publication reference
numbers do not establish executable ABI, version applicability or state oracles.

A different retained thread must review all 21 numeric records, both scopes,
whole-original source/case equality and sole-emitter/preimage proofs. Focused
existing generator/shared IR regressions, actual REGION family instance and
mandatory integration gates precede serial child sealing. Global 18-family
gate, all six command gates, application/selected-route/recovery/licensed and
269 SPI/39 FEPI parent acceptance remain pending, credit0. Shared ownership
is serialized under the manager.

Manager scope wording correction for the active WEB domain lane: the existing
closed symbolic domain fields are option, values and source_lines. Storage
width belongs to the existing operand's source_max_value_bytes, which must be
four; there is no domain.bytes field. Earlier prompt shorthand incorrectly
named bytes as a domain field. The actual frozen schema is authoritative, and
read-only inspection confirms the live author uses its correct source_lines
shape. No worker file was edited or shared schema relaxed.

### Pre-dispatch REGION/WEB review and narrow source repairs

REGION numeric prerequisite gates pass 31 generator/nine IR regressions, actual
family instance and mandatory policy/format/schema/docs/module/public API/
architecture/changelog checks. Two additive domains have 21 source numeric
records; global projection changes from 60/196 to 62/217 numeric domains/records.
All 868 REGION case objects and other source properties remain exact. Three
external inventory-helper shape errors were repaired without product changes;
all attempts cleaned target and successful emission was not repeated.

Each lane below depends on terminal producer/reviewer handoffs and hash-frozen
source inputs; declaration precedes CLI dispatch. Source-only ownership is
isolated. Manager exclusively owns schema/generators/shared IR/security/UOW/
status, actual family/projection/focused/mandatory integration gates and child
sealing. Different-thread independent review must pass before integration.
No all-family, application, parent, runtime, route, recovery or licensed gate
is passed; all six command gates remain Pending, credit0.

SPI-1001.region-cvda-numeric-bindings-review; frozen SHA-256 ced75fad8c4cdab31c080118786489f577784f03a6a1724bb19ecc8bcb256067.
Ownership: external reports only; repository read-only.
Independent review of the manager-owned two additive numeric_encoding facets on INQUIRE UOWDSNFAIL row0184 REASON (15 members) and INQUIRE UOWENQ row0185 TYPE (six members). Read all 21 actual symbol/number/citation records and primary qualifiers, source-preservation and global projection-preimage proof under manager-region-numeric-bindings and validation/region-numeric-bindings-prereview-verified. Independently verify each literal against dfha80c pinned raw/parser reference, exact command-domain spelling/membership and common dfha80x fullword guidance. No rename/alias or wire ABI inference. Existing PROGRAM60domains196numeric records, all 173 projected facts except these two facets, all 25 REGION rows/509options/156responses/868 complete cases must remain exact. Review frozen changed generated Rust facade/chunks against sole-emitter behavior, immutable domain shape and digest preimage. No alternate runtime encoder. External helpers failed on optional forms/domain lists/form wrapper, corrected without product changes; inspect actual current form.grammar shape. No source family amendment is allowed. Both complete publication primary bodies preserve CAUSE/REASON applicability, resource-kind/UOW locking facts; do not transfer inquiry metadata to a mutation or execution oracle.
Exact rows: `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0004`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0065`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0096`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0100`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0101`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0113`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0157`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0160`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0161`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0163`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0165`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0166`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0183`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0184`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0185`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0186`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0199`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0202`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0205`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0208`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0215`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0245`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0246`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0261`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0262`.

SPI-1001.spi-web-cvda-domains-review; frozen SHA-256 e8406e1d669f6d5467489ff9540f11355f3bdef62afa4a69f1dee06f2a3f7894.
Ownership: external reports only; repository read-only.
Fresh independent reviewer, not either prior WEB author or current domain writer. Review complete terminal spi-web-cvda-domains handoff, all42actual property operations, all99scoped domains/329members (56parent,43in six existing forms), all18 appended precise gaps. Domain shape is option/values/source_lines; operand.source_max_value_bytes retains four, no domain.bytes or numeric facets. Verify every membership/conditional clause with independent pinned primary expectations and bounded source reads; form domains must match their precise named scope, no browse guessing. INQUIRE transitional receiver states differ from SET accepted inputs; XMLTRANSFORM agents distinct; SET DOCTEMPLATE COPY singleton NEWCOPY; CREATE LOG geometry/default pending; DOCTEMPLATE TYPE data-area/CVDA source opposition; URIMAP ATTLS6.3 and VALIDATEHOSTbeta and ANALYZERforcedNO qualifiers remain gaps. Preserve all25rows336option names/shapes/widths/directions/citations134conditions six form identities and all550complete cases/IDs/order, all old gaps as exact prefixes, all source/runtime/security/lifecycle/UOW/recovery facts. Whole original40bc41026bbf213a26957f785a979cea2af25bb9cdbdec12647e0758868d39e9 after removing only42declared additions must equal bytes. Use source-derived expectations rather than existing case outputs, no pending waiver hides an unsupported symbol.
Exact rows: `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0003`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0009`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0023`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0035`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0036`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0063`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0070`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0081`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0091`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0092`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0097`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0114`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0150`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0187`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0190`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0191`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0193`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0198`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0206`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0216`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0239`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0263`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0266`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0267`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0269`.

SPI-1001.spi-network-connections-final-narrow-repair; frozen SHA-256 ab61a5de1ba08dc79baddddbfa9d15cb5e944e4c77566a21382816a97e5e5ee7.
Ownership: conformance/0.10/cics/families/spi-network-connections.json.
Continue same retained thread, prior TERMINAL repair56571 terminal0. Own ONLY existing spi-network-connections.json; current prior repair author is busy in a different TRANSACTION worktree and must not be messaged or displaced. Complete exactly NCRR-01..03 from terminal spi-network-connections-repair-review findings.json/full handoff. NCRR01: replace each of ten SET TCPIP expected single-character truncations with complete per-case source qualification, preserve actual OPEN/CLOSED coupled limit action and independent receiver, no harmless status selector/unchanged-on-error/caller rollback/automatic retry. The external old helper double-indexed an already selected string; do not repeat this. NCRR02: repair three exact source_lines arrays, include decisive1..65535/superuser limits and MAXDATALEN3..524288, <=16sortedunique, align all retained claims. NCRR03: missingTCPIPSERVICE fixture must remove impossible priorBACKLOG64/MAXDATALEN64KB actual mutation claims while preserving absent name, authorization, allocated128/32 sender areas and sourceNOTFND13/3 with unresolved ordering. Exactly14existing properties allowed: tenexpected, threecitations, onefixture. Preserve all other whole source bytes, all24rows332options188responses271case IDs/order, forms/domains/oldgap facts. Fresh search/read only two exact primary bodies SET TCPIP and SET TCPIPSERVICE; required diagram already verified source hash, reuse only actual bytes. Complete all repair dispositions, whole forward/backward proof, old/newfullcase objects, unchanged other cases and external standalone delta/full.diff. No unrelated new cases/forms/options/gaps or runtime implementation.
Exact rows: `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0005`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0013`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0028`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0066`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0073`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0085`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0108`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0112`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0124`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0128`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0129`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0130`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0131`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0168`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0169`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0194`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0196`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0210`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0214`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0225`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0226`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0227`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0248`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0249`.

SPI-1001.spi-terminal-sessions-final-narrow-repair; frozen SHA-256 75289e71d56d5501b3ea141f0a649463394df0a9763869a52f20c2e3cd81577d.
Ownership: conformance/0.10/cics/families/spi-terminal-sessions.json.
Continue same retained thread, prior TRANSACTION full review8775 terminal0. Own ONLY existing spi-terminal-sessions.json. Read terminal spi-terminal-sessions-repair-review handoff.md/json and full findings.json. Repair exactly TSRR01 and02 five existing properties: ACQUIRE TERMINAL lifecycle-2 expected remove unsupported USERDATALEN minimum0; preserve halfword/max255 and precise negative/minimum pending. Two NETNAME ACCESSMETHOD BGAM/BSAM cases: each preconditions and expected must mark exact resource-kind-to-NETNAME lookup binding and nominal observation unresolved; inherited TERMINAL enum does not prove lookup applicability. Preserve IDs/receivers/domain symbols/order, do not conclude globally impossible or choose TERMIDERR or substitute actual TERMINAL execution. Fresh pinned search/read only ACQUIRE TERMINAL, INQUIRE NETNAME and supporting INQUIRE TERMINAL paragraphs; preserve all27rows505options200numericresponse plus2unmappedNOTFOUND,145domains10forms all1133wholecases/IDs/order except these3cases. Exactotherproperties byte/ledger forward/reverse proof required; no newcases/options/forms/gaps/source repin, no ABI/output equality without source. Original prior reviewer dispositions remain provenance and all precise limitations pending.
Exact rows: `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0001`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0018`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0021`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0022`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0025`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0027`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0030`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0034`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0064`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0080`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0083`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0087`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0098`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0099`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0102`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0138`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0144`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0149`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0154`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0172`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0189`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0207`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0209`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0233`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0237`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0252`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0265`.

### Pre-dispatch TRANSACTION changed-fact review

SPI-1001.spi-transaction-resources-repair-review depends on terminal TXR-R01–R15
source repair, SHA-256 f4714d96ef451a952accdd31045e9e7f8dbe56bb13b428018d5d49050410cb21.
A different retained thread owns external reports only in an isolated review
checkout; no shared/source/index writes, build/test/generator or source refresh.
Independent changed-fact review of all 275 property actions and 223 changed-fact groups, TXR-R01–R15 dispositions, 117 repaired complete cases and 12 separately counted adjacent cases. Original 737 IDs/order and 620 exact complete cases, 22 rows/311 operand identities/148 response records/64 domains with178 members/five forms require whole-byte forward/reverse proof. Review TCLASS/TRANCLASS output widths and exclusions/defaults; optional MAXIMUM; AT/AFTER component receivers; retained-lock region openness; WLM report timestamp/API cadence versus health-change; all REMOTESYSTEM states; independent PROFILE timeout; all 28 authorization and21 negative-state precondition replacements; requested independent eight-character class sender; REQID raw mandatory TIME/prose contradiction and SET/LENGTH pointer choices explicitly pending; source PTT/class effects; actual before/after sender values and distinct receiver allocations; all ten timestamps using pinned FORMATTIME packed-decimal eight-byte ABSTIME in parent/form19 declarations and10 fixtures. Source opposition on threshold arithmetic, PRIORITY/TRACING/PURGEABILITY/OTSTIMEOUT/VOLUME/RECOVSTATUS/QUESCESTATE remains pending. Do not turn whole pending response/order/ABI assumptions into universal success or clone expected output as independent state. Review every changed implication, not sampled cases. Reuse unchanged complete VOLUME objects only after whole-source/pin/receipt equality.
Manager exclusively owns schema/generators/IR/security/UOW/status and actual
family/projection/focused/mandatory gates and serial sealing. Independent source
expectations and complete amended-property preservation must pass first. All six
gates and application/runtime/selected route/recovery/licensed/parent acceptance
remain pending, credit0. Exact rows: `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0010`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0024`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0031`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0032`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0082`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0088`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0089`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0115`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0153`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0156`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0167`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0176`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0178`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0188`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0192`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0217`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0240`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0247`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0256`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0258`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0264`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0268`.

The TERMINAL narrow repair author emits its candidate/patch externally because
an appended read-only review rule over-restricted repository writes. NETWORK
uses its explicit one-file ownership and preserves the frozen source preimage
externally. This is a manager prompt ambiguity, not a source/compiler failure.
Manager will import only after independent delta review. No live worker file
is edited by the manager or worker interrupted.

### Pre-dispatch CSD symbolic-domain source development

SPI-1001.spi-csd-cvda-domains depends on sealed CSD source contracts and the
existing symbolic-domain schema/emitter. Demonstrated inventory gap: all 25
CSD command parents currently have zero finite CVDA domains. A retained CLI
source author owns only spi-csd-definition.json and spi-csd-browse.json in a new
isolated worktree. Add only source-explicit option/values/source_lines arrays
in existing parent/form scopes and precise related pending gaps. Preserve all
25 rows, 443 option identities/roles/widths/citations, responses, existing forms
and all 475 complete cases/IDs/order and other lifecycle/security/UOW/recovery
facts. No numeric facets, new options/forms/cases, raw syntax inference from
command names, execution/alias/condition-order claims or source repins. Fresh
pinned search/read verifies command-specific accepted versus observed symbols
and qualifiers; finite-domain members require exact primary clauses and four-
byte declared operand extent. Independent changed-fact review is required
before manager sole generation/focused/actual instance/mandatory gates and
serial child sealing. Shared schema/IR/generator/status/security/UOW remain
manager-owned. All six gates and application/runtime/route/recovery/licensed/
parent acceptance remain pending, credit0.
Exact rows: `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0037`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0038`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0040`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0041`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0042`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0053`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0054`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0055`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0056`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0060`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0061`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0039`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0043`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0044`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0045`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0046`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0047`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0048`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0049`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0050`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0051`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0052`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0057`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0058`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0059`.

### Pre-dispatch spi-database-messaging-report-correction

SPI-1001.spi-database-messaging-report-correction depends on terminal independent review and frozen source SHA-256 bc66dddf5afca257819da8ce70372f394e142b79a76f4343d575c54d8c9cfcde. External output copies/candidates only; existing artifacts and checkout are immutable.
Resolve only DBMQ-CR01 four external report coordinates [16]→[14], not normative source data. Read exact terminal findings/handoff. Preserve all original external artifacts as immutable provenance: write corrected copies of the four listed JSON reports into your assigned NEW external root, plus an erratum mapping each original path/hash/pointer to corrected path/hash/coordinate. Update affected path/hash/byte refs in the corrected handoff where they point to the corrected report copies, retaining every unrelated reference. Fresh pinned search AND bounded read INQUIRE DB2CONN exactpin43060df40e712da776d3245422637d343a6e594034df64f1bbdb355671834f0f to verify syntax14 versus common response16 and raw SVG repeatable42choice. No product/candidate/source/repository/cache/shared writes or semantics change. Candidate SHA bc66dddf5afca257819da8ce70372f394e142b79a76f4343d575c54d8c9cfcde must remain exact. All 21 DB rows, 364 options, 192 responses and 550 cases remain unchanged. Independent source reviewer has already found no semantic defect. Complete old/new roundtrip proof allowing only four coordinates plus required report-link updates; original artifact bytes stay intact. Handoff.md/json and exact changed-copy refs, source argv/exit/pin/hash. No whole-family review repeat.
Manager owns shared contracts/status/actual integration gates/sealing. No source/runtime/selected-route/recovery/licensed/parent credit follows; all six gates Pending. Exact rows: `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0006`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0007`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0008`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0019`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0020`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0067`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0068`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0069`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0078`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0079`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0109`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0110`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0111`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0140`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0141`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0142`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0211`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0212`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0213`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0235`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0236`.

### Pre-dispatch spi-platform-programs-final-narrow-repair

SPI-1001.spi-platform-programs-final-narrow-repair depends on terminal independent review and frozen source SHA-256 f42ba9273c5178ac3bcfee56916325bf1689085086825f1ac9c9345dcee42151. External output copies/candidates only; existing artifacts and checkout are immutable.
Resolve only PP-RR-01 seven precondition strings from your terminal independent review. Write the repaired source candidate and standalone delta/full patches ONLY in the NEW external root; source checkout/frozen original remains unchanged. Final source base f42ba9273c5178ac3bcfee56916325bf1689085086825f1ac9c9345dcee42151 is frozen under spi-platform-programs-repair-review/frozen-spi-platform-programs.json. Replace exactly seven unsupported PGMIDERR/1 TRUE-with-SPI sentences with actual source alternatives: not-enabled originalidentity, omitted EXIT for global query, EXIT supplied for TRUE query. Preserve actual independent global state and required EXIT(XFCREQ), SPIST234–250 and explicit-global versus global-as-TRUE/missing-EXIT source reconciliation pending; no licensed precedence selected. Fresh pinned search AND bounded read INQUIRE EXITPROGRAM cb04d56bb29100d1536fb1cc5ea48ed2fc76c2e2a1dd8a4ac63c66cc23edeb22 body relevant314–319 plusSPIST qualifiers. Exactly seven existing input.preconditions properties allowed, all19rows330option identities150responses562caseIDs/order75domains221members19forms and all other complete facts equal. Preserve case expected strings/citations/request/response catalog/gaps, no generic success/alias/ABI inference. Produce property ledger seven pointers, complete seven old/new case objects, wholeforward/reverse originalbytes, exact source receipts, original other555wholecases proof and handoff.md/json. No broad unchanged source re-review. Requires a DIFFERENT retained thread to independentlyreview this concrete delta before manager import/gates/seal.
Manager owns shared contracts/status/actual integration gates/sealing. No source/runtime/selected-route/recovery/licensed/parent credit follows; all six gates Pending. Exact rows: `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0015`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0016`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0062`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0076`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0077`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0093`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0094`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0125`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0135`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0136`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0137`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0143`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0145`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0146`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0147`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0197`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0230`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0231`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0232`.

### Pre-dispatch NETWORK final narrow independent review

SPI-1001.spi-network-connections-final-narrow-review depends on terminal14-property repair SHA-256 a07e66ea26ed58adfceddae6cab0591f582c9af481262dccddc1c32f4a9b95c6. Different retained thread owns external reports only in isolated review checkout.
Independent review of exactly 14 repaired properties from NCRR-01–03: ten
SET TCPIP coupled limit/status expectations, three decisive source arrays and
one absent TCPIPSERVICE fixture. Review all 12 amended complete cases and
259 exact reused cases, 24 rows/332 options/188 responses/all271IDs/order.
Actual OPEN/CLOSED must remain genuine coupled actions, limit/NEWLIMIT
fullword/condition qualifiers and unknown order/partial effects remain precise;
no harmless selector, universal success, unchanged-on-error, caller rollback
or automatic retry. Decisive bounds1..65535 and MAXDATALEN3..524288 retain
units/superuser/CLOSED constraints and correct source support. MISSING1 cannot
have prior resource attributes; keep allocated128/32 sender areas, absent name,
authorization and original NOTFND13/3 with unresolved ordering. All other source
properties/forms/domains/responses/lifecycle/UOW/security/gaps remain exact.
Only two changed primary sources require fresh bounded search/read; unchanged
whole-family review reused only after complete preimage/source/receipt hashes.
Manager owns shared schema/IR/generator/status/security/UOW and actual instance/projection/focused/mandatory integration/sealing. All six and parent/application/route/recovery/licensed acceptance remain Pending, credit0. Exact rows: `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0005`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0013`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0028`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0066`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0073`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0085`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0108`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0112`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0124`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0128`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0129`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0130`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0131`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0168`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0169`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0194`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0196`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0210`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0214`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0225`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0226`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0227`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0248`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0249`.

### Pre-dispatch FILE symbolic-domain source development

SPI-1001.spi-file-cvda-domains depends on sealed FILE source contracts and the
existing symbolic-domain schema/emitter. Demonstrated inventory gap: all 4
FILE command parents currently have zero finite CVDA domains. A retained CLI
source author owns only spi-file.json only in a new
isolated worktree. Add only source-explicit option/values/source_lines arrays
in existing parent/form scopes and precise related pending gaps. Preserve all
4 rows, 139 option identities/roles/widths/citations, responses, existing forms
and all 166 complete cases/IDs/order and other lifecycle/security/UOW/recovery
facts. No numeric facets, new options/forms/cases, raw syntax inference from
command names, execution/alias/condition-order claims or source repins. Fresh
pinned search/read verifies command-specific accepted versus observed symbols
and qualifiers; finite-domain members require exact primary clauses and four-
byte declared operand extent. Independent changed-fact review is required
before manager sole generation/focused/actual instance/mandatory gates and
serial child sealing. Shared schema/IR/generator/status/security/UOW remain
manager-owned. All six gates and application/runtime/route/recovery/licensed/
parent acceptance remain pending, credit0.
Exact rows: `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0012`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0072`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0127`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0224`.

REGION numeric independent changed-fact review finds no actionable defect in
all 21 records, two scopes, exact source/case restoration and sole-projection
preimage. Source candidate keeps all 868 cases and every other family fact.
Focused 31 generator/nine IR, actual REGION instance and mandatory prereview
gates are reused on their byte-identical product inputs; later dispatch/status
declarations require this final current docs regeneration/check. Numeric
projection totals 62 domains/217 records, digest
sha256:fcdc517c8a88e6aac851663c8b37f242f3578d0338004e4183ecb6d07e47ca45.
Only SPI-1001.region-cvda-numeric-bindings child is sealed here. All 269 SPI/
39 FEPI parent, six command gates, application/route/runtime/recovery/licensed
acceptance remain pending, credit0. EVENT bounded source repair review has no
actionable defect and awaits subsequent serialized integration; DATABASE has
a report-only coordinate correction and PLATFORM seven qualification repairs.

EVENT source integration is manager-owned after all 424 repair operations, all 38
whole amended cases, 667 unchanged cases and independent no-defect review.
Source final7d630da49d4564008dac31e992cfbde6cb45471b70304bfb50bb14366e8d4e44
contains 19 rows, 267 options, 112 conditions, eight forms and 91 scoped domains
with 319 members,
544 obligations and 705 candidate cases. Five explicit START dependencies, distinct
response receivers and isolated conditional command-authorization denial are
source metadata. FEATUREKEY required VALUE versus example omission remains
pending. All original 149 gaps and the precise conflict are retained. Manager
whole-byte replay and 21-topic, 63-command pinned source consult are current source
review, not runtime/license evidence. Existing schema/types/shared runtime and
condition/security/UOW authorities remain unchanged. Only bounded source child
SPI-1001.spi-event-source-contracts is sealed after actual integration gates;
all six command gates and parent/application/route/recovery/licensed acceptance
remain pending, credit0. Primary baseline
ibm-cics-ts-6x-spi-command-bodies-2026-09-12; exact enrolled source pins remain
in spi-event-policy.json.

EVENT integrated candidate passes 31 generator and nine IR regressions and actual
Draft202012 family instance validation. Private projection has 192 commands
(153 SPI and 39 FEPI), 3240 operands and 5682 case candidates. Five of 18 enrolled
family inputs remain unintegrated; all-family wave and runtime gates pending.

### Pre-dispatch spi-platform-programs-final-narrow-review

SPI-1001.spi-platform-programs-final-narrow-review depends on terminal source candidate and bounded existing source contracts. Different retained CLI task owns external reports only; repository paths remain manager-owned.
Independent exact-seven-precondition review of PP-RR-01. Reviewer differs from original platform author01a0fcfc-90c0-7060-a0c2-d2754d3f78a7 and narrow repair author01a0fced-45da-7442-b3d2-82ace535b067. Only /commands/7/obligations/5/cases/{7,12,13,16,20,23,27}/input/preconditions changed; read all seven whole request/precondition/expected/citation relationships. PGMIDERR/1 must reflect not-enabled identity, missing EXIT global inquiry or EXIT supplied TRUE; never unsupported not-defined-with-SPI. Preserve EPONE/global EXIT(XFCREQ) premise. SPIST234-250 NOTAPPLIC global versus NOSPI global-as-TRUE with omitted EXIT and PGMIDERR1 missing-EXIT qualification314-319 remain precise opposed context; do not select success/precedence. Fresh bounded primary INQUIRE EXITPROGRAM cb04d56bb29100d1536fb1cc5ea48ed2fc76c2e2a1dd8a4ac63c66cc23edeb22, row0125 baselineibm-cics-ts-6x-spi-command-bodies-2026-09-12 search/read122-139,234-250,314-319. Verify exact f42ba9273c5178ac3bcfee56916325bf1689085086825f1ac9c9345dcee42151 preimage and 555 unchanged cases, all562IDs/order,19rows330options150responses19forms75domains221members115obligations206gaps87output-fixture gaps. Earlier full221-property review provenance retained only by equality; no rerun unchanged whole-family campaign.
Exact rows: `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0015`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0016`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0062`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0076`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0077`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0093`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0094`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0125`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0135`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0136`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0137`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0143`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0145`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0146`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0147`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0197`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0230`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0231`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0232`.
Shared schema/IR/generator/status/security/UOW/runtime remain manager-owned. Independent source review and manager actual instance/projection/focused/mandatory checks required before serial child seal; all six and parent/application/route/recovery/licensed Pending credit0. Input hashes: {'conformance/0.10/cics/families/spi-platform-programs.json': '4c005d771472af8b7b678346e3ff03029f12d03e254d9cf8f8fbd7f5dc01ff82'}.

### Pre-dispatch spi-csd-cvda-domains-review

SPI-1001.spi-csd-cvda-domains-review depends on terminal source candidate and bounded existing source contracts. Different retained CLI task owns external reports only; repository paths remain manager-owned.
Independent full changed-fact review of exactly16addition operations:12 parent symbolic domains262members over seven definition parents plus nine precise unresolved gaps (seven domain-array property additions plus nine gap entries). Reviewer differs from domain author01a0fcfc-90c0-7060-a0c2-d2754d3f78a7. All25rows443options250responses475whole cases, all forms/IDs/order/306originalgaps exact. ALTER/DEFINE/USERDEFINE COMPATMODE(COMPAT,NOCOMPAT)+RESTYPE37 each; COPY DUPACTION(DUPERROR,DUPNOREPLACE,DUPREPLACE)+RESTYPE37; DELETE LISTACTION(REMOVE)+RESTYPE36 excludes MQMONITOR in pinned diagram; INSTALL RESTYPE31 with TERMINAL and single-resource CONNECTION/SESSIONS/TERMINAL pool restrictions; RENAME RESTYPE37. Review every262source symbol and operation qualification from fresh pinned search/read before independent expected facts; no clone/global-table assumption. GETNEXTRSRCE returned CVDA list not closed, INQUIRERSRCE37 syntax selectors versus output wording/direction not resolved: retain two precise output gaps, no output domains invented. Distinguish command SVG token labels from common/numeric/global catalogs; no repins/table extrapolation/numeric bindings/new forms/new case values/default receiver geometry. Frozen schema closed option/values/source_lines and existing declared 4byte operand role required. Producer had26freshsourcecalls; fresh changed-topic independent lookups bounded by actual relevant spans; old complete185receipt fullcampaign reused only after exact proofs.
Exact rows: `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0037`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0038`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0040`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0041`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0042`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0053`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0054`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0055`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0056`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0060`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0061`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0039`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0043`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0044`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0045`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0046`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0047`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0048`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0049`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0050`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0051`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0052`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0057`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0058`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0059`.
Shared schema/IR/generator/status/security/UOW/runtime remain manager-owned. Independent source review and manager actual instance/projection/focused/mandatory checks required before serial child seal; all six and parent/application/route/recovery/licensed Pending credit0. Input hashes: {'conformance/0.10/cics/families/spi-csd-definition.json': '257fbb35ab3bb577cd7886951baebde80362e46ce699e1286b5e9bc3b6ae32ac', 'conformance/0.10/cics/families/spi-csd-browse.json': 'a41131654381cfdd4fcc15b8fe9ee25d2da4d8f4525b57d4666bcae728aaadb4'}.

### Pre-dispatch spi-terminal-sessions-citation-compaction

SPI-1001.spi-terminal-sessions-citation-compaction depends on terminal source candidate and bounded existing source contracts. Different retained CLI task owns external reports only; repository paths remain manager-owned.
Bounded external candidate repair authorized for EXACT seven source_lines arrays listed completely in prior residual-shape-dispositions.json. The five earlier TSRR01/02 amended strings must remain exact. Start from frozen external1cc30eef candidate, not older repository75289 candidate. Compact only /commands/6/obligations/11/cases/0/source_lines; /commands/19/obligations/{104,107}/cases/0/source_lines; /commands/23/obligations/0/cases/0/source_lines; /commands/26/obligations/12/cases/8/source_lines and /commands/26/obligations/{15,16}/cases/0/source_lines. Existing22,18,22,27,28,27,17 members exceed max16. Fresh relevant CREATE TERMINAL9fa44fccb464fdd43cb09d58611f0094153316af529c0e8fd303a06d3654204b (49-91), INQUIRE TERMINAL4b76c16256a9810ea00d83800ecc1a52384029c5952da6b93d58c05b0013320e (472-584,745-784), SET MODENAME0e557e82a7e38138f144c74dbb8e29baafe6a0b64dbf6d35a4b3cacee2c0ad91 (31-73), SET VTAMe74032ca2487b4cecf788eddd21aaedf4090a517c782df31185b1d23c46afa69 (60-157). Select at most16 decisive actual clause lines per case preserving every semantic requirement with per-old-citation disposition and whole actual case/support coverage, never truncate first16 or replace with generic headings. If obligations cannot be supported within16 report exact genuine block without schema relaxation or semantic rewrite. All27rows505options200responses+two unmapped NOTFOUND145domains10forms1133IDs and other1126 wholecases? Original five-property repair changed three whole cases; this new seven-array slice preserve every other wholecase and exact requests/preconditions/expectations. No other citations/schema/forms/domain/gap/test/runtime changes. Candidate external only, source/input/historical reports/index/cache unchanged. Different-thread combinedfiveplusseven review required afterward; do not self-approve.
Exact rows: `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0001`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0018`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0021`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0022`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0025`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0027`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0030`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0034`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0064`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0080`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0083`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0087`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0098`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0099`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0102`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0138`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0144`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0149`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0154`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0172`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0189`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0207`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0209`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0233`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0237`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0252`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0265`.
Shared schema/IR/generator/status/security/UOW/runtime remain manager-owned. Independent source review and manager actual instance/projection/focused/mandatory checks required before serial child seal; all six and parent/application/route/recovery/licensed Pending credit0. Input hashes: {'conformance/0.10/cics/families/spi-terminal-sessions.json': '1cc30eefd8e3ba2756b273b56ccb065d67c8f0888be7056c43fc3ac6f1aed5a9'}.

WEB finite-domain integration uses exact42additions and whole-byte forward/reverse
source proof. All25rows336options134responses6forms550completecases unchanged.
Ninety-nine parent/form domains contain329 source-explicit symbol memberships;
no new operand, form, case, numeric binding, execution route or runtime authority.
Fresh manager19topic41command pinned search/read verifies command-specific sender
and inquiry receiver domains and fullword rules. Existing lifecycle, USAGE, bundle,
6.3 ATTLS and beta VALIDATEHOST qualifiers and precise unresolved defaults remain
pending source context; no universal execution/default/alias interpretation.
Independent different-thread review reports zero actionable findings.
SPI-1001.spi-web-cvda-domains is a bounded source/projection child only. Baseline
ibm-cics-ts-6x-spi-command-bodies-2026-09-12 and command pins/rows remain in
spi-web-resources.json; common dfha80x baseline
ibm-cics-ts-6x-application-api-sources-b-2026-09-10. All six and parent/application/
selectedroute/recovery/licensed acceptance Pending credit0.

WEB finite-domain integrated candidate passes31generator and nineIR regressions
and actual Draft202012 WEB instance validation. Private192command projection
(153SPI39FEPI),3240operands5682case candidates unchanged;62numeric domains and
217numeric records unchanged. Five of18inputs and all-family/runtime gates pending.

### Pre-dispatch TRANSACTION final citation independent review

SPI-1001.spi-transaction-resources-final-citation-review depends on terminal complete independent review and manager external one-property correction SHA-256 8e7fb0b6f03af475db80d69abe7b6b6659a95c6bb32e4e17261f65fe4472ddd7. Retained independent reviewer owns external reports only; no source writer session is live.
Independent final exact-one-property citation review TXRR-01. Manager-owned
external repair changes only /commands/16/lifecycle/mutations/0 embedded citation
from primary55-64,73-75 to55-64,67-69,73-75, adding decisive enable prerequisite.
All22rows311options148responses64domains178members5forms749wholecases and every
other fact remain byte-identical. Fresh SET PROCESSTYPE search/read exact primary
e535e7719eb0f39b8ae1f1c8267920f0d1376efefee4c8f867277c11e0e0262a, row0240,
baselineibm-cics-ts-6x-spi-command-bodies-2026-09-12 lines55-78. Do not infer
drain, termination, caller rollback, precedence or process recovery. Prior complete
275operation223group117repaired+12adjacentcase25topic61call independent review
reused only after exact f4714d96 preimage/source/report/receipt identities.
Exact rows: `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0010`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0024`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0031`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0032`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0082`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0088`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0089`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0115`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0153`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0156`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0167`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0176`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0178`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0188`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0192`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0217`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0240`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0247`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0256`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0258`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0264`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0268`.
Manager exclusively owns shared schema/IR/generator/status/security/UOW/runtime, actual instance/projection/focused/mandatory integration and serial seal. All six and parent/application/route/recovery/licensed Pending credit0.

### Pre-dispatch spi-file-cvda-domains-review

SPI-1001.spi-file-cvda-domains-review depends on terminal source candidate and bounded existing source contracts. Different retained CLI task owns external reports only; repository paths remain manager-owned.
Independent full changed-fact FILE review of exactly13additions:three parent domain arrays44domains138members plus10related gaps. Different reviewer from source author01a0fdea-71a3-7323-a602-468bea1dc88d. Originala153ca5bba7c4ab9212be8a8dfdaacab00be045f46b80150b0f7895fe55abd55; all4rows139operandobjects91responses18obligations166wholecases0forms51oldgaps and everyother lifecycle/security/UOW/recovery fact exact. CREATE FILE LOGMESSAGE1domain2members sender; DISCARD has no CVDA; INQUIRE27domains100members output; SET16domains36members input. Review each symbol against command-specific source clauses, existing declared4byte operand extent/direction and surrounding applicability/transition rules, not similarly named SET versus observed inquiry lists. ACCESSMETHOD data-table VSAM, ENABLESTATUS versus OPENSTATUS, READINTEG saved RLS/nonRLS NOTAPPLIC/per-read overrides, LOADTYPE/UPDATEMODEL saved nonCFDT outputs, FWDRECSTATUS/RECOVSTATUS first-versus-last-open/ICF, RECORDFORMAT BDAM UNDEFINED/usertableVARIABLE and distinct provenance sets must remain exact. SET BUSY onlyDISABLED/CLOSED, ignored irrelevant attributes, closed prerequisites/next-open effects, retained locks/bundles and UOW boundaries; READINTEG CFDT ignore/nonRLS, TABLE filekind, recoverableCFDT LOCKING retained. Precise10gap additions retain CREATE logging omitted-default unknown notation, SET LOADTYPE data-value heading versus raw cvda/list/invalidCVDA condition, BLOCKFORMAT BDAM heading versus VSAM BLOCKED, RELTYPE BDAM versus VSAM NOTAPPLIC, EMPTYREQ ignore versus INVREQ57, WAITcompletion versus example start, linkedunknownhashes. Inspect actual SVG where implicated, do not infer defaults/aliases or newnumeric facets/forms/cases. Fresh pinned relevant4primaries+common CVDA/APIformat searches AND boundedreads required; unchanged prior84receipt29pin campaign reused only after exact full identity. All raw bodies external reference credit0.
Exact rows: `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0012`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0072`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0127`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0224`.
Shared schema/IR/generator/status/security/UOW/runtime remain manager-owned. Independent source review and manager actual instance/projection/focused/mandatory checks required before serial child seal; all six and parent/application/route/recovery/licensed Pending credit0. Input hashes: {'conformance/0.10/cics/families/spi-file.json': '67f10293e2d47572302dc32a931c79e51d61b981429c748ef8c1233b28e5a957'}.

DATABASE integration child SPI-1001.spi-database-messaging-source-contracts is
manager-owned, bounded to 21 exact enrolled DB2/MQ rows and 84 obligations;
364 head operands, 192 conditions, 14 forms, 99 scoped domains/325 members and
550 source-derived cases remain private and non-routing. Final source
bc66dddf5afca257819da8ce70372f394e142b79a76f4343d575c54d8c9cfcde
passed independent full semantic review and manager review of every 164 repair
operation and 37 amended whole cases, preserving 513 whole cases and 144 gaps.
Manager verified the external report-only 16-to-14 citation erratum without
changing candidate bytes. All 16 source topics and 60 pinned search/read receipts
are source review with zero execution credit. Mandatory 42-output DB2CONN and
16-output DB2ENTRY alternatives, bare selectors, seeded receiver fixtures and
explicit CREATE implicit-syncpoint metadata retain precise pending conflicts.
Baseline ibm-cics-ts-6x-spi-command-bodies-2026-09-12; topic pins/catalog rows
are enrolled in spi-database-messaging.json. Manager exclusively owns generator,
shared schema/IR facade/status and integration; source import adds no separate
dispatcher, condition mapper, security, UOW, resource or persistence authority.
Only this child may be sealed after focused current-candidate tests and mandatory
policy/schema/docs/module gates. Parent SPI-1001, all six per-command gates,
application dependency, selected-route, recovery and licensed acceptance remain
Pending; official/execution/licensed credit stays zero.

DATABASE integrated candidate passes 31 generator and nine IR regressions plus
actual Draft202012 family instance validation. Private projection has 213 commands
(174 SPI and 39 FEPI), 3604 head operands and 6232 case candidates. Four of 18
enrolled family inputs remain unintegrated; all-family and runtime gates pending.

NETWORK child SPI-1001.spi-network-connections-source-contracts is manager-owned
and bounded to 24 enrolled network rows/24 obligations, 332 options, 188
conditions, four forms, 73 scoped symbolic domains and 271 source-derived cases.
Exact final a07e66ea26ed58adfceddae6cab0591f582c9af481262dccddc1c32f4a9b95c6
passed prior full-family and different-thread final review. Manager inspected all
14 final repaired properties and 12 complete amended cases; whole-byte forward
and reverse replay preserve 259 other cases, IDs/order and all 200 precise gaps.
Manager's two-topic/four-call pinned search/read consult rechecks SET TCPIP and
SET TCPIPSERVICE; prior full-family 51-call review stays source provenance only.
Coupled limit and OPEN/CLOSED effects, clamping, interrupted and partial opening,
receiver validity and absent-resource fixture remain bounded pending candidates.
No caller rollback, universal attainment or automatic redispatch is inferred.
Baseline ibm-cics-ts-6x-spi-command-bodies-2026-09-12; exact catalog rows/body
pins remain enrolled in spi-network-connections.json. Sources stay private,
non-routing and unadvertised. Manager exclusively owns shared facade/generator,
schema/status and serial integration; existing runtime/resource/condition/security/
UOW/persistence authorities are unchanged. Only this source child may be sealed
after actual family instance and focused/mandatory integration checks; all six
command gates, SPI-1001 parent, application dependency, selected-route/recovery
and licensed acceptance remain Pending with execution/official/licensed credit0.

NETWORK integrated candidate passes 31 generator and nine IR regressions plus
actual Draft202012 family instance validation. Private projection has 237 commands
(198 SPI and 39 FEPI), 3936 head operands and 6503 case candidates. Three of 18
enrolled family inputs remain unintegrated; all-family and runtime gates pending.

TRANSACTION child SPI-1001.spi-transaction-resources-source-contracts is
manager-owned and bounded to 22 enrolled transaction/process rows with 311
operands, 148 conditions, five forms, 64 domains/178 members and 749 source cases.
Final 8e7fb0b6f03af475db80d69abe7b6b6659a95c6bb32e4e17261f65fe4472ddd7
passed independent full-family review and final one-property citation review.
The sole final correction adds primary67-69 to SET PROCESSTYPE's existing
already-disabled prerequisite; all 749 whole cases, every other fact and all
249 precise gaps are preserved. Manager byte replay and current pinned
search/read agree with the independent expectation; prior 275-action/223-group
full repair review, 25 pins and 61 source-call identities remain immutable
source-only provenance. Manager verified all 35 final handoff artifact refs.
Primary baseline ibm-cics-ts-6x-spi-command-bodies-2026-09-12; command catalog
rows and exact source pins stay enrolled in spi-transaction-resources.json.
REQID raw/prose/pointer, threshold, tracing, purgeability, partial effects, SAF,
ABI, caller UOW and recovery conflicts remain Pending. Source contracts stay
private and non-routing. Manager exclusively owns shared schema/generator/IR
facade/status and serial integration; no runtime/condition/resource/security/UOW
authority is duplicated. Only this bounded source child may seal after actual
family Draft202012, focused regressions and mandatory integration gates. All
six command gates, application dependency, SPI-1001 parent, selected route,
physical recovery/restart and licensed acceptance remain Pending, credit0.

TRANSACTION integrated candidate passes 31 generator and nine IR regressions
plus actual Draft202012 family validation. Private projection has 259 commands
(220 SPI and 39 FEPI), 4247 head operands and 7252 case candidates. Two of 18
enrolled family inputs remain unintegrated; all-family and runtime gates pending.

### Pre-dispatch spi-terminal-sessions-combined-final-review

SPI-1001.spi-terminal-sessions-combined-final-review depends on terminal source candidate and bounded existing source contracts. Different retained CLI task owns external reports only; repository paths remain manager-owned.
Independent combined final review; reviewer must differ from original/repair author01a0fd23-a3ae-7732-aa5c-78e740726af8 and cannot self-approve. EXACT combined12 changed properties: five TSRR01/02 strings and seven source_lines arrays, ten affected whole cases, 1123 unchanged whole cases relative75289. Current finale973cf223105abaf5f3827ca093406e1d778611281ecfb86377dcb3f9b0f17c9; intermediate1cc30eef used only to isolate lastseven arrays. Review all actual12 before/after fields and complete10 request/receiver/preconditions/expectations/citation relationships, all161 original citation-member dispositions (109retained/52coalesced), two fullword declaration additions. Each array sorted unique1..16; no first16 truncation/generic-heading substitution or unsupported loss. Fresh pinned search/read of four relevant originaltopics plus any bounded primary needed for five earlier strings: CREATE TERMINAL9fa44fcc (full133lines), INQUIRE TERMINAL4b76c162 (472-584,745-784), SET MODENAME0e557e82 (full113), SET VTAMe74032ca (full193). Preserve partialpool implicit-syncpoint/earlyexception UOW, NQNAME relogon/warm/emergency origin, tracing-policyvsSUPPRESSED, TYPETERMreceiptvsPROFILEpresentation, APPC/SNASVCMG/MAXIMUM/DFqueue recoverability-dependent wait, OPEN/ACB partialattainment/PSDINT reset, interval bounds/fullword and deferred CLOSEDACBdelivery. No source contradiction or ABI/context/range/order/recovery unknown discarded. Exactall27rows505operands200numericconditions+two unmappedNOTFOUNDcaseclauses145parentdomains10forms1133IDs/allgaps/current719schema. Reuse immutable fullfamily priorreview only after exact endpoint/report/body/SVG/source-command receipt hashes; no repeat unchanged whole-source campaign. Wholebyte combined/intermediate/original forwardreverse and fulladdition diff must restore exactfiles/keyorder/allunchangedfacts. Actual Rust Draft202012 validation and allintegrationtests/sealing remain manager-owned Pending. Only externalreviewreports, no candidate/source edits.
Exact rows: `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0001`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0018`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0021`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0022`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0025`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0027`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0030`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0034`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0064`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0080`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0083`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0087`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0098`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0099`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0102`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0138`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0144`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0149`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0154`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0172`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0189`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0207`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0209`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0233`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0237`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0252`, `ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0265`.
Shared schema/IR/generator/status/security/UOW/runtime remain manager-owned. Independent source review and manager actual instance/projection/focused/mandatory checks required before serial child seal; all six and parent/application/route/recovery/licensed Pending credit0. Input hashes: {'conformance/0.10/cics/families/spi-terminal-sessions.json': 'e973cf223105abaf5f3827ca093406e1d778611281ecfb86377dcb3f9b0f17c9'}.

PLATFORM child SPI-1001.spi-platform-programs-source-contracts is manager-owned
and bounded to 19 enrolled platform/exit-program rows, 115 obligations, 330
options, 150 conditions, 19 forms, 75 domains/221 members and 562 source cases.
Exact final 4c005d771472af8b7b678346e3ff03029f12d03e254d9cf8f8fbd7f5dc01ff82
passed prior full-family and different-thread final seven-precondition review.
Manager inspected every final actual field and all seven complete case relations;
whole-byte replay preserves 555 other cases, all IDs/order, all 206 gaps and 87
output-fixture gaps. Original ENABLE identity/default ENTRYNAME, explicit-global
EXIT(XFCREQ), option-specific NOTAPPLIC and omitted-EXIT global-as-TRUE NOSPI
remain qualified; PGMIDERR/1's not-enabled/missing-EXIT/global/TRUE alternatives
replace unsupported source attribution without choosing a failure or precedence.
Manager fresh search/three reads cover 41 decisive primary lines at row0125,
INQUIRE EXITPROGRAM cb04d56bb29100d1536fb1cc5ea48ed2fc76c2e2a1dd8a4ac63c66cc23edeb22,
baseline ibm-cics-ts-6x-spi-command-bodies-2026-09-12. Full earlier 221-property
source review is immutable provenance; 31 final referenced artifacts verified,
using its immutable status-read snapshot for the historical manager declaration.
All candidate source pins/catalog rows remain enrolled in spi-platform-programs.json.
Source metadata stays private/non-routing. Manager exclusively owns schema,
generator/IR facade/status and serial integration; no duplicate runtime/condition/
resource/security/UOW/persistence authority. Only this bounded child may seal
after actual family instance, focused regressions and mandatory integration gates;
all six command gates, parent, application dependency, selected route, physical
recovery/restart and licensed acceptance remain Pending, credit0.

PLATFORM integrated candidate passes 31 generator and nine IR regressions plus
actual Draft202012 family instance validation. Private projection has 278 commands
(239 SPI and 39 FEPI), 4577 head operands and 7814 case candidates. One of 18
enrolled family inputs remains unintegrated; all-family and runtime gates pending.

FILE integration child SPI-1001.spi-file-cvda-domains is manager-owned after
independent review of all 13 additions. All four exact enrolled rows, 139 complete
operands, 91 responses, 18 obligations, zero forms and 166 whole cases remain
unchanged. Three parent-array additions contain 44 scoped domains/138 symbolic
members; ten precise source gaps are added and all 51 original gaps retained.
Manager read every actual domain/qualifying member and full added gap, verified
whole-byte forward/reverse replay and 33 handoff artifact identities, and used
four pinned topics/29 fresh search/read calls including 25 bounded primary pages.
CREATE logging/default geometry, INQUIRE open history/local/remote/saved settings,
first/last-open recovery/ICF and SET value-dependent closed-state/RLS/CFDT rules
remain qualified. BUSY, FORCE, ignored attributes, deferred close and prior
recoverable-work commit requirements do not imply rollback or universal success.
LOADTYPE heading/raw-CVDA, BLOCKFORMAT/RELTYPE applicability, CFDT EMPTYREQ
versus INVREQ57 and WAIT completion/example-start oppositions remain Pending.
Baseline ibm-cics-ts-6x-spi-command-bodies-2026-09-12; exact rows/pins stay in
spi-file.json. Common fullword/direction dfha80x uses
ibm-cics-ts-6x-application-api-sources-b-2026-09-10. No numeric encoding, new
operand/case/form, runtime route or duplicate shared authority is added. Manager
solely owns schema/types/generator/IR facade/status and serial integration.
Only this bounded source child seals after actual FILE instance, projection,
focused and mandatory gates. All six per-command, parent/application/selected
route/recovery/restart/licensed gates stay Pending, credit0. Historical report
only FULLAPI coverage finding SPI-FILE-REPAIR-REVIEW-R1 remains explicit pending
provenance; no acceptance or retrospective fixture claim is inferred.

FILE domain integrated candidate passes 31 generator and nine IR regressions and
actual Draft202012 FILE instance validation. Private 278-command projection
(239 SPI and 39 FEPI), 4577 operands/7814 case candidates and all 62 numeric
domains/217 numeric records are unchanged. One of 18 enrolled family inputs
remains unintegrated; all-family/runtime/acceptance gates remain Pending.

CSD integration child SPI-1001.spi-csd-cvda-domains is manager-owned after the
independent no-actionable-defect review of exactly 16 additions: seven parent
arrays with 12 symbolic domains/262 members and nine precise pending gaps.
All 25 rows, 443 complete operands, 250 responses, 475 whole cases and their
IDs/order, obligations/forms and 306 original gaps remain exact. Manager verified
whole-byte forward/reverse replay, all actual domains/symbols, nine entire gaps
and qualifiers, 533 referenced artifact identities and 26 fresh offline calls
on 12 pinned topics, with all 14 actual bounded parser pages read. Own raw SVG
labels independently confirm each operation's selector list. ALTER/DEFINE/
USERDEFINE COMPATMODE defaults, COPY DUPACTION and forms, DELETE's own 36-name
list lacking MQMONITOR, INSTALL's own 31-name list/TERMINAL pool prohibition,
partial installation/CSDE and implicit-syncpoint early-exception rules remain
qualified. RENAME gains no fabricated default/requiredness. GETNEXTRSRCE output
membership and INQUIRERSRCE direction remain unresolved without finite domains.
Common fullword/direction and global all-command table do not close a receiver.
Primary baseline ibm-cics-ts-6x-spi-command-bodies-2026-09-12; exact rows/topic
pins stay in both CSD family inputs. Context baselines remain sources-a/b and
ibm-cics-ts-6x-misc-tail-cvda-2026-09-23. No numeric encodings, operand/form/case,
receiver geometry, runtime route or separate resource/UOW authority is added.
Manager solely owns schema/types/generator/IR facade/status and serial integration.
Only this bounded source child seals after actual CSD instances, projection,
focused and mandatory gates. All six per-command, parent/application/selected
route/recovery/restart/licensed gates remain Pending, credit0. Three deferred
v0.9 identities stay catalogued; no application acceptance is inferred.

CSD domain integrated candidate passes 31 generator and nine IR regressions and
actual Draft202012 definition and browse instance validation. Private 278-command
projection (239 SPI and 39 FEPI), 4577 operands/7814 case candidates and all
62 numeric domains/217 numeric records remain unchanged. TERMINAL source input
still awaits independent final review/integration; runtime acceptance pending.

TERMINAL integration child SPI-1001.spi-terminal-sessions-source-contracts is
manager-owned after full-family and different-thread combined final review.
All 27 rows, 505 operands, 200 numeric response clauses plus two unmapped
NOTFOUND case clauses, 145 domains/487 symbols, ten forms and 1133 whole cases
retain source scope. Exactly five string and seven citation-array changes affect
ten cases; 1123 whole cases and all IDs/order/gaps/grammar remain exact. Manager
read every actual old/new field and all ten entire cases/relations, accounted all
161 old citation members (109 retained/52 coalesced), and inspected complete
qualified clauses around compacted anchors plus two specific fullword additions.
Manager whole-byte forward/reverse replay, 74 artifact identities and 14 fresh
pinned offline calls on six topics/eight actual pages pass. USERDATALEN maximum255
has no fabricated minimum; BGAM/BSAM NETNAME binding/output equality remain
unresolved. Pool syncpoint/early exception concerns recoverable task work without
installed-definition rollback. NQNAME relogon and device catalog origins differ;
terminal trace policy/SUPPRESSED and TYPETERM receipt/PROFILE presentation remain
separate. APPC parallel SESSIONS excludes SNASVCMG and DF recoverability can defer
CLS1 until commit. OPEN partial attainment, unsupported interval reset, component
fullwords/conditional59 bounds and closed-ACB later delivery remain qualified.
Baseline ibm-cics-ts-6x-spi-command-bodies-2026-09-12, product SSJL4D_6.x; exact
catalog rows/topic pins stay in spi-terminal-sessions.json. Unknown linked pins,
NOTFOUND numeric identity, omitted extent/defaults, BMS/PROFILE/printing/CREATESESS
source oppositions, exact SAF/ABI and recovery remain precise Pending. No guessed
alias, universal success, caller-UOW atomic rollback or automatic redispatch.
Manager exclusively owns schema/types/generator/IR/status and serial integration.
Only this bounded source child seals after actual family/all-family instances,
projection preservation, focused and mandatory gates. Parent SPI-1001, all six
per-command gates, application dependency, selected route, recovery/restart and
licensed acceptance remain Pending, credit0; no routing is advertised.

TERMINAL integrated candidate passes 31 generator and nine IR regressions plus
actual Draft202012 family instance and all-18-family validation. Private source
projection has 305 commands (266 SPI and 39 FEPI), 5082 head operands and 8947
case candidates. Removing only 27 added TERMINAL rows restores the exact prior
278-command grammar projection, including all FILE/CSD domains and all62 numeric
domains/217 records. Three official raw PERFORM rows remain without source joins;
269 SPI /39 FEPI denominator and mandatory behavioral obligations are unchanged.
All18 mapped source families are enrolled; source inventory is not execution,
application dependency or complete parent/acceptance evidence.

### Pre-dispatch spi-monitoring-cvda-domains

Bounded SOURCE AUTHOR slice SPI-1001.spi-monitoring-cvda-domains. Produce a reviewable external candidate for ONLY conformance/0.10/cics/families/spi-monitoring-control.json, exact original SHA fd707eff3d2cd6f6166ccfcf45ab4582d4b320242a5fc5bd1e4c385b639286f5, retaining every existing row, complete operand, response, form, obligation, whole case/ID/order and all original gaps exactly. Demonstrated inventory gap: zero parent finite symbolic CVDA domains across 21 mapped rows. Add cvda_domains only to an existing grammar/form which has an exact declared option and a command-specific source-closed finite symbolic membership; allowed domain keys option/values/source_lines, not invented bytes/ABI/numeric encodings. Existing declared direction/extent must remain exact; unresolved direction, data-value heading conflict, open output list, service applicability or missing pin stays a precise gap, never guessed input/output. Symbol spellings/aliases remain exact source identities; no global CVDA namespace/default/other-command list extrapolation. Preserve any existing per-form domains; adding domains must not alter existing constraints or cases. Add only precise pending gap strings, without deleting or rewriting old ones. No optional numeric facets, named form, case, response, resource/state/condition/runtime/security/UOW/evidence schema/generator changes. Derive each finite member from its OWN pinned option paragraph or raw syntax token branch before comparing with candidate expectations. Read relevant source qualifiers/defaults/exclusions/opposition/implicit-syncpoint/atomicity/error clauses, retain unresolved service-level specifics. For monitoring preserve the two earlier exact RESP2 mapping repairs, component-specific trace scope, OTEL6.3 qualification and dump/monitor persistence conflicts. For queue preserve intra/extra/remote/application distinctions, ignored/retained flags, rationalized ENQ/journal aliases, TS browse lifetimes/receiver layouts, recoverability/syncpoint exceptions. Source membership is metadata only; all per-command gates and accepted runtime/selected route/license/parent credit stayPending0. Different-thread independent review will follow; do not self-approve. Candidate output /Users/tore/.codex/worker-runs/v010-20261002/spi-monitoring-cvda-domains/spi-monitoring-control.json plus full/delta diff, complete exact property/member/qualifier/gap source ledger, whole byte forward/reverse replay proof and unchanged complete case/order inventory. Fresh offline search/read for every changed exact topic, bounded pages including all relevant qualifiers; source bodies stay outsideGit. Stop when this bounded artifact is concrete and reviewable; do not repeat unchanged broad full-source campaigns after exact historical identities verified.
Exact owned rows: ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0002, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0095, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0116, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0139, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0148, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0159, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0164, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0173, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0174, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0175, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0177, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0195, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0218, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0234, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0238, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0243, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0244, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0253, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0254, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0255, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0257.
Dependency: sealed305-command source projection plus original reviewed family/schema. Sole worker owns external candidate/report root; repository file remains read-only. Manager owns shared schema/types/generator/IR/status/security/UOW/runtime, actual focused/family/mandatory gates and serial child seal; independent different-thread source review required; all six/parent/application/recovery/licensed Pending0.

### Pre-dispatch spi-queue-cvda-domains

Bounded SOURCE AUTHOR slice SPI-1001.spi-queue-cvda-domains. Produce a reviewable external candidate for ONLY conformance/0.10/cics/families/spi-queue-storage.json, exact original SHA a7a4a7a270cf4a768d46fba275c253cf06598445b809157396ce0a450e5e4ecc, retaining every existing row, complete operand, response, form, obligation, whole case/ID/order and all original gaps exactly. Demonstrated inventory gap: zero parent finite symbolic CVDA domains across 30 mapped rows. Add cvda_domains only to an existing grammar/form which has an exact declared option and a command-specific source-closed finite symbolic membership; allowed domain keys option/values/source_lines, not invented bytes/ABI/numeric encodings. Existing declared direction/extent must remain exact; unresolved direction, data-value heading conflict, open output list, service applicability or missing pin stays a precise gap, never guessed input/output. Symbol spellings/aliases remain exact source identities; no global CVDA namespace/default/other-command list extrapolation. Preserve any existing per-form domains; adding domains must not alter existing constraints or cases. Add only precise pending gap strings, without deleting or rewriting old ones. No optional numeric facets, named form, case, response, resource/state/condition/runtime/security/UOW/evidence schema/generator changes. Derive each finite member from its OWN pinned option paragraph or raw syntax token branch before comparing with candidate expectations. Read relevant source qualifiers/defaults/exclusions/opposition/implicit-syncpoint/atomicity/error clauses, retain unresolved service-level specifics. For monitoring preserve the two earlier exact RESP2 mapping repairs, component-specific trace scope, OTEL6.3 qualification and dump/monitor persistence conflicts. For queue preserve intra/extra/remote/application distinctions, ignored/retained flags, rationalized ENQ/journal aliases, TS browse lifetimes/receiver layouts, recoverability/syncpoint exceptions. Source membership is metadata only; all per-command gates and accepted runtime/selected route/license/parent credit stayPending0. Different-thread independent review will follow; do not self-approve. Candidate output /Users/tore/.codex/worker-runs/v010-20261002/spi-queue-cvda-domains/spi-queue-storage.json plus full/delta diff, complete exact property/member/qualifier/gap source ledger, whole byte forward/reverse replay proof and unchanged complete case/order inventory. Fresh offline search/read for every changed exact topic, bounded pages including all relevant qualifiers; source bodies stay outsideGit. Stop when this bounded artifact is concrete and reviewable; do not repeat unchanged broad full-source campaigns after exact historical identities verified.
Exact owned rows: ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0011, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0014, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0017, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0029, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0033, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0071, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0074, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0075, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0086, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0090, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0107, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0117, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0118, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0132, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0133, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0134, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0162, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0170, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0171, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0179, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0180, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0181, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0182, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0219, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0228, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0229, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0250, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0251, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0259, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0260.
Dependency: sealed305-command source projection plus original reviewed family/schema. Sole worker owns external candidate/report root; repository file remains read-only. Manager owns shared schema/types/generator/IR/status/security/UOW/runtime, actual focused/family/mandatory gates and serial child seal; independent different-thread source review required; all six/parent/application/recovery/licensed Pending0.

### Manager development SPI-1001.condition-response-identity

Bounded manager-owned source-gate change: bind each declared SPI/FEPI response
condition/name to the existing common CICS EIBRESP name/code authority. Inventory
gap: family schema accepts any nonnegative RESP without pairing its condition;
current validator does not inspect response pairs. Current18 families contain
2700 response clauses/39 condition names, all already matching common authority.
Manager owns xtask/src/cics_system_families.rs, unique changelog/status/docs and
actual validation; two domain CLI authors own external candidates only. Reuse
CICS_APPLICATION_CONDITION_NAMES/AUTHORITY_SHA256 and the existing generated
application contract's121 name/code records; no second committed response table
or runtime mapper. Exact primary dfhp4_eibfields.html, pin a342c35cc115f34f07c0f596f24dc203f3e51e2ec765da5954e4c4c0f3eed8e7,
baseline ibm-cics-ts-6x-application-api-sources-a-2026-09-10, table dfhp4au__table_nt5_kw4_b1c,
parser777-1056: EIBRESP numbers identify condition; RESP2 is command-specific.
Reject known-name/wrong-code and unknown aliases only in declared response rows;
no attempt to resolve unknown case-only NOTFOUND clauses, choose precedence,
normalize RESP2 null to zero or infer recovery/atomicity/response success.
Preserve all family/source/schema/generator/IR/runtime authorities and every
existing2700 response clause. Different-thread review plus negative regressions,
all-family/focused/mandatory policy/shape/docs/module gates precede serial child
seal. All six per-command/parent/application/selected-route/recovery/restart/
licensed gates remain Pending0; current305 grammar facts remain private/nonrouting.

### Pre-dispatch condition-response-identity independent review

Independent reviewer of manager-owned SPI-1001.condition-response-identity,
base sealed65. All305 exact family rows/2700 response clauses in scope for a
read-only gate review; no family mutation or inferred execution obligation.
Dependencies: existing common121-condition EIBRESP authority and verified
application sources-a pin; application execution acceptance remains Pending.
Reviewer owns only external condition-response-identity-review reports, uses
read-only isolated checkout and frozen original/candidate xtask full files.
Manager solely owns xtask/shared authorities/status/fragment/generated docs.
Check exact common source pin/table/hash, digest domain/serialization/order,
unknown aliases, wrong RESP, null/dynamic RESP2 preservation, negative regression
expectations and all-family compatibility. Reviewer independently consults exact
pinned EIBRESP search/read offline; no source refresh/runtime mapper/worker spawn.
Focused20 source-gate regressions, actual18-family instances and mandatory gates
plus independent findings resolution precede serial child seal. Source-only
credit0; all parent/application/selected-route/recovery/licensed Pending.

Manager condition gate build prerequisite: mainframe-env-ir is currently only
an xtask dev-dependency. Manager also owns xtask/Cargo.toml for moving that
existing direct workspace dependency to normal dependencies, enabling the
production xtask source validator to reuse its exported common constants.
No new crate, dependency version, lock entry, response table or runtime mapper.
Independent first review frozen code remains unchanged; final Cargo manifest
preimage/diff is separately retained for final independent review before seal.

### Pre-dispatch fepi-pool-resource-cvda-domains

Bounded source AUTHOR SPI-1001.fepi-pool-resource-cvda-domains, only existing families fepi-pool, fepi-pool-list, fepi-resources, exact original identities in declaration. Demonstrated gap: zero parent finite symbolic CVDA domains across 20 mapped FEPI rows. Only add optional cvda_domains arrays in existing grammar/forms when OWN exact pinned command option paragraph/raw syntax defines a finite closed symbolic list and existing declared operand role/direction/extent is resolved. Domain keys option/values/source_lines only; no numeric encodings/bytes/ABI/bindings, new forms/options/cases, deleted/rewritten existing properties or gaps. May append precise Pending gap strings. Preserve every original complete case/ID/order, operand, response, form, obligation, source pin and existing grammar exactly. No shared/global symbols or command-to-command extrapolation. Preserve FEPI property/event/target/node/session/conversation distinctions, ignored properties, record/buffer/cursor modes, install/delete/free/resource association and reserved/application/service-level scope; source contradictions and optional outputs remain explicit. Read entire membership branches and relevant defaults/exclusions/opposition/conditions/lifecycle/syncpoint/timeout/cancel/recovery qualifiers from source before candidate comparison. Your retained thread may have authored source earlier; you are AUTHOR, never independent approval. All repository files/index/historical artifacts/sharedcache read-only; own external output /Users/tore/.codex/worker-runs/v010-20261002/fepi-pool-resource-cvda-domains exclusively. Produce complete candidate(s) named family.json, full/delta diff, exact new property/member/qualifier/gap ledger, every argv/exit/hash/pin/parser-range and whole-byte forward/reverse preservation proofs, all old complete cases unchanged. Fresh offline search AND read each changed exact pinned topic. Matching HTML stays external; no network/refresh. Do not rerun unrelated historical whole-source campaign. Different-thread independent review + manager actual family/projection/focused/mandatory gates before bounded child seal. All six runtime gates/parent/application/recovery/license Pending0; no routing/behavior credit.
Exact rows: ibm-cics-ts-6x-2026-08-31:fepi-commands:0001, ibm-cics-ts-6x-2026-08-31:fepi-commands:0007, ibm-cics-ts-6x-2026-08-31:fepi-commands:0009, ibm-cics-ts-6x-2026-08-31:fepi-commands:0018, ibm-cics-ts-6x-2026-08-31:fepi-commands:0021, ibm-cics-ts-6x-2026-08-31:fepi-commands:0034, ibm-cics-ts-6x-2026-08-31:fepi-commands:0035, ibm-cics-ts-6x-2026-08-31:fepi-commands:0008, ibm-cics-ts-6x-2026-08-31:fepi-commands:0010, ibm-cics-ts-6x-2026-08-31:fepi-commands:0011, ibm-cics-ts-6x-2026-08-31:fepi-commands:0017, ibm-cics-ts-6x-2026-08-31:fepi-commands:0019, ibm-cics-ts-6x-2026-08-31:fepi-commands:0020, ibm-cics-ts-6x-2026-08-31:fepi-commands:0022, ibm-cics-ts-6x-2026-08-31:fepi-commands:0023, ibm-cics-ts-6x-2026-08-31:fepi-commands:0024, ibm-cics-ts-6x-2026-08-31:fepi-commands:0032, ibm-cics-ts-6x-2026-08-31:fepi-commands:0033, ibm-cics-ts-6x-2026-08-31:fepi-commands:0036, ibm-cics-ts-6x-2026-08-31:fepi-commands:0037.
Dependencies: sealed305-command source projection and reviewed original FEPI contracts/schema. Worker owns external candidate/report folder only; manager solely owns sharedschema/generator/IR/facades/status/security/UOW/runtime and serial integration. Gates: independent review, preservation, actual affected family instance, projection/focused/mandatory; source0credit.

### Pre-dispatch fepi-session-cvda-domains

Bounded source AUTHOR SPI-1001.fepi-session-cvda-domains, only existing families fepi-session-data, exact original identities in declaration. Demonstrated gap: zero parent finite symbolic CVDA domains across 19 mapped FEPI rows. Only add optional cvda_domains arrays in existing grammar/forms when OWN exact pinned command option paragraph/raw syntax defines a finite closed symbolic list and existing declared operand role/direction/extent is resolved. Domain keys option/values/source_lines only; no numeric encodings/bytes/ABI/bindings, new forms/options/cases, deleted/rewritten existing properties or gaps. May append precise Pending gap strings. Preserve every original complete case/ID/order, operand, response, form, obligation, source pin and existing grammar exactly. No shared/global symbols or command-to-command extrapolation. Preserve FEPI property/event/target/node/session/conversation distinctions, ignored properties, record/buffer/cursor modes, install/delete/free/resource association and reserved/application/service-level scope; source contradictions and optional outputs remain explicit. Read entire membership branches and relevant defaults/exclusions/opposition/conditions/lifecycle/syncpoint/timeout/cancel/recovery qualifiers from source before candidate comparison. Your retained thread may have authored source earlier; you are AUTHOR, never independent approval. All repository files/index/historical artifacts/sharedcache read-only; own external output /Users/tore/.codex/worker-runs/v010-20261002/fepi-session-cvda-domains exclusively. Produce complete candidate(s) named family.json, full/delta diff, exact new property/member/qualifier/gap ledger, every argv/exit/hash/pin/parser-range and whole-byte forward/reverse preservation proofs, all old complete cases unchanged. Fresh offline search AND read each changed exact pinned topic. Matching HTML stays external; no network/refresh. Do not rerun unrelated historical whole-source campaign. Different-thread independent review + manager actual family/projection/focused/mandatory gates before bounded child seal. All six runtime gates/parent/application/recovery/license Pending0; no routing/behavior credit.
Exact rows: ibm-cics-ts-6x-2026-08-31:fepi-commands:0002, ibm-cics-ts-6x-2026-08-31:fepi-commands:0003, ibm-cics-ts-6x-2026-08-31:fepi-commands:0004, ibm-cics-ts-6x-2026-08-31:fepi-commands:0005, ibm-cics-ts-6x-2026-08-31:fepi-commands:0006, ibm-cics-ts-6x-2026-08-31:fepi-commands:0012, ibm-cics-ts-6x-2026-08-31:fepi-commands:0013, ibm-cics-ts-6x-2026-08-31:fepi-commands:0014, ibm-cics-ts-6x-2026-08-31:fepi-commands:0015, ibm-cics-ts-6x-2026-08-31:fepi-commands:0016, ibm-cics-ts-6x-2026-08-31:fepi-commands:0025, ibm-cics-ts-6x-2026-08-31:fepi-commands:0026, ibm-cics-ts-6x-2026-08-31:fepi-commands:0027, ibm-cics-ts-6x-2026-08-31:fepi-commands:0028, ibm-cics-ts-6x-2026-08-31:fepi-commands:0029, ibm-cics-ts-6x-2026-08-31:fepi-commands:0030, ibm-cics-ts-6x-2026-08-31:fepi-commands:0031, ibm-cics-ts-6x-2026-08-31:fepi-commands:0038, ibm-cics-ts-6x-2026-08-31:fepi-commands:0039.
Dependencies: sealed305-command source projection and reviewed original FEPI contracts/schema. Worker owns external candidate/report folder only; manager solely owns sharedschema/generator/IR/facades/status/security/UOW/runtime and serial integration. Gates: independent review, preservation, actual affected family instance, projection/focused/mandatory; source0credit.

Condition gate architecture repair: the production IR dependency must be
declared in the same additive dependency graph authority checked against actual
Cargo metadata. Manager additionally owns conformance/0.10/inventory/dependency-additions.json,
its enrollment in xtask/src/main.rs, and ADR0030 plus the ADR index. Declare only
xtask -> mainframe-env-ir; no profile/package additions or layer exceptions.
Normal source validation must use the existing shared compiled authority rather
than a new copy or test-only import. Architecture gate first diagnosed the exact
missing edge and cleaned target; retry only after this concrete repair. Final
independent review includes all dependency/inventory/ADR deltas. Runtime remains
private/nonrouting and every acceptance obligation Pending0.

### Pre-dispatch spi-queue-cvda-domains independent review

Independent SOURCE REVIEW SPI-1001.spi-queue-cvda-domains: exact30 rows and only77 actual additions (54 parent domains155 members,50 existing-form domains143 members,37 gaps) from stopped source-author thread01a0fd23-a3ae-7732-aa5c-78e740726af8. Frozen final SHA310db3cc9ff1bd1c773b43d5962105a7bcb5b114a0c8a5e306685e24ea62c2a6; originala7a4a7a270cf4a768d46fba275c253cf06598445b809157396ce0a450e5e4ecc. All337 operands204 responses165 obligations24 forms903 wholecases491 originalgaps/IDs/order must remain exact. Review EVERY actual new array/value/member/source citation and EVERY full gap/qualifier; no sampling/digest-only expectation or using cases as oracle. Derive each finite closed list from own primary option paragraphs/rawsyntax before candidate comparison. Independently verify spelling/alias/browse/read/write/direction/extent/defaults/ignored/service/syncpoint exceptions and pending source oppositions, including CFDT vsTSPOOL,NOTRECOVABLE vsNOTRECOVERABLE,SMF/SHARE/SHR,queryvsSET transient states,SYSOUTCLASS conflict and ENQ/UOWENQ explicitsynonymjoin. Different retained thread01a0fd19-00cc-74b3-87c6-df1af0580650 may reuse unchanged historical review ONLY after exact currentinput compatibility; historicalcandidate3f4edc8e is not currentinput. All previous findings remain reported with exactdispositions. Dependency: sealed305-command source projection + unchangedschema; read-only independent checkout. Own only external /Users/tore/.codex/worker-runs/v010-20261002/spi-queue-cvda-domains-review reports, no candidate editing/repository/index/sharedcache writes. Manager owns status/sharedschemas/generator/IR/runtime and serial integration. Actual manager family/projection/focused/mandatory gates and independent findings resolution before bounded seal; all six behavioral/parent/application/recovery/licensed Pending0.
Exact rows: ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0011, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0014, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0017, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0029, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0033, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0071, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0074, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0075, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0086, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0090, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0107, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0117, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0118, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0132, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0133, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0134, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0162, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0170, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0171, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0179, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0180, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0181, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0182, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0219, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0228, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0229, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0250, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0251, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0259, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0260.

ADR0030 registration repair: manager also owns docs/documentation-registry.json
and its generated docs/README.md navigation. Docs gate requires every numbered
ADR to be explicitly registered; retain metadata Scope and canonical navigation
through the existing docs generator. This only documents the same source-tool
IR dependency edge, without runtime/profile authority changes.

### Pre-dispatch condition-response-identity final repair review

Retained independent reviewer01a0fdea-71a3-7323-a602-468bea1dc88d will inspect
only final dependency repair: xtask/Cargo.toml promotion, single graph enrollment
in xtask/src/main.rs, v0.10 dependency-additions edge, ADR0030/index and docs
registry/navigation. Existing reviewed validator remains SHA1dd56fc566a1ed3879563e4496f8b1aee0542d6d691ba05ae2a3ec8eba78cdbc;
all121 source-authority records and2700 response clauses/family/schema pins
unchanged. Reuse exact first independent source proof after equality checks; no
repeat source campaign. Own external condition-response-identity-final-review
reports only in read-only isolated base65 checkout; manager solely owns all
shared paths/status and serial seal. Acceptance: close exact production-import
finding, preserve historical first report, no new edge/profile/exception or
response mapper; actual focused/family/architecture/docs/policy gates and final
findings resolution precede bounded child seal. All runtime/parent/dependency/
recovery/licensed credit stays0 and Pending.

### Pre-dispatch spi-monitoring-cvda-domains independent review

Independent SOURCE REVIEW SPI-1001.spi-monitoring-cvda-domains: exact21 rows and only69 domain additions210 members in18 parentgrammars plus9 full Pendinggapstrings from stopped sourceauthor01a0fc9f-0147-73c2-b9de-d14b4632d7fd. Frozen finalSHA4556fc21e25577f61f3a1086c23d230a9038688c384571ebf4e82123f14bc7e8, originalfd707eff3d2cd6f6166ccfcf45ab4582d4b320242a5fc5bd1e4c385b639286f5. All635 operands142 responses101 obligations768 wholecases210 oldgaps/formabsence/IDs/order remain exact. Review EVERY new actual domain/member/citation and each fullgap/qualifier, no sampling or hashes replacing actual source reads. Derive finite lists from own primary option paragraphs/rawsyntax before candidatecomparison. Verify declared direction/extents,queryvsupdate memberships (NOTAPPLIC vsTCEXITALLOFF andDUMPDS SWITCH),STATISTICS RECORDING unresolveddirection excluded,TRACETYPE bitstrings,OTEL6.3betaqualification,component/service scope,defaults/count/dump persistence conflicts. Preserve two earlier exact response repairs and allnumericresponses/cases unchanged. Read sourceauthor fullhandoff.json/md and ALL linked actual ledgers/proofs/diffs/findings and source receipts; verify each hash. Independent retained reviewer01a0fced-45da-7442-b3d2-82ace535b067 never authored these sourcechanges. Priorreview reusedonlyunchanged hashes afterfull compatibility. Dependencies sealed305 sourceprojection/actualreviewedfamily/schema. Own external /Users/tore/.codex/worker-runs/v010-20261002/spi-monitoring-cvda-domains-review reports only; read-only isolated checkout/index/sharedcache and author artifacts. Manager owns sharedschema/generator/IR/status/runtime andactual family/projection/focused/mandatorygates plus serial childseal. Independent findings resolvedbeforeseal. Every behavioral/parent/application/recovery/licensed Pending0; sourcecontract notexecution.
Exactrows: ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0002, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0095, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0116, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0139, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0148, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0159, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0164, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0173, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0174, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0175, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0177, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0195, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0218, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0234, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0238, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0243, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0244, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0253, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0254, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0255, ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0257.

### Pre-dispatch fepi-pool-resource-cvda-extents

SOURCE AUTHOR bounded SPI-1001.fepi-pool-resource-cvda-extents, exact20rows/3existingfamilies fepi-pool,fepi-pool-list,fepi-resources. Previous finite-domain task stoppedterminal with40owncommand lists125members blockedsolelynull extents. New scope authorizes source-backed changes ONLY source_max_value_bytes null->4 for existing valued CVDA operands whose own pinned option/syntax identifies CVDA and existing direction/shape is settled; no nonCVDA/inferreddirection/length/ABI/numeric facet. Option/source_lines may only append exact ownbody lines identifying thatCVDA when needed; retain oldanchors/allotheroptionproperties. Add existinggrammar cvda_domains option/values/source_lines only fromOWNclosedfinite list, plusprecisegapstrings. CommonCVDA primary SSJL4D_6.x/system-programming/intro/dfha80x.html, pin81f101e030365400b431ecf68250dfcabc5673e1acbf05010c9285bf590e3b25, sources-b baselineibm-cics-ts-6x-application-api-sources-b-2026-09-10; parser7-15 distinguishes senderdata-value/receiverdata-area and fullwordbinary representation; do not equate language C long/ABI/alignment/endian/numericencoding tothissourceconstraint. Independently verify applicabilitytoFEPI and every exact ownCVDA designation; ifunsettled stayPending. Commonargumentvalues authority mayclarifyfullword butmustalreadybehashpinned; nofetch. Manager alreadyfreshsearched/read entire40lineCVDA primary andmatching SHAarchive; worker freshsearch/readrequired before changes. New inventory demonstration40null declaredCVDAextents is concrete prerequisite repair, not permissiontoguessextents or relaxvalidator. Original committed row/operands/forms/responses/obligations/all362wholecases/IDs/order/sourcepins/175gaps remainexact apart authorized extentfield/citationappend/domainarrays/newprecisegaps. Prior15gap-only UNSEALED prototype is superseded; it neverenteredGit andshould not force keeping newlymisleading extraPendingextent claims. Begin from exactroot frozenoriginals; accountprototype differences andall originalgapvalues. Prior source/list/qualifier observations areprovenance only, not independentapproval; reuseunchanged exactbytes afterequality andfreshconsult relevantcommon/extent paragraphs. QueryvsSET/INSTALL states,DEVICETYPE/FORMAT/unsolicited/journal/event/TDqualifiedbehavior, zero-count andRESP2174vs176oppositions remain precise. No newcommand/options/forms/cases, registry/schema/generator/IR/facade/resource/security/UOW/runtime/cache modifications. External candidate/reports/Users/tore/.codex/worker-runs/v010-20261002/fepi-pool-resource-cvda-extents only; repository/indexread-only. Manager soleowner sharedtypes/schema/generator/IR/status/allactual integrationgates andserial boundedseal. Different-thread independent reviewofextents/domains plusfocused/projection/actualfamily/mandatorygates precedeacceptance. Every sixbehavioral/parent/application/recovery/licensed Pending0; noABI/runtime mapper or routingauthority. Produce full/delta/property/extent+member+qualifier+gapledgers, actualargv/exits/hash/pin/parserrange receipts andwholebyteforward/reverse allunchangedcase proofs. Stop concreteboundedhandoff, no redundantoldfullsourcecampaign.
Exactrows: ibm-cics-ts-6x-2026-08-31:fepi-commands:0001, ibm-cics-ts-6x-2026-08-31:fepi-commands:0007, ibm-cics-ts-6x-2026-08-31:fepi-commands:0009, ibm-cics-ts-6x-2026-08-31:fepi-commands:0018, ibm-cics-ts-6x-2026-08-31:fepi-commands:0021, ibm-cics-ts-6x-2026-08-31:fepi-commands:0034, ibm-cics-ts-6x-2026-08-31:fepi-commands:0035, ibm-cics-ts-6x-2026-08-31:fepi-commands:0008, ibm-cics-ts-6x-2026-08-31:fepi-commands:0010, ibm-cics-ts-6x-2026-08-31:fepi-commands:0011, ibm-cics-ts-6x-2026-08-31:fepi-commands:0017, ibm-cics-ts-6x-2026-08-31:fepi-commands:0019, ibm-cics-ts-6x-2026-08-31:fepi-commands:0020, ibm-cics-ts-6x-2026-08-31:fepi-commands:0022, ibm-cics-ts-6x-2026-08-31:fepi-commands:0023, ibm-cics-ts-6x-2026-08-31:fepi-commands:0024, ibm-cics-ts-6x-2026-08-31:fepi-commands:0032, ibm-cics-ts-6x-2026-08-31:fepi-commands:0033, ibm-cics-ts-6x-2026-08-31:fepi-commands:0036, ibm-cics-ts-6x-2026-08-31:fepi-commands:0037.
Dependencies: sealed66 commonconditiongate/305sourceprojection, existing reviewedFEPIinputs/schema and exact common/ownCVDApins. Worker owns external3familycandidate/reportfolder only. All source-only; manager/integration/readiness boundaries unchanged.

### Pre-dispatch fepi-session-cvda-domains independent review

Independent source review SPI-1001.fepi-session-cvda-domains: exact19 rows,25 property additions:26 parent domains106 members and12 existing-form domains72 members,11 full Pending gaps. Frozen candidate SHA cfffa839d84ae4d2e2e69b8dc93e098fe91621b334007f482425d4c4f004c467, original e14d05be1a07e5b9f2ca035bc7a126c1c66a6a0c6be3407984908f2d3e4881d9. All222 operands380 response records,five forms,779 complete cases/obligations,272 original gaps/IDs/order remain exact; verify actual array counts rather than inherited wording. Derive EACH closed finite list from its own primary option paragraph/raw syntax before comparison. Read EVERY actual new property/value/member/citation and every complete gap/qualifier; no sampling or case oracle. Preserve ISSUE mode/control union versus cross-product,temporary/allocated CONVERSE ending distinctions,RECEIVE FORMATTED prose/table opposition,device extraction versus installable property capability,optional receivers/task ownership and request versus attained state. Check four-byte valued operands already declared; no extent change in this slice. Source author thread01a0fcfc-90b7-7092-ae58-8ad026a30531 stoppedterminal0; independent reviewer01a0fcfc-90b7-7c90-b85e-466200fbded2 is different and must not claim authorship approval. Prior full source/review provenance reused only after exact input/source/report/receipt hashes; all prior findings retain dispositions. Read all complete handoff.md/json,artifact-hashes.json,25-addition ledger/member/qualifier/gap/full-case inventories,source-coverage-final.json/source-receipts.json/source-linkage-proof.json,raw retained SVG/table receipts,preservation-proof.json,both exactdifferences and findings/deviations. Verify every linked artifact hash. Full diff is external standalone candidate snapshot,delta modifies existing file; whole forward/reverse both must reproduce original/current bytes with type/key/array/ID order. Dependencies: sealed66 commonconditiongate and305sourceprojection, original reviewed FEPI family and unchangedschema; candidate author base65 compatible only after exact family/pin/schema proof. Reviewer owns only external /Users/tore/.codex/worker-runs/v010-20261002/fepi-session-cvda-domains-review reports; isolated repository/index/cache/author artifacts read-only. Manager soleowner sharedtypes/schema/generator/IR/facades/status/runtime and actual family/projection/focused/mandatorygates,serial childseal. Independent findings resolved before sourcechild acceptance; every behavioral/parent/application/recovery/licensed Pending0. No numeric encoding,ABI, new field/form/case or source repin. Fresh offline search/read each exactchangedtopic and decisivecontext, bounded≤200pages; do not repeat unrelated unchanged fullcampaign. Write findings.json actionable_findings plus full actual member/qualifier dispositions and handoff.md exacthashes/source receipts/preservation/gate boundaries. No selfrepair/build/test/nestedworker. Stop concrete review.
Exact rows: ibm-cics-ts-6x-2026-08-31:fepi-commands:0002, ibm-cics-ts-6x-2026-08-31:fepi-commands:0003, ibm-cics-ts-6x-2026-08-31:fepi-commands:0004, ibm-cics-ts-6x-2026-08-31:fepi-commands:0005, ibm-cics-ts-6x-2026-08-31:fepi-commands:0006, ibm-cics-ts-6x-2026-08-31:fepi-commands:0012, ibm-cics-ts-6x-2026-08-31:fepi-commands:0013, ibm-cics-ts-6x-2026-08-31:fepi-commands:0014, ibm-cics-ts-6x-2026-08-31:fepi-commands:0015, ibm-cics-ts-6x-2026-08-31:fepi-commands:0016, ibm-cics-ts-6x-2026-08-31:fepi-commands:0025, ibm-cics-ts-6x-2026-08-31:fepi-commands:0026, ibm-cics-ts-6x-2026-08-31:fepi-commands:0027, ibm-cics-ts-6x-2026-08-31:fepi-commands:0028, ibm-cics-ts-6x-2026-08-31:fepi-commands:0029, ibm-cics-ts-6x-2026-08-31:fepi-commands:0030, ibm-cics-ts-6x-2026-08-31:fepi-commands:0031, ibm-cics-ts-6x-2026-08-31:fepi-commands:0038, ibm-cics-ts-6x-2026-08-31:fepi-commands:0039.

### Pre-dispatch FEPI pool/resource CVDA extent independent review

SPI-1001.fepi-pool-resource-cvda-extents-review independent source review, exact20 rows in fepi-pool,fepi-pool-list,fepi-resources. 70 actual operations:40 source_max_value_bytes null-to-4 replacements,15 parent domain arrays containing40 domains125 members,15 full Pending gaps. No citation appends. Derive every extent/CVDA applicability/role and every literal finite member from own pinned source BEFORE comparing candidate; read ALL70 complete changed properties, all qualifiers and15 gaps, not a sample. Review three-part extent join: common dfha80x fullword receiver/direction; FEPI SPI overview dfhp7k4 explicit applicability; own40cvda headers/roles, plus argumentvalues FIXED BIN(31). Verify four-byte projection without host C long ABI/alignment/endian or numeric encodings. Source author01a0fc9f-0147-7e13-b6c1-05a525997e0d is different from retained independent reviewer01a0fced-45da-7442-b3d2-82ace535b067. Author terminal0; no independent selfapproval. Frozen original hashes {'conformance/0.10/cics/families/fepi-pool.json': '1b665af69a1318821aaf84c33cee693a4cf01864cb3c571c388f0e5b652e7af2', 'conformance/0.10/cics/families/fepi-pool-list.json': '65c2950c331dc8f09ccb2dc991cf46f666cad881d44c5b34a1c735e495deb7da', 'conformance/0.10/cics/families/fepi-resources.json': 'e3c6d369d1cfbc226df10d5a983c8a5da1988de324e02b39a6ea83337934a6a6'}; finalhashes {'conformance/0.10/cics/families/fepi-pool.json': '44c5a1191b27085591e2f3b85bdf45a3494cc6f60648d16998ad09a9fdf92f6e', 'conformance/0.10/cics/families/fepi-pool-list.json': '7fdbda7dd3fd988c1ad5c8a468041ec966a014e2c9d7e3addbe487b93ad14ac7', 'conformance/0.10/cics/families/fepi-resources.json': 'f95b445843b11580c76542d2fc3deef82a00ff228c8fd81226b8597cf094f6d4'}; schema719edd27ae0b6bcc3fba82689b6f9c4f39f815c4bcdad45ceac2aabdfaf273eb unchanged. All20rows255operandobjects306responses91obligations362wholecases175originalgaps/IDs/types/key/arrayorder exact except40explicit extents;215complete operands exact. No forms. Verify whole-byte forward/reverse BOTHdiffs and actual recursive/span replay, all artifactreferences. Read full author handoff.md/json,all complete ledgers/source/provenance/hash/deviation/preservation receipts. Prior15-gap-only prototype never integrated; removing only15prototype gaps restores original; prototype/provenance remain immutable and superseded, original175gaps preserved. Preserve NODE174 versus inherited176 opposition,ADD/INSTALLzerocount,listpartialsuccess versus general no-change wording; queryGOINGOUT/ACQUIRING/RELEASING/NOTAPPLIC not senderstates,device/SLU/format/contention/initialdata/journal/unsoliciteddata servicequalifiers. No acquiredstate/callerrollback/atomiclist/redispatch implication. Fresh offline search/read exact changedownprimaries and necessary context, ≤200parserlines/page; retain sourcepinproductbaselinehash. Reuse unchanged fullbody/provenance only after exact hashes; don't rerun unrelated wholecampaign. Repository/index/cache/author/history READONLY, external /Users/tore/.codex/worker-runs/v010-20261002/fepi-pool-resource-cvda-extents-review only owned. Manager alone owns status,shared schema/types/generator/IR/facades/security/UOW/runtime and serial integration; reviewer no repair/build/test/lint/repositorygenerator/stage/commit/seal/install/network/browser/license/nestedworker. Dependencies sealed66 commonconditiongate,305privateprojection,three exact reviewed originalfamilies. Sourcefamily independent findings resolved before manager actual Rust Draft202012/projection/focused/mandatory childgates. Every behavioral/parent/application/selectedroute/recovery/restart/licensed gate staysPending0. Write actionable_findings plus complete extent/member/qualifier/fullgapdispositions and handoff exactscope/hashes/source receipts/preservation/checks. Stop concrete bounded review.
Exact rows: ibm-cics-ts-6x-2026-08-31:fepi-commands:0001, ibm-cics-ts-6x-2026-08-31:fepi-commands:0007, ibm-cics-ts-6x-2026-08-31:fepi-commands:0009, ibm-cics-ts-6x-2026-08-31:fepi-commands:0018, ibm-cics-ts-6x-2026-08-31:fepi-commands:0021, ibm-cics-ts-6x-2026-08-31:fepi-commands:0034, ibm-cics-ts-6x-2026-08-31:fepi-commands:0035, ibm-cics-ts-6x-2026-08-31:fepi-commands:0008, ibm-cics-ts-6x-2026-08-31:fepi-commands:0010, ibm-cics-ts-6x-2026-08-31:fepi-commands:0011, ibm-cics-ts-6x-2026-08-31:fepi-commands:0017, ibm-cics-ts-6x-2026-08-31:fepi-commands:0019, ibm-cics-ts-6x-2026-08-31:fepi-commands:0020, ibm-cics-ts-6x-2026-08-31:fepi-commands:0022, ibm-cics-ts-6x-2026-08-31:fepi-commands:0023, ibm-cics-ts-6x-2026-08-31:fepi-commands:0024, ibm-cics-ts-6x-2026-08-31:fepi-commands:0032, ibm-cics-ts-6x-2026-08-31:fepi-commands:0033, ibm-cics-ts-6x-2026-08-31:fepi-commands:0036, ibm-cics-ts-6x-2026-08-31:fepi-commands:0037.

QUEUE integration child SPI-1001.spi-queue-cvda-domains is manager-owned after
independent review of exactly 77 additions: 21 parent arrays and 19 existing-form
arrays with 54 parent domains/155 members and 50 form domains/143 members, plus
37 precise Pending gaps. All 30 rows, 337 operands, 204 responses, 165 obligations,
24 forms, 903 whole cases and 491 original gaps retain exact values/types/IDs/order.
Manager read all 77 complete additions and all 54 parent qualifier records;
whole-byte forward/reverse replay and 62 current handoff artifact references
match. Fresh manager offline consultation used 46 successful search/read calls,
21 pinned topics and 25 actual parser pages/2250 lines, all read completely.
QCDR-01 external report accounting is resolved in a new immutable-history erratum:
the current author identity ledger has 35 records; the historical full campaign
has 37 topics; FORMATTIME and TRANSACTION belong only to that historical campaign.
The independently verified source candidate is unchanged by this report correction.
Primary baseline ibm-cics-ts-6x-spi-command-bodies-2026-09-12; common dfha80x uses
ibm-cics-ts-6x-application-api-sources-b-2026-09-10. Exact catalog rows/topic hashes
stay in spi-queue-storage.json. ENQ uses the explicit primary synonym join to
UOWENQ, not name/EIBFN equality. CFDTPOOL and TSPOOL states, NOTRECOVABLE and
NOTRECOVERABLE, SMF and SHARE remain distinct; CEDA SHR adds no alias. Inquiry
transient states add no SET request. SYSOUTCLASS syntax/prose, TSPOOL direction,
null receiver authority, ENQ filter omission, SET ENQMODEL STATE wording, TD
remote/indirect and disable/ATI oppositions, expiry/PUTQ/truncation and browse
limits remain precise Pending gaps. CREATE LOG/NOLOG supplies no omitted default
or durable audit; task-work implicit syncpoint retains early-exception scope.
LSR replacement keyed by LSRPOOLNUM applies at next build after all files close
and a file reopens, without immediate attainment or installed-definition rollback.
Journal effects and recoverable TS deletion stay asynchronous/qualified. No
numeric encoding, ABI, receiver geometry, operand/form/case or runtime route is
added. Manager solely owns shared schema/types/generator/IR/status and serial
integration. Only this bounded source child seals after actual instance,
projection, focused and mandatory gates. Parent/application/route/recovery/
restart/licensed and all six command gates remain Pending, credit0. Three
v0.9 rows 0027/0093/0114 remain catalogued and deferred by user scope.

QUEUE domain integrated candidate passes 31 generator and nine IR regressions,
actual Draft202012 queue instance and all 18 actual family instances, including
the shared condition-name/RESP authority. Private 305-command projection
(266 SPI and 39 FEPI), 5082 operands/8947 case candidates and all 62 numeric
domains/217 numeric records remain unchanged. MONITOR and FEPI source children
retain their own independent review/integration gates; runtime acceptance pending.

MONITOR integration child SPI-1001.spi-monitoring-cvda-domains is manager-owned
after different-thread independent review of exactly 27 additions: 18 parent
arrays containing 69 symbolic domains/210 members and nine precise Pending gaps.
All 21 rows, 635 operands, 142 response records, 101 obligations, 768 whole cases,
210 original gaps and form absence remain exact, including both prior response
repairs, IDs/types/key/array order. Manager independently proved whole-byte
forward/reverse replay for both diffs and all 81 current handoff artifact references;
read every complete added field/value/citation/gap and all 18 full qualifiers.
Fresh offline consultation used 46 successful search/read calls on 20 exact
pinned topics, with all 26 actual parser pages/3236 lines completely read. The
unchanged common dfha80x fullword/direction source and exact manager receipt were
reused after hash verification; the earlier read is not relabeled as current
execution evidence. Primary baseline ibm-cics-ts-6x-spi-command-bodies-2026-09-12;
exact catalog rows/topic/hash pins stay in spi-monitoring-control.json. Common
CVDA context is sources-b and dump-default dfhs14a context is sources-a, both
2026-09-10. Source consultation supplies no licensed credit.
EXTRACT resource/global and private/public application selector combinations stay
Table1-qualified, including absent explicitly selected private resources without
fallback and CICS-owned reusable/freed statistics storage. Table1 NODEJSAPP versus
RESTYPE list omission and FEPI POOL/NODE condition opposition remain explicit old
gaps; no authoritative precedence is invented. INQUIRE STATISTICS RECORDING keeps
its unresolved mutating-versus-return role and gains no domain. OTEL is qualified
to 6.3/beta with transaction controls, inquiry invalid-CVDA opposition and enabling
span flush/loss; broader TS6.x applicability and delivery remain Pending. DUMPDS
inquiry OPEN/CLOSED and update SWITCH remain distinct. TRACEFLAG query NOTAPPLIC
and update TCEXITALLOFF retain different service scopes. TRACETYPE component bits
remain separate from FLAGSET symbolic values. Dump ADD defaults/count comparison,
temporary versus explicit restart persistence, catalog-error current-run partial
effects and shutdown loss retain exact source qualifications. Trace switching/
exception recording/GTF prerequisites, TABLESIZE requested/effective effects,
monitoring class accumulation/write/loss, QR/CO scope and syncpoint rollback limits
remain separate. No default, alias, numeric encoding, field/form/case, receiver
geometry, caller-UOW rollback, mutation or runtime route is added. Manager solely
owns schema/types/generator/IR/status/shared authorities and serial integration.
Only this bounded source child seals after actual family/projection/focused and
mandatory gates. Parent/application/selected-route/recovery/restart/licensed and
all six command gates remain Pending, credit0; three v0.9 identities remain
catalogued and deferred by user scope.

MONITOR integrated source candidate passes 31 generator and nine IR regressions,
actual Draft202012 monitoring instance and all 18 actual family instances,
including the common condition-name/RESP authority. Private 305-command projection
(266 SPI and 39 FEPI), 5082 operands/8947 case candidates and all 62 numeric
domains/217 numeric records remain unchanged. FEPI session and pool/resource
source children retain independent manager integration gates; runtime pending.

### Pre-dispatch application dependency evidence audit

SPI-1001.application-dependency-evidence-audit is a bounded read-only dependency audit, not implementation or semantic acceptance. Exact scope: cics.application-api prerequisite on sealed68 v010 integration candidate fa0709419d9848d5b4911e11e43f54d82222f317, fetched main213ed878ec138bdb2914330db6613559bffc5a86, existing263 catalog identities and260 typed baseline; user explicitly defers only CICSMESSAGE0027,GETNEXT TIMER0093,ISSUE COPY0114 and requests accelerated v010. Do not implement, delete, reclassify or quietly accept those three. Distinguish actual application integration/start gate from formal full application completion, licensed acceptance and merely typed advertised counts. Read current full application status/plan/prompt,system start/integration dependency contracts, DEPENDENCIES and applicable existing shared CICS/security/UOW/effect/recovery/condition/product-route acceptance requirements. Inspect actual current code and exact retained evidence by candidate/input hash, tests/receipts/selected-route/recovery/backend matrix; historical green or source receipts don't prove currentcandidate. Determine every concrete remaining prerequisite with path/line, criterion, actual proof, hash/candidate binding and honest missing status. Verify actual per-row mappings/readiness rather than counts. Audit which receipt is required to start private SPI/FEPI runtime work and what non-conflicting shared-contract work is independently dependency-ready. Identify smallest concrete implementation/test slice that moves whole v010 objective forward under existing owners, not a narrower terminal goal or invented generic-success/private shadow subsystem. Preserve required licensed differential Pending0 under user's instruction to omit unavailable licensed CICS; no license bypass/fabrication/newnotapplicable. Parent/all269SPI39FEPI remainPending. No newly accepted applicationclaim without evidence. Existing v010305privategrammar/runtime0 is source preparation, not execution. Review core actual runtime/shared owners and existing private named PROGRAM STATUS route if relevant, retain its limits rather than restart sealedwork. Report exact current versus historical input compatibility and concrete next work, not a status restatement or speculative large plan. This scope may read relevant pinned sources offline via search/read for any semantic recommendation; matching retained bodies only, norefresh/repin/browser/network/license. No whole-cache audit or new exploratory suite. Ownership repository/index/cache/history/current manager files READONLY; external /Users/tore/.codex/worker-runs/v010-20261002/application-dependency-evidence-audit only. Manager soleowner shared schemas/generator/IR/facades/status/security/UOW/runtime/integration. Dependencies sealed68 sourceprojection/commonconditiongate and three explicitly deferred identities; never equate source-child seals with accepted parent. No build/test/lint/repositorygenerator/stage/commit/seal/install or workerdelegation. Read full objective and newest frozen declaration/status; no overlapping manager MONITOR/FEPI source-domain proof investigation. Handoff requirements: dependency-requirements.json criterion-by-criterion evidence disposition, current-input-identities.json, actual-read/receipt identities, findings.json exact actionable blockers versus sourcequalifications, and concise handoff.md with concrete dependency-ready nextaction. Stop bounded audit.
Exact application selector: conformance/0.9/cics/command-descriptors.json SHA-256 af06985dba30c6fd7f8dab88886340b3b4fa88ad5b770ef287201b7e172ea948, application_catalog.commands[*].official_row (263 unique rows, unit api-commands, baseline ibm-cics-ts-6x-2026-08-31); the complete immutable row list and labels are frozen in /Users/tore/.codex/worker-runs/v010-20261002/application-dependency-evidence-audit/exact-row-scope.json. Deferred exact rows: ibm-cics-ts-6x-2026-08-31:api-commands:0027 CICSMESSAGE, ibm-cics-ts-6x-2026-08-31:api-commands:0093 GETNEXT TIMER, ibm-cics-ts-6x-2026-08-31:api-commands:0114 ISSUE COPY. All other 260 identity mappings remain within read-only audit scope.

FEPI session integration child SPI-1001.fepi-session-cvda-domains is manager-owned
after different-thread independent review of exactly 25 additions: 14 arrays
with 26 parent domains/106 members and 12 existing-form domains/72 members, plus
11 precise Pending gaps. All 19 rows, 222 parent operands, 380 response records,
five forms, 779 whole cases/obligation objects and 272 original gaps retain exact
values/types/key/array/ID order. Eight whole commands are unchanged. Whole-byte
forward/reverse delta replay and standalone full-snapshot reconstruction and
reversal are exact; all 493 current artifact references were hash-verified,
including repository blobs at declared author/reviewer bases. Manager consumed
all 25 complete additions and 30 full qualifier records, the 11 independent gap
reasons and eight retained FSR dispositions. Fresh manager offline consultation
used 25 successful search/read calls on 12 exact pinned topics, with all 13
actual pages/1460 parser lines read completely. The unchanged common dfha80x
fullword/direction source and exact prior read receipt were reused after hash
verification, without relabeling historical consultation as current execution.
Primary baseline ibm-cics-ts-6x-fepi-command-bodies-2026-09-12, ending context
ibm-cics-ts-6x-fepi-context-candidates-2026-09-12 dfhp74m hash
2f5c45518008e0f9b5e090fde7030a916ff4374d472d65225790ece0c64a3c60;
common CVDA uses sources-b 2026-09-10. Exact own catalog row/topic/hash identities
stay in fepi-session-data.json. Publication references provide no execution credit.
ISSUE CONTROL/VALUE lists remain mode/control-qualified unions, with omission
distinct from NONE and no Cartesian product or phase-precedence inference.
Allocated and temporary CONVERSE endings differ; the complete raw dfhp74m table
excludes LIC/RU for temporary POOL and MORE for formatted commands. Own RECEIVE
FORMATTED LIC/EB/CD completion versus context end-chain wording stays both-authority
Pending. EXTRACT DEVICE has its own twelve names without implying installable
property/device capabilities; FORMAT stays SLU2-qualified. Optional receiver
requests, current-buffer field geometry, caller-owned CONVID, sequence-number
set/test, scheduled versus owning tasks and pending acquire/service states remain
distinct. SET has only requested ACQUIRED/RELEASED and INSERVICE/OUTSERVICE;
omission retains the respective state. Immediate request return does not mean
achieved bind/unbind/drain. Existing conversations survive OUTSERVICE; unowned
and owned conversations release differently. Item-list bounds are not total
resource caps, partial list failure stays possible and source not-audited does
not discharge durable product audit. Prior response/case repairs remain exact;
unknown contexts/pins, numeric encoding/ABI, receiver storage, version/service
admission, precedence, cancellation, caller rollback and physical recovery stay
Pending. No operand/form/case, numeric binding, request handler or runtime route
is added. Manager solely owns schemas/types/generator/IR/status/shared authorities
and serialized integration. Only this bounded source child seals after actual
family/projection/focused/mandatory gates. Parent/application/selected route/
recovery/restart/licensed and all six command gates remain Pending, credit0.
The three user-deferred v0.9 rows0027/0093/0114 remain catalogued and Pending.
The application evidence audit declaration present before this integration is
preserved; its separate reviewer has no shared-path ownership.

FEPI session integrated source candidate passes 31 generator and nine IR
regressions, the actual Draft202012 FEPI-session instance and all 18 actual
family instances, including the common condition-name/RESP authority. Private
305-command projection (266 SPI and 39 FEPI), 5082 operands/8947 case candidates
and all 62 numeric domains/217 numeric records remain unchanged. FEPI pool/resource
extent/domain review and application dependency audit retain separate gates;
runtime and parent acceptance remain Pending.

FEPI pool/resource integration child SPI-1001.fepi-pool-resource-cvda-extents
is manager-owned after different-thread independent review of all 70 operations:
40 source-backed null-to-4 extent replacements, 15 parent arrays with 40 domains/
125 member occurrences and 15 exact Pending binding gaps. All 20 rows, 255
operands, 306 response objects, 91 obligations, 362 whole cases and 175 original
gaps retain exact values/types/key/array/ID order except the 40 authorized extent
scalars; the other 215 whole operand objects and every old citation are unchanged.
No option, direction, form, response or case is added. Manager independently
proved whole original/candidate forward/reverse bytes and actual full/delta patch
replay; all 102 current declared artifact references match hashes. Manager read
all 70 changed-property records, 40 own domain/qualifier records and 15 independent
gap dispositions. Fresh manager offline source consultation used 33 successful
search/read calls on 16 exact topics, with all 17 actual pages/1116 parser lines
read completely. Primary baseline ibm-cics-ts-6x-fepi-command-bodies-2026-09-12;
exact own catalog row/topic/hash identities stay in the three family inputs.
Extent authority joins common dfha80x sources-b 2026-09-10 hash
81f101e030365400b431ecf68250dfcabc5673e1acbf05010c9285bf590e3b25 parser7-15,
FEPI SPI applicability dfhp7k4 context-candidates 2026-09-12 hash
ceb549cbc97f724aab9d489ec464d708d45e487d38e6479703de507d2d726712 parser3-7,
and each own CVDA header and settled sender/receiver role. Complete argument-values
dfhp4_argumentvalues sources-b hash44e85f97788be382d70df61dd7059ba079f26a0da4dff8e40e31751cf3c68e70
corroborates fullword binary with FIXED BIN(31). Four-byte source projection does
not establish native C long width, ABI/alignment/endian/numeric encoding, aliases
or actual receiver allocation. All such bindings remain Pending.
Query GOINGOUT, ACQUIRING/RELEASING and NOTAPPLIC remain separate from SET/INSTALL
sender domains. DEVICE/SLU mode, FORMAT character attributes, CONTENTION,
INITIALDATA recommendation, journal direction, unsolicited-data acknowledgment
and optional outputs retain own paragraph qualifications. Handler/TD queue names
remain names, without invented finite domains or omitted defaults. Property-set
inquiry specified-pool wording remains ambiguous. Existing NODE body174/index176
OPEN ACB and ADD/INSTALL zero-count oppositions remain exact and Pending. General
FEPI error no-change wording does not override list partial successes, including
valid-pool installation with failed node lists. Requests can return before
attainment; event/TD loss, EXCEPTIONQ/CSZX routing, journal versus durable platform
audit, caller UOW/recovery and uncertain-outcome retry remain separate obligations.
The prior 15-gap-only external prototype was never integrated; exact root inputs
are the preimage, and only its extra blockers are superseded externally. Historical
prototype bodies/reports are immutable. No schema/guard is loosened, no shared
shadow authority or runtime route is added. Manager solely owns shared schema/
types/generator/IR/status and serialized integration. Only this bounded child
seals after actual affected instances/projection/focused/mandatory gates. All six
command gates, parent/application/route/recovery/restart/licensed acceptance stay
Pending, credit0. The three user-deferred v0.9 identities remain catalogued/Pending.

FEPI pool/resource integrated source candidate passes 31 generator and nine IR
regressions, all three actual affected Draft202012 family instances and all 18
actual family instances, including common condition-name/RESP authority. Private
305-command projection (266 SPI and39 FEPI),5082 operands/8947 case candidates
and all62 numeric domains/217 numeric records stay unchanged. Exactly40 source
extent scalars and40 scoped symbolic domains are added to the existing authority.
Application dependency audit retains its separate evidence gate; runtime and
parent acceptance remain Pending.

### SYNCPOINT dependency consumption and private PROGRAM preparation

Manager-owned child: `SPI-1001.application-syncpoint-contract-consumption`.
This bounded child consumes application row 0218 SYNCPOINT's frozen registration,
condition, participant context, canonical effect and UOW contracts through the
existing compiled pilot. It does not accept the full application API or enable
SPI routing. All 263 application identities stay intact; user-deferred unready
rows 0027 CICSMESSAGE, 0093 GETNEXT TIMER and 0114 ISSUE COPY stay Pending.
Licensed differentials stay Pending, with no licensed execution credit.

Before dispatch, the manager declared separate exact ownership: the CLI author
owns only the new private test module
`crates/tooling/mainframe-env-conformance/src/cics_pilot/tests/contract_consumption.rs`;
the manager owns its cfg(test) registration, shared compiler repair, status,
prepared contract, fragment, generated documentation and serialized integration.
No worker changes shared schemas, generators, providers, fixtures or runtime
facades. CLI workers use gpt-6.1-sol/high/default, fast mode off, no nested agents
and isolated worktrees. Independent review uses a different retained CLI thread.

The demonstrated consumption gap is current binding of row/descriptor/EIBFN,
compiled conditions and participant contexts to durable effect, UOW, audit and
physical SQLite reopen observations. Independent frozen participant/golden
fixtures supply expectations. The author retained a failing duplicate-ROLLBACK
regression: the existing parser coalesced repeated ROLLBACK despite row 0218's
frozen duplicate-option rejection. The manager excludes only ROLLBACK from bare
flag coalescing. The complete 263-row option inventory confines ROLLBACK to this
row; exact repeated NOHANDLE and every other compatibility rule remain intact.
This is a frozen product admission repair, not an IBM repeated-keyword claim.

Affected acceptance checks are the four new consumption regressions, existing
exact-flag/value-repeat/legacy-SPI controls and three affected participant,
unknown-outcome and compiled-pilot selectors. Manager integration also requires
module, public API, schema, format, architecture, documentation, changelog,
dependency-policy and bounded work-package seal checks. Worker-local receipts
cannot supply integrated-candidate evidence. No broad certification is implied.
The existing participant codec metadata and MQ context repairs remain intact;
new coordinator, dispatcher, evidence ledger or automatic redispatch is forbidden.

Source review precedes semantic changes. Exact application row 0218 sources are
baseline `ibm-cics-ts-6x-application-api-sources-c-2026-09-10`, topics
`SSJL4D_6.x/reference-applications/commands-api/dfhp4_syncpoint.html`
(SHA-256 `2e1bebaa9ac35c7444eeb63d2e15d1773a5d39e06f0e65f00970e96f411f9b34`,
parser 22–41) and `dfhp4_syncpointrollback.html`
(SHA-256 `566d8661a0af02559d8679234e959d2e2aa2577dcf14c55211ec07c7c03e2954`,
parser 22–24 and 46–51). Matching retained bodies were read offline through the
pinned search/read tool; no refresh, repin or publication text enters Git.

The separate named PROGRAM STATUS preparation is documented in
[the prepared contract](../../../contracts/CICS-NAMED-PROGRAM-STATUS-V1.md).
Its exact scope is SPI row 0155, trusted public/local/non-Java artifact-backed
cohort, existing ProgramControl/State observation, distinct future typed identity
and validated STATUS receiver. The existing helper is already sealed and is not
reimplemented. Different-thread independent review found no actionable error in
all 30 clauses after 14 successful source calls covering 211 parser lines. The
contract retains exact source pins and prerequisite ownership. Trusted issuer,
complete visible namespace, configured command/resource security activation,
resolved receivers, route binding and selected-backend evidence remain Pending.
No new operation tag, executable profile or advertised SPI route is assigned.

These are bounded dependency and private preparation children. PostgreSQL,
full application acceptance, all six SPI/FEPI command gates, restart acceptance,
licensed differentials and parent completion remain Pending with zero new
command coverage. Actual local test counts are not official row credit.

Module gate found the three-line test registration exceeds the existing facade's
frozen 1321-line production ceiling. Manager owns a structural correction only:
move the new module under the existing terminal test module at
`cics_pilot/tests/contract_consumption.rs` and register it inside `mod tests`.
Adjust only its two relative include paths and selector prefix. No ceiling,
inventory, fixture or behavior changes. Independent review must consume the
final registration/layout delta before seal.

The first integrated attempt executed ten regressions with no failure or
ignore: four contract-consumption tests, three exact-flag/duplicate-option/legacy
SPI controls and three participant/unknown-outcome/compiled-pilot controls. The
then-current tests exercise eight context forms on Memory and SQLite, eight physical
SQLite reopens and two golden commit/backout record reopens. They bind typed
registration and EIBFN, independent response/condition fields, canonical effect
identity/result, UOW owner/state/codec and mandatory audit/lifecycle observations.
Subordinate rejection preserves provider business state; unknown outcomes retain
no automatic redispatch. This grants focused local regression evidence only.
Existing Conformance IR has three pilot rows and needs its own current scoped
consumption receipt before runtime admission; no full-row credit is inferred.

Independent review identified a test-expectation defect: default/NOHANDLE could
accept a Respond policy because output expectations came from the decoded plan
and the Respond branch accepted every input. Manager owns a bounded test repair:
fixed expected output lists supplied from the source cases, with Respond allowed
only for the three explicit RESP forms. Keep every case and the compiler repair.
Final layout/test delta requires independent review; local tests are rerun for
changed expectations. The documentation gate also requires the new contract's
exact registration in `docs/documentation-registry.json`; manager owns that
one-entry addition, preserving all existing document identities and order.

Documentation validation requires supported normative metadata and navigation.
Manager will mark the prepared document Proposed, supply its existing owner,
bounded scope and development version, keep runtime admission/integration
explicitly Pending, and register its single navigation entry. The reviewed body
remains exact; no source rule or runtime prerequisite is changed. Final header
and navigation delta join the independent incremental review before seal.

Final independent review scope: SYNC-REV-01 expectation repair, test-only
registration/layout and Proposed contract metadata/navigation delta. Reuse
unchanged source/fixture/semantic inputs only by exact hash. Manager retains
serialized integration and all acceptance gates.

After SYNC-REV-01's fixed independent expectations, the final four-test module
passes with zero failure/ignore. Module and public API checks also pass on that
final test input. The six compatibility controls and schema check retain their
actual previous-attempt receipts with unchanged relevant inputs; they are not
relabeled as new executions. Final contract metadata now uses supported Proposed
status with explicit Pending admission and a single navigation registration.
The reviewed semantic body remains unchanged. Final independent delta review
and outstanding mandatory documentation/policy/seal gates precede integration.

### Pre-dispatch scoped SYNCPOINT Conformance IR consumption

Manager declares SPI-1001.syncpoint-consumption-ledger from sealed71 f36532348edfbe7c29d4cf4bfb86c5281e0e23bd, tree10073f310d9b4c3f5a65398059a139bb109098ab. Exact application dependency row0218 SYNCPOINT, non-differential local consumption proof only. Demonstrated gap: sealed four-test compiled consumption proof does not yet bind observations through the existing Conformance IR/verdict/ledger machinery. Existing effective shared pilot has READ0156/REWRITE0181/SYNCPOINT0218,12obligations30gatecases and cics.file-uow.local ScenarioSpec; no full260 acceptance follows. User defers only0027/0093/0114, retain all263 identities and readiness flags. No licensed execution, parent credit or public SPI admission.
CLI author owns only NEW crates/tooling/mainframe-env-conformance/src/cics_pilot/tests/contract_consumption/ledger.rs plus a module registration (and only any strictly necessary private test helper access adjustment, reported exactly) inside existing sealed crates/tooling/mainframe-env-conformance/src/cics_pilot/tests/contract_consumption.rs. The entire old test module body/fourtests, frozen source/golden/participant/command fixtures and compiler predicate must remain exact except minimal declared helper visibility or module-registration changes. Manager solely owns global Conformance IR/spec/schema/generator/xtask/public facades/fixtures/status/ABI/security/runtime/integration. Worker must not alter those paths or construct a second dispatcher, coordinator, response mapper, evidence ledger or admitted source authority.
Implement a bounded test-side consumption binding using the actual existing Conformance IR/RuntimeRegistry/ConformanceRunner, ScenarioSpec, ObservationCheck, RunnerContext and canonical verdict event formats. Reuse the actual existing compiled COBOL -> binary interpreter -> durable coordinator -> CICS path and independent frozen six participant cases plus applicable omitted-local controls and SQLite reopen. Bind official row, rule/obligation/gate, candidate, spec, fixture/environment and scenario/artifact identities through existing framework. Prefer extending/consuming the existing SYNCPOINT ScenarioSpec and declared obligations; do not create a disconnected lookalike spec or hardcode pass verdicts. Inspect actual existing effective xtask augmentation; do not make the old static-spec-only inference. If a manager-owned seam prevents safe reuse, demonstrate the exact minimal required interface rather than bypassing it or writing shared paths. New private driver glue may adapt actual test helpers but cannot return generic success or predict actual values from itself. Independent expected responses/contexts/UOW/audit/effect identities stay frozen. Add meaningful negative controls: corrupt a returned condition/context/effect observation and prove the existing evaluator produces Fail; malformed/missing binding must fail closed. Keep all scenarios local/non-differential, scoped, candidate-bound and zero licensed credit. Do not assert trusted deployment context or complete row/application acceptance from pilot bindings; broad MQ capacity/trusted controls remain Pending unless actually exercised.
Acceptance: demonstrate inventory gap first, implement real compiled-route ledger tests, focused new filter executes nonzero tests and passes; existing four consumption tests stay green. Preserve actual logs, input hashes, fixtures/scenario/gate counts and candidate/ledger binding receipts externally; do not relabel historical71 receipts. Rust module ceilings/pinned tools apply; no broad Cargo/whole-cache/source campaign. Manager reviews independent expectations and actual effective spec binding, then integrates/mandatory-gates/seals exact bounded child serially. No manager/root writes, push/PR/license/install/delegation or schema/criteria loosening. Each build/test/lint sequence cargo clean the isolated intended checkout in finally; receipts outside target, CARGO_BUILD_JOBS2, --locked --offline. Source BEFORE semantic comparisons: pinned Python -B conformance/tools/ibm_docs.py --cache the configured retained topic cache search/read exact sources-c SYNCPOINT dfhp4_syncpoint.html SHA2e1bebaa9ac35c7444eeb63d2e15d1773a5d39e06f0e65f00970e96f411f9b34 full44lines and dfhp4_syncpointrollback.html SHA566d8661a0af02559d8679234e959d2e2aa2577dcf14c55211ec07c7c03e2954 full54lines; verify manifests and host/archive bytes first. No refresh/cache writes/repin. Reuse exact prior normative reads only after hash equality; no duplicate full dependency or source-family audit. Commit owned coherent implementation only after required focused pass; manager owns actual sealing. Handoff concise handoff.md/findings.json/source-receipts.json/validation.json with actual tests, current spec/scenario inputs, input hashes/candidate/tree/cleanup and full.diff. Stop at bounded handoff.

Manager independent receipt lane: execute the existing effective
`cics.file-uow.local` ScenarioSpec through `cargo xtask conformance --subsystem
cics --gate local` on an isolated clean sealed71 checkout. This is actual current
three-row/twelve-obligation local evidence, not the new context-extension worker
or full application acceptance. The manager owns only external receipts and
checkout-local disposable target; source/index are read-only. Record exact
candidate/tree, spec/environment/verdict identities, actual passes/failures and
cleanup. No source refresh, full certification, license or runtime promotion.

### Manager-owned effective Conformance IR export seam

`SPI-1001.effective-conformance-spec-export` follows sealed71. The compiled
consumption worker demonstrated that the test crate cannot call xtask's private
effective-spec builder; the committed static spec omits the existing CICS pilot.
Manager owns only a read-only `conformance-spec-export` CLI seam, extracting
`compile_shared_spec` into one owner module and retaining the exact augmentation
order, catalog validation and compilation for both old/new entrypoints. Export
validated raw effective document, normalized catalog metadata and current
candidate/catalog/spec identities as tooling output; never execution acceptance,
licensed evidence or a second spec/ledger builder. No row, obligation, rule,
fixture, registry admission or evaluator policy changes. Manager owns
`xtask/src/main.rs`, new `xtask/src/conformance_spec_export.rs`, the existing
verification runbook, unique fragment, status and derived documentation. Worker
owns only its declared private test paths and cannot edit this seam. Independent
review, actual same-builder roundtrip/effective-pilot checks and mandatory gates
precede serial child seal. Current clean71 receipt has30local verdicts passing
on three pilot rows/twelve obligations; it is distinct from new context-extension
evidence and does not accept260application rows or any SPI runtime route.

Pre-dispatch independent export-seam review: exact frozen owner module,
main routing/builder extraction and verification-runbook delta. Manager authored
this infrastructure change; different retained CLI reviewer owns external
findings only. No runtime or source rule changes, no duplicate worker ownership.
Actual focused/export and mandatory integration gates remain manager-owned.

### Resume scoped SYNCPOINT ledger implementation after owned seam repair

The same retained CLI author resumes on sealed72 after SCL-01 is repaired by
the independently reviewed same-builder export. Ownership stays limited to
`cics_pilot/tests/contract_consumption/ledger.rs` and minimal private-test parent
registration/access. Export is external, exact-candidate metadata with credit0;
all prior tests and source/fixture/rule/obligation identities remain intact.
Actual compiler/coordinator/provider observations and negative verdict proofs
remain required. Manager retains shared spec/fixture/CI and integration ownership;
no duplicated builder, hidden positive skip or runtime/parent/license acceptance.

### Manager-owned test input setup for scoped ledger consumption

Before integration, the manager declares the bounded setup dependency of
SPI-1001.syncpoint-consumption-ledger. The new private tests require a freshly
exported effective Conformance IR bundle; missing input must fail, never skip.
Manager ownership is limited to Jenkinsfile's existing Foundation test setup,
the existing verification runbook, a unique change fragment, status and derived
documentation. Preserve the existing workspace test invocation, evidence recorder
and all gate policy. Generate the bundle through the sealed same-builder exporter
before Cargo tests, retain it in the existing ignored CI output and pass its path
explicitly. The developer recipe likewise exports outside Git before the focused
tests. No worker edits these shared files; author80596 still exclusively owns its
two private test paths. Independently review final setup and test binding together;
focused integrated regressions and mandatory gates precede the child seal.
Source/spec/catalog/rule identities and official, runtime and licensed credit
remain unchanged. This setup does not accept the full application prerequisite.

Pre-dispatch independent review SPI-1001.syncpoint-ledger-input-setup-review:
exact manager-owned Jenkins Foundation export setup and developer recipe only.
The different retained CLI reviewer reads frozen full original/candidate files
and exact diff, repository/index/author80596/cache/history read-only. No Cargo,
repair, source refresh or broad audit; review actual export path/ignored-output/
failure propagation/current-candidate setup and preserved test/evidence commands.
Author ledger code is live and excluded from this review; its final frozen full
diff will require separate independent acceptance before integration.

### Pre-dispatch private PROGRAM physical SQLite observation

Manager declares SPI-1001.program-status-sqlite-reopen from sealed72 b6345483cad3bb2515999d26961b18758496fc16. Exact SPIrow0155 INQUIRE PROGRAM, stable immutable named definition observation only, no command admission or row credit. Demonstrated inventory gap: existing administrative_status/tests.rs recreates an owner over the same retained MemoryStore and explicitly disclaims physical restart; it contains no SQLite reopen proof. Existing concurrency/no-load/corruption tests remain sealed and are not repeated as new implementation.
The isolated CLI author owns only NEW crates/providers/mainframe-env-cics/src/handlers/program_control/administrative_status/tests/sqlite_reopen.rs plus one minimal module registration in existing administrative_status/tests.rs, preserving every old test/helper body. Manager alone owns production helper/facades, shared schemas/generators/IR/xtask/CI/status/fragments/documentation and serialized integration. Dependencies are sealed existing helper, MECPGD1/catalog owner/SQLite APIs and Proposed CICS-NAMED-PROGRAM-STATUS-V1 boundary. No dependency on live SYNCPOINT test paths; author does not inspect or edit them.
Prove actual last SQLite Arc close and fresh SqliteStateStore/CicsService reopen of stable enabled/disabled/multi-generation definitions, legacy NameOnly and absence, exact immutable references, full catalog/provider bytes and no query mutation. Reopen preserves local/non-Java cohort assumptions; no private/public issuer inference, loader call, autoinstall, new namespace/resource codec/status/response mapper, generic dispatcher or new operation/public route. Artifact storage is the existing authority; state exactly whether separately retained memory artifacts or physical artifact store is used, and never claim more than actual closure. Add meaningful malformed/truncated durable MECPGD1 and catalog inconsistency controls through existing store/API, requiring existing fail-closed outcome without fallback. No production fix without exact demonstrated defect and manager ownership disposition. Avoid redundant old concurrency/property/helper tests. PostgreSQL, warm restart, cross-instance freshness, selected product route, namespace/SAF/receiver and full application/SPI/license gates remain Pending0.
Source before expectations: pinned Python search/read INQUIRE PROGRAM own baseline ibm-cics-ts-6x-spi-command-bodies-2026-09-12 dfha8_inquireprogram.html SHAe3d8ed4c069bd26822c6373b35278ec126591e2efaf020b8fbcf8b811845f20f, no-load35-56/status566-572 and namespace/conditions scope. Consult current exact prepared contract; matching retained cache/archive only, no refresh/repin/publication bodies Git or oracle. Do not repeat whole cache/source campaigns.
Acceptance focused new module filter must execute nonzero SQLite tests0failure0ignore, old helper tests preserved and relevant focused controls pass, all stores physically dropped before reopening, fixtures/temp paths bounded and cleaned; logs/hash/closure/candidate/source receipts outside targets. CLI gpt-6.1-sol/high/default fastOFF,multi_agentfalse workspace-write/never, no nestedworkers. Pinned host Cargo/Python jobs2 --locked --offline; cargo clean intended checkout in finally. No Cargo.toml/lock or shared changes, push/PR/seal/install/license/network. Coherent owned test commit and concrete handoff.md/findings.json/validation.json/source-receipts.json/full.diff. Manager performs different-thread review and integrated focused/mandatory/sealer gates serially.

Independent setup review SETUP-R1 found the developer recipe did not enforce
export success before tests in an ordinary shell. Manager repairs only shell
sequencing by joining directory creation, export and the environment-bound test
with &&; quoted paths, selector and test flags remain exact. Jenkins already
uses its existing strict shell. Final delta review and actual integrated consumer
checks remain required; the original finding and frozen review are preserved.
Author-local final verification passes two new ledger tests, four frozen
consumption tests and the unknown-outcome compatibility test with zero failure
or ignore. Those actual worker receipts do not substitute for integrated checks.

Pre-dispatch SPI-1001.syncpoint-consumption-ledger-full-review: different retained
CLI reviewer consumes frozen actual passing attempt3 inputs (whole931-line new
module and exact two-line parent registration), existing independent fixtures,
actual same-builder export/ScenarioSpec/registry/verdict/ledger binding and
SETUP-R1's exact repaired developer sequencing plus unchanged Jenkins setup.
Frozen input hashes must match current validated author bytes. Author80596 may
finish external bookkeeping only; any new code delta invalidates this packet.
Repository/index/author/root/cache/source/history read-only, reviewer owns only
external review artifacts. No Cargo, repair, nestedworker or broad source audit;
fresh bounded pinned SYNCPOINT search/read required before semantic expectation
review. Manager alone integrates and seals after independent acceptance and
actual focused/mandatory integrated checks. Full app/SPI/FEPI/license Pending0.

Manager serialization begins after actual author80596 terminal0. Passing owned
bytes are imported with exact attempt3 hashes; worker sandbox staging failure
remains a retained diagnostic and is handled only by the authorized manager lane.
Manager review identifies a provenance validation gap: the exported catalog digest
hashes the committed index and the compiled spec digest hashes the raw document;
neither hashes supplied normalized row metadata. Row count/gate closure alone
therefore cannot reject altered source locators, families or subsystem metadata.
Before acceptance, manager owns a bounded private-test repair: bind the complete
normalized exported row array to the frozen current same-builder projection hash
and add altered-row-metadata negatives. No second catalog loader or evidence
builder, source/schema/spec/catalog policy change or credit is introduced.
Original validated author bytes/review packet remain preserved. Changed expectations
require actual focused rerun and independent final delta review before seal.

### User-authorized CLI worker sandbox change

The user explicitly requests Codex CLI workers without the sandbox after shared
Git index.lock denials. Future CLI dispatches/resumes use --sandbox
danger-full-access -a never with the same current account, gpt-6.1-sol/high/default,
fastOFF and multi_agentfalse. Existing live turns finish before their retained
thread resumes; no quiet-process restart, alternate account or nested workers.
Exact path ownership, isolated checkouts, source-refresh limits, cleanup and
acceptance gates remain in force. Manager owns root/shared authorities/integration.
First bounded retained author resume owns only a coherent local Git checkpoint
of the exact two already-passed attempt3 test files. Preserve all original
failure/handoff/validation receipts; no code or expectation edit and no repeated
Cargo tests. This worker checkpoint is unsealed and grants no acceptance credit.
Actual integrated repaired candidate review/gates/sealer remain manager-owned.
