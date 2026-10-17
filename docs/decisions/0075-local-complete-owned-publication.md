# ADR-0075: Owned initial local Complete publication

Status: Proposed
Owner: Foundation runtime / application composition
Scope: Explicit signed initial local Complete/Applied publication and ordinary compiled ProgramCall/JES owner
Applies from: mainframe-env explicit local complete-initial profile, generation 1

The signed `resource/local-complete-publication` Resource must contain exactly
`mainframe-env.application-local-complete-initial@1`. This opts into the finite
initial-only publication port. It does not upgrade a storage-only selection or
permit forward selection, rollback, external migration, automatic MQ triggering
or vendor-equivalence claims.

The actual application publication writer, installer and all prepared CICS,
dataset, Db2, IMS, MQ, Batch and RACF owners remain held through one genuine
same-physical-store composition CAS acknowledgement. The canonical existing
publication row is included in the selected binding and exact static row set;
its Applied sections derive from actual prepared controller/catalog owners.
Unknown commit outcomes quarantine every participating owner. No equal row,
caller flag or independently written Complete document supplies this authority.

Selected complete readback requires the original Ready generation, signed mode,
full selected static rows, SourceIds, seed ownership and exact provider catalog
projections. Recovery validates this complete owned row and skips the old IMS
republication over those fixed-catalog versions. Legacy publication and the old
queue-only/storage-only profiles keep their original refusal predicates.

The first owner control compiles the actual finite LOCALPGM source, selects its
signed ProgramCall controller, submits original IKJEFT01 JCL and runs the normal
JES worker. It compares actual Completed job/step/work and committed checkpoint,
plus the original Success program audit, on Memory and SQLite. The ordinary
outer host dispatch is not labelled as a core EffectRecord. MQ-consuming durable
producer/launch, IBM automatic triggering and broader migration remain separate.
All new controls are unrun until Root applies and executes the focused recipe.
