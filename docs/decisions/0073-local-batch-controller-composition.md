# ADR-0073: Owned local TSO ProgramCall controller composition

Status: Proposed

Owner: Application package and batch maintainers

Scope: Finite local controller-registry and compiled program-mapping selection

Applies from: mainframe-env 0.2.0 local owned application composition profile

The finite local profile admits the existing typed `batch_controllers` section only for TSO `ProgramCall`. The signed generic Resource at `resource/batch-controllers` contains the exact canonical, name-ordered existing `BatchControllerDefinition` list. The original decoder and controller registry validator remain authoritative. Every controller refers to a genuinely compiled, source-committed, signed Program admitted by the same local composition. Built-in program names, unsupported IMS plans, missing commitment, foreign selected selectors and unowned legacy adoption refuse.

The actual controller mutex remains held through the full original registry snapshot, prospective install or retained rollback, full original-codec CAS row and genuine same-Arc acknowledgement. The server retains its original publication write exclusion. Ordinary controller installation and reads keep that same exclusion and mutex. Provider owners precede RACF; the final composition order is CICS, dataset, Db2, IMS, MQ when present, batch, RACF. Unknown acknowledgement fences the live controller owner and server until authoritative reopen. Direct independent provider administration remains trusted; full observed bytes and versions plus CAS do not claim external-writer exclusion.

The new participant retains every unrelated controller generation and all JES job, scheduler, topology, spool and replay state. An exact retry does not rewrite the registry or compiled mappings. Forward generations and rollback preserve the original retained-generation identities, caps and selector ownership rules. The new finite cache check reads at most two keys under the original bounded namespace API and refuses any key other than the single global registry key; this snapshot grants no later external-writer exclusion. The original global registry is bounded to 16 MiB for this finite observation; current and next canonical copies together to 32 MiB, selected projection to 8 MiB, and the prepared row to 16 MiB. Larger retained history can be refused by the new local profile; the original 64 MiB legacy registry cap is unchanged. These are serialized admission limits, not resident-memory limits.

The atomic commit also selects exact `batch-program` mappings backed by genuinely admitted immutable artifacts. Readiness checks full selected controller content, actual registry cache/store version and full static mapping rows. It does not fabricate `ApplicationPublicationState::Complete`: the original controller execution facade still requires genuine Complete publication with its controllers section Applied. This component proves local installation and selection only. It supplies no IMS launcher, runtime batch execution, vendor ABI, global atomicity or full CD009 credit. Original V3, full-record, seed, reopen, rollback, 114-observation, 105-criterion and missing vendor-body requirements remain unchanged.
