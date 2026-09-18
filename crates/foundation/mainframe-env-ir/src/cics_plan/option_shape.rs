use super::{CicsEffectPlan, CicsPlanOperation, CicsPlanOption};

pub(super) fn has_unsupported(plan: &CicsEffectPlan) -> bool {
    plan.options.iter().any(|option| match plan.operation {
        CicsPlanOperation::FormatTime => !matches!(
            option,
            CicsPlanOption::NoHandle | CicsPlanOption::DateSep | CicsPlanOption::TimeSep
        ),
        CicsPlanOperation::Start => !matches!(
            option,
            CicsPlanOption::NoHandle | CicsPlanOption::Fmh | CicsPlanOption::Protect
        ),
        CicsPlanOperation::Retrieve => {
            !matches!(option, CicsPlanOption::NoHandle | CicsPlanOption::Wait)
        }
        _ => !matches!(option, CicsPlanOption::NoHandle),
    })
}
