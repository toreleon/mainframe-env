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

## Reconcile the existing board safely

Project [#3](https://github.com/users/toreleon/projects/3) is the existing
coordination view. Reuse its item and field identities instead of replacing the
project or closing/recreating issues for formatting. `Status` is the flow field;
rename Backlog to Triage while preserving its option ID, retain the other option
IDs and add Awaiting integration. Add P3 to Priority without resetting P0–P2.

Keep these additional fields: Phase, Work package, Owner, Delivery state,
Dependencies, Blocker, PR base, Source and Canonical issue. Assignees remain the
actual GitHub assignments; Owner can also record an existing scoped claim such
as Foundation #393. Unclaimed work remains explicit, and historical authors
are not assigned automatically. Unknown delivery is not proof that code is
missing. Closed-without-delivery distinguishes abandoned/duplicate scope from
completed scope. Branch-qualified closures retain their integration dependency.

Recommended views reuse the existing table and board as All work and Active
work (`is:open`), with Foundation (`is:open subsystem:Foundation`) and History
(`is:closed`) tables. Keep Status, Phase, Work package, Owner, Delivery state,
Priority, Dependencies and Blocker visible. Preserve existing Size values and
milestones; they do not define acceptance or ownership.

At claim, review, integration and handoff:

1. Update the canonical issue's coordination record and one flow label.
2. Reconcile its board fields and linked PR. Search existing content IDs before
   adding an item, and retain an existing parent instead of moving it silently.
3. Review source-qualified delivery separately from issue closure. Closed
   Foundation controls #394–#397 remain Verified on branch until actual main
   integration is established.
4. Read the affected items back and compare them with their canonical issues.
   Stop a stale update when scope, labels, owner or producing source changed.

## Access and current adoption evidence

The 2026-10-11 inspection reads all 137 existing items and 17 fields. The
account-level `viewerCanUpdate` query returns true, but actual field and item
write mutations both return `Resource not accessible by integration`. That flag
does not prove the current credential can write. No direct board write succeeded.
Branch-protection read separately returns HTTP 403; the accessible ruleset list
is empty. Neither observation verifies that merge protections are installed.

All 68 open issues now have a current coordination record and one flow,
priority and area classification. Existing issue states and assignees are
preserved. Eight explicit child/parent links are installed: #406–#412 under
#405, and #413 under #16. The staged board reconciliation adds the twelve
missing open issues, four branch-qualified closed controls and two open PRs,
for 155 unique items, and corrects closed issues left in active columns.
Those board changes remain unapplied until a credential completes the actual
write operations and readback succeeds. Preparation is not configuration.
After the parent links were installed, a subsequent read contains #413 and
138 items. This does not establish successful direct board mutation or complete
synchronization; the migration deduplicates existing content IDs.

Track authorized board write access, remaining ownership/handoff decisions,
independent workflow review/integration and administrator protection checks in
#228. This change installs no automatic flow/delivery synchronization; existing
automation configuration has not been verified. Maintainers perform the routine
above until an authorized automation has successful execution evidence. Do not
store access tokens in issues or Git.
