# CICS baseline lint repair and Rust caller migration

This repair makes `mainframe-env-cics` pass strict Clippy with all targets and all
features on the pinned Rust toolchain. It preserves validation order, subsystem
behavior, error mapping, authorization, test assertions, and durable/wire bytes.
It adds no IBM semantic coverage, licensed execution credit, or issue acceptance.

## Exported Rust input migration

`bts_lifecycle::BtsEffectInput<'a>` carries the existing `run_unit`,
`owner_execution`, `owner_principal`, `replay_key`, and `request_digest` arguments.
The receiving methods still validate these values in the original order.

- `BtsLifecycleStore::define_child(process_type, process_name, parent_id,
  definition, effect)` groups the final five arguments into `effect`.
- `BtsLifecycleStore::remove_subtree(process_type, process_name, effect, removal)`
  groups the five arguments between the process name and removal.
- `BtsLifecycleStore::mutate_process(process_type, process_name, effect,
  transition)` groups the five arguments between the process name and transition.

`CicsPartitionInput<'a>` carries the existing `aid`, `partition`, `data`, and
`cursor` arguments. `CicsService::submit_partition_input(session, principal,
csrf_token, input, now_tick)` accepts that input without changing authentication,
partition selection, cursor bounds, session CAS, or expiry handling.

CICS and server regression callers are migrated here. Embeddings must wrap their
existing argument expressions in these named inputs. No durable data migration
is required.

## Compiled source authorities

The existing private fixture setup, LU6.1 owner assignment setup, and receipt
pruning controls retain their implementations under `cfg(test)`. Their call sites
are test-only in current main and the reserved Foundation branch. The lint repair
does not export the private device module or promote its embedding configuration
workflow. Production load/invoke/finish validation and durable state stay intact;
the Foundation owner still controls future public provisioning and retention
integration. The named effect and partition inputs above are the only new public
Rust types needed for caller migration.

Private process-definition retry options, operator message options, parsed calendar
fields, supplemental web-service writes, and simple return-type aliases replace
long argument lists and nested types. Boolean assertions, borrowed single-element
slices, initializer defaults, explicit chunk arrays, and collapsed conditional
branches retain their original assertions and short-circuit behavior.

Focused regression receipts belong outside Git. Aggregate policy gates, external
caller migration, and integration-tree validation remain manager-owned; a local
package lint pass does not establish whole-workspace acceptance.
