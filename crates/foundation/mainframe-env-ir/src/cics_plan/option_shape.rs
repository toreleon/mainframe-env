use super::{CicsEffectPlan, CicsPlanOperation, CicsPlanOption};

pub(super) fn has_unsupported(plan: &CicsEffectPlan) -> bool {
    plan.options.iter().any(|option| match plan.operation {
        CicsPlanOperation::FormatTime => !matches!(
            option,
            CicsPlanOption::NoHandle | CicsPlanOption::DateSep | CicsPlanOption::TimeSep
        ),
        CicsPlanOperation::Start => !matches!(
            option,
            CicsPlanOption::NoHandle
                | CicsPlanOption::Fmh
                | CicsPlanOption::Protect
                | CicsPlanOption::After
                | CicsPlanOption::At
                | CicsPlanOption::NoCheck
        ),
        CicsPlanOperation::Delay => !matches!(
            option,
            CicsPlanOption::NoHandle | CicsPlanOption::For | CicsPlanOption::Until
        ),
        CicsPlanOperation::Retrieve => {
            !matches!(option, CicsPlanOption::NoHandle | CicsPlanOption::Wait)
        }
        CicsPlanOperation::Read => !matches!(
            option,
            CicsPlanOption::Gteq | CicsPlanOption::NoHandle | CicsPlanOption::Update
        ),
        _ => !matches!(option, CicsPlanOption::NoHandle),
    })
}
