# ADR-0100: Group existing CICS arguments into borrowed inputs

Status: **Proposed for the baseline lint repair**
Owner: **CICS provider maintainers**
Scope: **Rust callers of BTS effect transitions and partition input submission**
Applies from: **mainframe-env unreleased CICS baseline lint integration (pending merge)**

## Context

The pinned strict Clippy gate rejects exported BTS transition methods and
partition input submission because their positional argument lists exceed the
repository limit. The existing validation, effect identity, authorization, and
durable representation must remain intact during the lint repair.

## Decision

Introduce `bts_lifecycle::BtsEffectInput<'a>` to borrow the existing run unit,
execution, principal, replay key, and request digest. `define_child`,
`remove_subtree`, and `mutate_process` accept that input in place of the five
positional effect arguments. They destructure it before the same validations
in the original order.

Introduce `CicsPartitionInput<'a>` to borrow the existing partition and data and
carry the existing AID and cursor. `submit_partition_input` accepts that input
between the unchanged CSRF token and clock arguments. Authentication, bounds,
expiry, and session compare-and-swap behavior remain unchanged.

Migrate the provider callers and the four server regression call sites by
placing each original expression in its corresponding named field. The input
types retain no authority beyond the borrowed values and create no persisted
schema, effect encoding, or product route.

## Consequences and migration

This changes the exported Rust method signatures. Embeddings must wrap their
existing arguments in the two named input types; no durable data migration is
required. The provider and server regressions check the in-repository callers.

Keep the private device setup and receipt controls within their current fixture
scope. This decision does not export that module or integrate the reserved
Foundation work. Claims, issue scope, and Foundation branch ownership remain
with their recorded owners.

The decision remains proposed while aggregate gates and merge are pending.
Focused branch verification does not establish main integration, issue
acceptance, or licensed IBM conformance.
