use super::*;

pub(super) fn validate_condition(
    plan: &CicsEffectPlan,
    limits: CicsPlanLimits,
) -> Result<(), CicsPlanCodecProblem> {
    let response = output_target(&plan.outputs, CicsOutputName::Resp);
    let response2 = output_target(&plan.outputs, CicsOutputName::Resp2);
    let no_handle = plan.options.contains(&CicsPlanOption::NoHandle);
    match &plan.condition {
        CicsCondition::Default if !no_handle && response.is_none() => Ok(()),
        CicsCondition::NoHandle if no_handle => Ok(()),
        CicsCondition::Respond {
            response: expected,
            response2: expected2,
        } if !no_handle && response == Some(expected) && response2 == expected2.as_ref() => {
            validate_slot(expected, limits)?;
            if let Some(expected2) = expected2 {
                validate_slot(expected2, limits)?;
            }
            Ok(())
        }
        _ => Err(CicsPlanCodecProblem::Malformed),
    }
}
