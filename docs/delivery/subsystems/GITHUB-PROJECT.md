# Track subsystem work on GitHub

Status: **Subsystem work management**

Use the same subsystem and phase names as
[the registry](../../documentation-registry.json) in issues and project fields.
Link each issue to its plan and current status record. Keep a bounded work item
with an owner, consumed dependencies, acceptance checks, and blockers.

Follow the [coordination workflow](../WORK-COORDINATION.md) for issue intake,
claims, overlap checks, PR bases and closure. Recommended project fields are
Subsystem, Phase, Work package, Owner, Flow status, Delivery state, Priority,
Dependencies and Blocker. Flow runs through Triage, Ready, In progress, In review,
Awaiting integration, Blocked and Done; labels remain usable without board
access. Delivery state distinguishes branch implementation/verification from
merge and acceptance. An epic does not close from partial child results.

Completion means the item's declared scope passed its required checks and its
delivery boundary is recorded; licensed verification can remain pending only
when explicitly allowed by the plan. Main-targeted fixes await main integration.

Connect pull requests to their work item and describe the behavior, affected
contracts, validation, and unavailable environments. Update the owning status
record after integration. Generated indexes summarize these records; GitHub
release objects and historical release milestones are not used for tracking.
Keep the existing [board adoption issue #228](https://github.com/toreleon/mainframe-env/issues/228)
open until fields, access, ownership and synchronization are actually verified.
Issue forms and documentation alone do not install project automation or merge
protections.
