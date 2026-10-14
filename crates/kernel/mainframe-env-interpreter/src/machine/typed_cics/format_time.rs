use super::names::{SlotUse, output_slot_use};
use mainframe_env_ir::{CicsEffectPlan, CicsOperandName, CicsOutputName, CicsPlanOption};

pub(super) fn plan_output_slot_use(plan: &CicsEffectPlan, name: CicsOutputName) -> SlotUse {
    let slot_use = output_slot_use(name);
    let SlotUse::FormatTextOutput(field, _) = slot_use else {
        return slot_use;
    };
    let (operand, option) = if name == CicsOutputName::Time {
        (CicsOperandName::TimeSep, CicsPlanOption::TimeSep)
    } else {
        (CicsOperandName::DateSep, CicsPlanOption::DateSep)
    };
    let separated =
        plan.options.contains(&option) || plan.operands.iter().any(|value| value.name == operand);
    let separator_count = if name == CicsOutputName::Yyddd { 1 } else { 2 };
    SlotUse::FormatTextOutput(field, (!separated).then_some(field - separator_count))
}
