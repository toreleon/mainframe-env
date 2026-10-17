# ADR-0071: Owned local fixed IMS catalog composition

Status: Proposed

Owner: Application package and IMS maintainers

Scope: Finite local owned application composition with the existing fixed IMS catalog and retained provider state

Applies from: mainframe-env 0.2.0 local owned application composition profile

The finite local profile can join an existing full `ImsMetadataCatalog` and its retained package selection to the genuine CICS, dataset, Db2 and RACF provider composition. This is a local storage contract. It adds no IBM ABI, automatic legacy seed mapping, TM installation, schema migration or complete application-publication authority.

The signed generic Resource at `resource/ims-metadata` contains exactly the existing catalog's canonical serde JSON bytes. Every database, organization, version, segment, field, index, relationship, PSB, PCB, PROCOPT and sensitivity field participates. The full typed package section must equal those signed bytes. The original metadata validator, secondary applicability predicate, database engine construction and row codecs remain authoritative.

The provider retains its actual durable lock through the original full runtime snapshot, existing retained generation/selection codec, full CAS writes and genuine same-Arc acknowledgement. The ordinary metadata publisher takes that same lock. Unknown acknowledgement quarantines the live provider and server until authoritative reopen. Independent direct store administration remains trusted; a bounded scan is an immediate snapshot and gives no later exclusion authority over external writers.

The existing runtime supports one generic catalog and refuses installation of a different catalog. This finite profile therefore owns that one catalog exclusively. Foreign nonempty CURRENT metadata selection, unowned adoption and missing or stale owner rows are refusals, including exact retries. A package-generation advance or rollback may select only a retained byte-equal catalog. Runtime records, replay rows, positions, checkpoints and settled unit-of-work data remain unchanged. An active pending undo is refused. Schema changes, independent catalog merging and data migration require separate owned implementations.

The new profile bounds full runtime observations and each retained metadata namespace to 16 MiB, original 64 retained generations and original store scan bounds. Canonical current plus next snapshots are bounded to 32 MiB, the static projection to 8 MiB and full prepared writes to 16 MiB. Larger existing histories can be refused by this new finite admission; legacy runtime limits and ordinary metadata publication remain unchanged. These limits are not aggregate resident-memory claims.

Local readiness compares the genuine selected generation, full installed catalog, engine definitions and actual provider versions. Runtime payloads remain provider-owned. It uses the existing local Ready selection and does not fabricate `ApplicationPublicationState::Complete` or authorize the original complete-publication facade. Full CD009/V3/reopen/rollback/oracles, 114 observations, 105 criteria and vendor body requirements remain unchanged.
