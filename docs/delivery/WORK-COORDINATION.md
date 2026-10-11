# Coordinate contributions and parallel work

Status: **Proposed contribution workflow**
Owner: **maintainers**
Scope: **issue intake, ownership, parallel changes, integration and closure**
Applies from: **mainframe-env integration of this workflow**

Use one canonical issue for each bounded outcome. Link it to its subsystem
phase, work-package ID and owning progress record before starting implementation.
The [roadmap audit](WORK-AUDIT-2026-10-11.md) identifies the current queues;
the [remaining-work inventory](subsystems/REMAINING-WORK.md) defines all 54
legacy roadmap packages. Read [CONTRIBUTING](../../CONTRIBUTING.md) for build,
source-review, evidence and repository hygiene requirements.

## Sources of truth

| Question | Authority |
|---|---|
| What belongs in a phase? | Registered subsystem plan and its acceptance contract |
| What exists and what passed? | Owning progress record, exact source and retained evidence |
| Who is doing the next bounded slice? | Canonical issue, confirmed assignee and current claim |
| What is being reviewed or integrated? | Linked PR, its base/head and review disposition |
| What can a user obtain from main? | Merged source and its applicable validation |
| What is licensed or independently verified? | Declared oracle/environment and the relevant acceptance gate |

The project board is a view of these records. Missing board access must not
prevent issue-based coordination. Historical release numbers remain lookup
aliases; use current subsystem/phase names and registry paths for new work.
Milestones are optional delivery groupings, not owners or evidence of readiness.

## Intake and readiness

Search open and closed issues, current PRs and the owning progress record before
creating an issue. Reuse an existing outcome; split a broad epic into a child
only when the child has a distinct deliverable and acceptance boundary. A bug
fix, source decision and acceptance campaign can depend on each other without
becoming duplicate implementation tasks. Security reports follow
[SECURITY](../../SECURITY.md).

Every bounded issue records:

- subsystem/phase, parent epic and stable work-package or slice ID;
- current source and implementation state, including existing drafts or PRs;
- one observable outcome, affected contracts/paths and explicit exclusions;
- consumed dependencies and PR base; identify blockers by issue or exact input;
- a confirmed owner before implementation, plus the intended review boundary;
- acceptance commands/journeys and required negative, recovery or parity cases;
- pending environments, source identities, decisions and the next action.

An issue is Ready when its next slice is bounded, its prerequisite inputs are
available and its overlap review is complete. A contributor claims it by naming
the exact slice, branch/base, owning contracts and expected paths in the issue.
The maintainer confirms the claim and assignee before parallel implementation.
The current task's explicit authorization can supply that confirmation. Do not
assign another person merely because they previously contributed to that area.

Private source/catalog preparation may precede public runtime acceptance when
the plan permits it. Missing licensed inputs block the dependent campaign,
not every locally implementable task in the phase. An unavailable prerequisite
must have a concrete next action; elapsed time does not supply an approval.

## Flow state and delivery state

Use one flow label on an open bounded issue. Review gates and implementation
state belong in the issue body and progress record, not in a guessed percentage.

| Flow label | Meaning and transition requirement |
|---|---|
| `status:triage` | Scope, owner, prerequisites or next action still need confirmation |
| `status:ready` | Bounded next slice can be claimed; no unresolved prerequisite for that slice |
| `status:in-progress` | Confirmed owner has active implementation or investigation |
| `status:in-review` | A reviewable PR and scoped validation are available |
| `status:awaiting-integration` | Child scope reviewed/verified; target main has not received it |
| `status:blocked` | The next required step waits on a named input, dependency or decision |

Separately record delivery as Planned, Preparation implemented, Implemented on
branch, Verified on branch, Merged with validation pending, or Scope complete.
These states do not imply completion of a parent phase. A draft is not a shipped
fix; a merged parser does not prove runtime behavior; a local pass does not
prove licensed equivalence. Mark unrecorded facts Unknown rather than Unstarted.

Use existing `area:*`, `priority:p0` through `priority:p3`, `bug`, `enhancement`,
`question`, `documentation` and `type:epic` labels. Prefer `area:jcl-jes` and
`area:dataset` for new work instead of the historical `area:jcl`/`area:vsam`
aliases. Priorities describe impact, not completion order inferred from an old
version number. Assign P0 to silent wrong results/data loss, P1 to current-line
blockers, P2 to bounded gaps and P3 to work no current delivery depends on.

## Parallel ownership and PR bases

Ownership applies to behavior and authority, not just filenames. Two PRs can
conflict through the same schema, route, store, generated catalog, fixture or
acceptance journey even when their diffs touch different paths.

1. Inspect the current PR diff and the relevant active claims before editing.
2. Reserve one implementation owner for a shared contract change. Consumers
   depend on that issue/PR rather than reimplementing the owner.
3. Coordinate a shared-path change through a reviewed slice or handoff. Record
   who owns the change and when the dependent work can resume.
4. Start independent work from current `origin/main` in a separate worktree.
   A Foundation-dependent repair targets `codex/foundation-public-hardening`
   while #393 owns those prerequisites; record it as a child PR.
5. Give generated outputs their existing generator and each PR its own change
   fragment. Regenerate shared manifests after combining both source changes.

The default is one active implementation slice per contributor and one writer
per shared contract. Record an explicit split when parallel slices are safe.
Review and input preparation may proceed alongside implementation. Check the
claim again before resuming stale work; a quiet chat is not a released claim.

Foundation owns its active READ/restart, package publication, online/BMS, batch,
SDK and IMS bridge changes. The source-only SPI-1001 join slice is a separate
candidate for parallel work; it must keep public SPI/FEPI routes fenced and
coordinate shared generators/manifests. This reservation is scoped to the
active #393 work, not permanent ownership of entire subsystems.

## Review, integration and closure

Keep PRs centered on one outcome. Include the canonical issue, consumed PRs,
base/head, behavior before/after, shared boundaries, migrations and limitations.
Use the [PR template](../../.github/pull_request_template.md). The reviewer
checks the diff, overlap and applicable gates; a template is not a CI gate.

| Evidence field | Required content |
|---|---|
| Source | Producing/tested SHA and affected input identities |
| Validation | Actual command/journey, result, scope and retained receipt |
| Reuse | Original producer and reviewed equivalence of the relevant inputs |
| Pending | Unrun, failed and blocked checks listed separately |
| Integration | Actual target branch, dependent consumers and final-candidate checks |

Run focused regressions and mandatory policy/documentation checks, then stop
unless changed inputs, a failure or the acceptance contract requires more.
Do not weaken deadlines, floors or assertions, add ignores to earn a pass,
repeat unchanged failed inputs, or relabel an old receipt as a new-head result.
Follow the [verification workflow](../runbooks/VERIFICATION-WORKFLOW.md).

For a main-targeted defect, use `Closes #N` only when its complete declared
scope can close on integration. Child PRs use `Refs #N`; keep the issue awaiting
integration until the reviewed fix reaches main and applicable acceptance is
recorded. An explicitly branch-scoped task may close earlier, but its delivery
scope and open integration tracker must be linked. Historical Foundation
closures #394–#397 are branch-scoped results; the Foundation coordination
tracker retains their outstanding delivery to main.

After integration, update the owning progress record, reconcile the issue and
regenerate documentation. Close a bounded issue only with a resolution link,
source, applicable results and explicit remaining parent obligations. Use
Duplicate/Not planned with a reason for abandoned or duplicate scope. Reopen a
completed issue only for a demonstrated unmet obligation or regression.

## Maintainer routine

At intake, integration and handoff, check active claims, dependency order,
stale blockers, final-head validation and branch-only fixes. During regular
triage, review unassigned work and release claims explicitly when an owner
stops. A handoff records the current source, remaining scope, evidence and next
action. Keep these updates in the canonical issues and progress records;
chat-only plans and old board snapshots are insufficient ownership records.

Board access and branch-protection configuration require their own authorized
repository access. Do not claim those protections are installed from prose or
a template. Track board adoption under
[#228](https://github.com/toreleon/mainframe-env/issues/228).
