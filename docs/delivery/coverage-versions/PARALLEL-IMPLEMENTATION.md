# Parallel implementation plan

Status: **Proposed**

Release numbers remain sequential, but implementation work does not have to be.
This document distinguishes three events:

1. **Start**: a workstream may create an isolated branch/worktree after its
   start gate is frozen.
2. **Integrate**: a workstream may merge contracts and implementation after its
   integration dependencies pass.
3. **Release**: versions are promoted in numeric order from one clean,
   evidence-backed source identity.

Starting 0.14 work before 0.11 ships does not allow 0.14 behavior to leak into
the 0.11 `core-server` profile.

## Post-131 entry and shared prerequisites

The release DAG below is unchanged. Before broad 0.9 work, verify and record the
integrated post-#131 entry candidate in [status/0.9.0.md](status/0.9.0.md);
[#137](https://github.com/toreleon/mainframe-env/issues/137) tracks this real
evidence handoff. A merged hardening PR alone does not make CIC-901 ready.

All 0.9–0.17 lanes use the shared
[hardened slice acceptance](../../prompts/coverage-versions/README.md#hardened-slice-acceptance).
Freeze the minimum shared participant contract through its existing owner before
dependent CICS/Db2/IMS/MQ adapters integrate; track that early work under INT-1601,
not as a new foundation release or another runtime. Provider-specific mutation,
replay and recovery proof lands with each slice; 0.16 owns the final mixed-resource
matrix and coherent restore boundary, not the first participant interface.

Prepare licensed environment, fixture and capture/normalization inputs alongside
each lane through CER-1701's shared interfaces. Preserve each minor's licensed
exit requirement and the complete final 0.17 campaigns. Normalize z/OSMF
operations and assign backend owners before broad adapter implementation; the
four named backend dependency versions alone do not cover every service family.

After accepted dependencies, CICS 0.9→0.10, Db2 0.12→0.13, IMS 0.14 and MQ 0.15
remain parallel lanes; z/OSMF 0.11 consumes accepted operation-level backends.
0.16 integrates the providers and 0.17 certifies their exact final composition.
Prepare REL-1001 metadata before terminal CER-1706 rehearsal; promote 1.0 only
with separate authorization. Early contract/harness work neither changes release
order nor permits later-version capabilities in an earlier public profile.

## Dependency graph

```text
0.2 coverage/catalog/de-hardcode foundation
 |
 +--> 0.3 COBOL structure --> 0.4 COBOL semantics ---------+
 |                                                           |
 +--> 0.5 RACF/SAF -------------------------------+          |
 |                                                |          |
 +--> 0.6 dataset/VSAM/AMS ------------------+    |          |
 |                                           |    |          |
 +--> 0.7 JCL converter -------------------+ |    |          |
 |                                         | |    |          |
 |                         0.5 + 0.6 + 0.7 -+-+----+--> 0.8 JES2
 |                                                           |
 |                         0.4 + 0.5 + 0.6 ---------> 0.9 CICS API
 |                                                           |
 |                                            0.9 --> 0.10 CICS SPI/FEPI
 |                                                           |
 |   0.4 + 0.5 ----------> 0.12 Db2 core --> 0.13 Db2 full  |
 |   0.4 + 0.5 + 0.6 ----> 0.14 IMS full                    |
 |   0.4 + 0.5 ----------> 0.15 MQ full                     |
 |                                                           |
 +-- 0.5 + 0.6 + 0.8 + 0.10 ------------------> 0.11 z/OSMF |
                                                             |
        0.8 + 0.10 + 0.13 + 0.14 + 0.15 -------> 0.16 integration
                                                             |
                     0.11 + 0.16 ----------------> 0.17 certification
                                                             |
                                              0.17 --> 1.0.0
```

## Safe concurrency waves

| Wave | Parallel workstreams | What remains serialized |
|---|---|---|
| Foundation | 0.2 only | Official catalog schema, generated identity model, handler registry, application-package schema, coverage store, and architecture gates |
| Structural foundation | 0.3 CI-300 thin Conformance IR/obligation model followed by COBOL structure; 0.5 RACF; 0.6 dataset; 0.7 JCL converter; private Db2/IMS/MQ catalog preparation | CI-300 freezes the shared IR v1, typed registries, obligation/verdict/ledger contract, and shard identity before other lanes merge behavioral coverage claims; host/store/source changes go through existing owners |
| COBOL bridge | 0.4 after 0.3; unfinished 0.5–0.7 lanes and private provider preparation continue | COBOL host-extension ABI and effect contract |
| Main semantic engines | After 0.4–0.7 dependencies pass: 0.8 JES; 0.9 CICS API; 0.12 Db2 core; 0.14 IMS; 0.15 MQ | Interpreter host-extension ABI, mutation/UOW contract, and capability vocabulary |
| Subsystem completion | 0.10 after 0.9 and 0.13 after 0.12 can overlap; 0.11 adapters prepare and complete after 0.8 plus 0.10 | Public route advertisement and cross-provider transaction contract |
| Integration | Independent 0.16 transaction, security, restart, cancellation, overload, and backup/restore matrix shards | Final mixed-state model, evidence digest, and source candidate |
| Certification | Per-subsystem IBM oracle runs for 0.17 | Environment receipt normalization, cross-subsystem replay, release evidence, and promotion |

## Recommended long-lived lanes

| Lane | Primary ownership | Earliest start | Can run alongside |
|---|---|---|---|
| Catalog/codegen | Official receipts, schemas, generated identities, thin Conformance IR compiler, obligation bindings, verdict runner, and derived ledger | 0.2 catalogs; executable IR from 0.3 | All later lanes after the relevant contract freeze |
| COBOL | Source, syntax, semantic types, HIR/MIR, interpreter, LE extensions | 0.3 | RACF, dataset, JCL; later Db2/IMS/MQ provider internals |
| Security | RACF commands, profile schemas, ACEE/tokens, SAF | 0.5 | COBOL, dataset, JCL |
| Data | Catalog, allocation, VSAM, locking, AMS | 0.6 | COBOL, RACF, JCL |
| Batch | JCL converter, JES2, spool, utilities | 0.7 | CICS and data services after dataset/security interfaces freeze |
| CICS | API, SPI, FEPI, resources, distributed conversations | 0.9 | JES, Db2, IMS, MQ |
| Db2 | SQL parser/binder/catalog/relational executor | parser skeleton after 0.2; completion after 0.4 and 0.5 | JES, CICS, IMS, MQ |
| IMS | DBD/PSB/SSA and DL/I organizations | catalog skeleton after 0.2; completion after 0.4–0.6 | JES, CICS, Db2, MQ |
| MQ | MQI and queue-manager object/lifecycle model | catalog skeleton after 0.2; completion after 0.4 and 0.5 | JES, CICS, Db2, IMS |
| z/OSMF | Generated routes and protocol adapters | route catalog after 0.2; completion after backend versions | Backend implementation lanes |
| Integration/certification | Mixed UOW, failure injection, licensed oracles, evidence | incremental after each provider freezes | All lanes, but final candidate is serialized |

## Parallel work rules

### Work that can proceed independently

- Official-document extraction, normalization, and review per subsystem.
- Parser and validator implementation against immutable subsystem catalogs.
- Provider-private state and semantics behind already-frozen owned contracts.
- Golden, property, malformed-input, resource-bound, and oracle fixtures.
- Application-package resources that use the accepted package schema.
- Out-of-process IBM oracle harnesses and environment preparation.

### Work that must be coordinated

- New public contract variants, semantic operation IDs, status/condition
  families, capability names, durable state schemas, and migration heads.
- Any change to invocation identity, effect sequencing, idempotency,
  transaction/UOW, security principal, or recovery semantics.
- Any route advertised by `core-server` and any change to profile closure.
- Any generator output consumed by two or more subsystem packages.
- Cross-subsystem `ScenarioSpec` driver, ordering, failure-point, or explicit
  row/obligation credit vocabulary.

### Merge discipline

1. Each lane uses an isolated branch/worktree and declares its catalog and
   contract versions.
2. Shared contract proposals land through an ADR and compatibility impact
   statement before provider implementations depend on them.
3. Generated output lands with its source catalog, generator version, digest,
   and review receipt.
4. A later-version lane may rebase on earlier releases but may not backport an
   incomplete public surface.
5. Integration merges one lane at a time and runs narrow affected gates after
   each merge. Run tier-3 complete affected-scope validation once for the final
   unchanged minor candidate; reserve global tier-4 environments for scheduled
   integration, 0.16/0.17, or release certification unless the lane changed
   their consumed contracts or routes.
6. Conflicts are resolved by the authority owner; no duplicate default route or
   fallback is introduced to make branches coexist.

## Highest-value parallel schedule

Immediately after 0.2 freezes the shared contracts, the most efficient five-way
start is:

1. COBOL structural completeness (0.3).
2. RACF/SAF (0.5).
3. Dataset/VSAM/AMS (0.6).
4. JCL converter/parameter catalogs (0.7).
5. Db2/IMS/MQ catalog and parser preparation, without merging runtime behavior
   until COBOL/security/data contracts are ready.

After 0.4–0.7 stabilize, JES 0.8, CICS 0.9, Db2 0.12, IMS 0.14, and MQ 0.15 are
the main parallel semantic-engine wave. This is the largest schedule reduction.

## Critical path

The likely release critical path is:

```text
0.2 -> 0.3 -> 0.4 -> 0.9 -> 0.10 -> 0.16 -> 0.17 -> 1.0
```

JES can become critical if 0.5/0.6/0.7 do not finish before 0.8. Db2 can become
critical because 0.12/0.13 has the largest semantic scope. Staffing should
therefore favor COBOL/CICS and Db2 while keeping dataset/security contracts
stable and well owned.
