//! Existing mandatory case closure, with explicitly pending obligations.
use super::*;

pub(super) fn pending_reason<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    String::deserialize(deserializer).map(Some)
}

pub(super) fn check<'a>(
    obligations: impl Iterator<Item = &'a MandatoryObligation>,
    cases: &BTreeMap<BindingKey, ConformanceCase>,
) -> Result<(), SpecProblem> {
    for obligation in obligations {
        for gate in &obligation.applicable_gates {
            let key = BindingKey {
                row_id: obligation.row_id.clone(),
                obligation_id: obligation.obligation_id.clone(),
                gate: *gate,
            };
            if obligation.pending_reason.is_some() && cases.contains_key(&key) {
                return Err(SpecProblem::IncompatibleGate(format!(
                    "pending obligation cannot have an executable binding: {}",
                    format_binding(&key)
                )));
            }
            if obligation.pending_reason.is_none() && !cases.contains_key(&key) {
                return Err(SpecProblem::MissingBinding(format_binding(&key)));
            }
        }
    }
    Ok(())
}
