//! Finite MQ foundation route through the shared runner and derived ledger.
use super::*;

pub(super) fn run_focused_mq(root: &Path, args: &ConformanceArgs) -> TaskResult {
    let limits = ConformanceLimits::default();
    let spec = compile_shared_spec(root)?;
    let (gate, local) = parse_focused_gate(args.gate.as_deref())?;
    let selection = if let Some(id) = args.replay.as_deref() {
        RunnerSelection::replay(id, limits).map_err(|e| e.to_string())?
    } else {
        make_focused_selection("mq", gate, local, args.shard, limits)?
    };
    let dataset = dataset_conformance_runtime();
    let jcl = jcl_conformance::runtime();
    let cics = cics_pilot_runtime();
    let movement = cobol_move_pilot_runtime();
    let arithmetic = cobol_arithmetic_pilot_runtime();
    let runtime =
        combined_conformance_runtime(&spec, &dataset, &jcl, &cics, &movement, &arithmetic, limits)?;
    let context =
        RunnerContext::new(candidate_digest(root)?, "local", limits).map_err(|e| e.to_string())?;
    let report = ConformanceRunner::new(&spec, runtime, limits)
        .run(&selection, &context)
        .map_err(|e| e.to_string())?;
    let events = report
        .batches
        .iter()
        .flat_map(|b| &b.events)
        .collect::<Vec<_>>();
    require(
        !events.is_empty(),
        "MQ gate has no executable evidence; mandatory obligations remain pending",
    )?;
    conformance_output::emit_report(&report, args.output.as_deref())?;
    require(
        events.iter().all(|e| e.verdict == Verdict::Pass),
        "MQ selected driver produced failing evidence",
    )?;
    let ledger = &report.ledger;
    let rows = ledger
        .rows
        .values()
        .filter(|r| r.subsystem == "mq")
        .collect::<Vec<_>>();
    require(rows.len() == 26, "MQ unique-row denominator changed")?;
    if args.gate.is_none() && args.replay.is_none() {
        require(
            rows.iter().all(|r| {
                r.gates
                    .values()
                    .all(|g| g.state == mainframe_env_coverage::GateState::Passed)
            }),
            "MQ full26 completion refused: complete applicable profiles/security/recovery/installed evidence remain pending",
        )?;
    }
    conformance_output::diagnostic(format_args!(
        "mq-foundation: finite selected verdicts only; full26 rows pending; no installed/native/licensed credit"
    ))?;
    Ok(())
}
