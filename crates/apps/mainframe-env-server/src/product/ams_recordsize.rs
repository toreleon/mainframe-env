use super::{GatewayProblem, HostProblem, control_name, gateway_problem};

pub(super) fn define_layout(text: &str, words: &[&str]) -> Result<(String, u32), GatewayProblem> {
    let name = control_name(text, "NAME").ok_or_else(|| gateway_problem(HostProblem::Malformed))?;
    // #273: IDCAMS RECORDSIZE is (average, maximum), default (4089, 4089).
    let ksds_cluster = words.get(1).is_some_and(|word| {
        word.split('(')
            .next()
            .is_some_and(|kind| kind.eq_ignore_ascii_case("CLUSTER"))
    }) && !words.iter().any(|word| {
        ["NONINDEXED", "NUMBERED", "LINEAR"]
            .iter()
            .any(|kind| word.trim_matches(['(', ')']).eq_ignore_ascii_case(kind))
    });
    let maximum = if !ksds_cluster {
        32760
    } else if let Some(value) = control_name(text, "RECORDSIZE") {
        let sizes = value
            .split(|ch: char| ch == ',' || ch.is_whitespace())
            .filter(|value| !value.is_empty())
            .map(str::parse::<u32>)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| gateway_problem(HostProblem::Malformed))?;
        if sizes.len() != 2 || sizes[0] == 0 || sizes[1] < sizes[0] {
            return Err(gateway_problem(HostProblem::Malformed));
        }
        sizes[1]
    } else if text.to_ascii_uppercase().contains("RECORDSIZE") {
        return Err(gateway_problem(HostProblem::Malformed));
    } else {
        4089
    };
    Ok((name, maximum))
}
