# Track subsystem work on GitHub

Status: **Subsystem work management**

Use the same subsystem and phase names as
[the registry](../../documentation-registry.json) in issues and project fields.
Link each issue to its plan and current status record. Keep a bounded work item
with an owner, consumed dependencies, acceptance checks, and blockers.

Recommended project fields are Subsystem, Phase, Owner, Status, and Blocker.
Recommended statuses are Proposed, Ready, In progress, In review, and Complete.
Completion means the item's declared scope passed its required checks; licensed
verification can remain pending only when explicitly allowed by the plan.

Connect pull requests to their work item and describe the behavior, affected
contracts, validation, and unavailable environments. Update the owning status
record after integration. Generated indexes summarize these records; GitHub
release objects and historical release milestones are not used for tracking.
