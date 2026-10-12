# ADR-0074: Explicit local MQ process/trigger graph admission

Status: Proposed; source-only draft, uncompiled and unrun.
Owner: MQ provider and application composition maintainers
Scope: Versioned finite static graph admission under the actual local composition owners
Applies from: mainframe-env 0.2.0

The opt-in signed Resource `resource/mq/graph-profile` contains exactly
`mainframe-env.local-mq-process-trigger-graph@1`. Its local graph is
Queue.target -> Trigger, Trigger.target -> Process, Process.controller -> an
exact admitted Batch ProgramCall controller. Queue/Trigger controllers and
Process targets are refused. Queue names, trigger names and process names
retain the existing canonical case-sensitive MQ name rules. A trigger has
exactly one incoming queue edge; every process and trigger must be reached.
Queue-only offers keep their original representation and admission.

Every graph row retains complete signed Resource bytes. The Batch participant
retains its existing exact typed controller Resource and actual compiler
source/program/artifact closure. The existing legacy catalog already derives
typed Process objects from queue trigger_program fields; this component uses
that unchanged constructor and original row codecs. The old public
queue-only prepare_catalog_composition predicate is retained unchanged;
only the explicitly named prepare_process_graph_composition port admits
process names after the application authenticates the signed graph. Both
ports share the genuine original prepared owner/CAS body. Prepared MQ and Batch
owners remain held through the same physical-store CAS acknowledgement.
Retries, generation changes and rollback retain the original immutable MQ
catalog predicate and runtime data. Reopen compares the retained signed graph
to both genuine selected projections before returning local readiness.
Original count/byte bounds, wrong-store refusal and unknown fencing remain.

This local graph is not an IBM trigger eligibility, initiation message or
automatic launch definition. Runtime MQ results still report the existing
process name and do not start a controller. The original Batch controller
admission still requires its original publication Complete and Controllers
Applied state; no local selection row substitutes for those conditions.
An actual borrowed selected-controller launch/completion owner and a real
trigger occurrence journal are locally feasible subsequent implementation.
They must preserve unknown/duplicate-work retention and original completion
predicates, rather than treating this static graph as executed work.

Offline lookup of pinned IBM MQ 9.4 sources found no verified retained MQ
TOC/body in the configured default cache. The baseline is
ibm-mq-9.4-mqi-2026-08-31. No MQI/HCONN/MQOD/options/native delivery or IBM
semantics change is made here; vendor and licensed workflow evidence remains
separate. Registry/navigation composition must be performed against Root's
actual final parent, preserving ADRs 0063 through 0073.
