use super::*;

pub(super) fn validate_slot(
    layout: &LayoutMetadata,
    slot_use: SlotUse,
) -> Result<(), MachineProblem> {
    if matches!(slot_use, SlotUse::SpoolTokenInput)
        && (layout.length != 8
            || !matches!(
                layout.category,
                LayoutCategory::Alphabetic | LayoutCategory::Alphanumeric
            ))
    {
        return Err(invalid_plan(
            "CICS spool TOKEN input must be an 8-character field",
        ));
    }
    if matches!(slot_use, SlotUse::SpoolTokenOutput)
        && (layout.length != 8
            || !matches!(
                layout.category,
                LayoutCategory::Alphabetic | LayoutCategory::Alphanumeric
            ))
    {
        return Err(invalid_plan(
            "CICS SPOOLOPEN TOKEN output must be an 8-character field",
        ));
    }
    if matches!(slot_use, SlotUse::SpoolUserIdInput)
        && (layout.length != 8
            || !matches!(
                layout.category,
                LayoutCategory::Alphabetic | LayoutCategory::Alphanumeric
            ))
    {
        return Err(invalid_plan(
            "CICS spool USERID input must be an 8-character field",
        ));
    }
    if matches!(slot_use, SlotUse::SpoolClassInput)
        && (layout.length != 1
            || !matches!(
                layout.category,
                LayoutCategory::Alphabetic | LayoutCategory::Alphanumeric
            ))
    {
        return Err(invalid_plan("CICS spool CLASS input must be one character"));
    }
    Ok(())
}
