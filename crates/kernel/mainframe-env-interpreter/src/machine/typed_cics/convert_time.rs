use super::{LayoutCategory, LayoutMetadata, MachineProblem, invalid_plan, names::SlotUse};

pub(super) fn validate_date_string_slot(
    layout: &LayoutMetadata,
    slot_use: SlotUse,
) -> Result<(), MachineProblem> {
    if matches!(slot_use, SlotUse::DateStringInput)
        && (layout.length != 64
            || !matches!(
                layout.category,
                LayoutCategory::Alphabetic | LayoutCategory::Alphanumeric
            ))
    {
        return Err(invalid_plan("CONVERTTIME DATESTRING must be 64 characters"));
    }
    Ok(())
}
