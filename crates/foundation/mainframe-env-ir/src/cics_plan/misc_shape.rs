//! Shape checks for the miscellaneous CICS command rows.

use super::*;

pub(super) fn invalid_convert_time_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
    scheduling_options: bool,
) -> bool {
    inputs != &BTreeSet::from([CicsOperandName::DateString])
        || !plan
            .operands
            .iter()
            .all(|operand| matches!(operand.value, CicsOperandValue::Storage(_)))
        || !outputs.contains(&CicsOutputName::Abstime)
        || scheduling_options
}

pub(super) fn invalid_format_time_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    _outputs: &BTreeSet<CicsOutputName>,
    scheduling_options: bool,
) -> bool {
    let allowed_inputs = BTreeSet::from([
        CicsOperandName::Abstime,
        CicsOperandName::DateSep,
        CicsOperandName::TimeSep,
    ]);
    !inputs.contains(&CicsOperandName::Abstime)
        || !inputs.is_subset(&allowed_inputs)
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::Abstime => !matches!(operand.value, CicsOperandValue::Storage(_)),
            CicsOperandName::DateSep | CicsOperandName::TimeSep => !matches!(
                operand.value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            ),
            _ => true,
        })
        || scheduling_options
}

pub(super) fn invalid_bif_deedit_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    _outputs: &BTreeSet<CicsOutputName>,
    scheduling_options: bool,
) -> bool {
    let field = plan
        .operands
        .iter()
        .find(|operand| operand.name == CicsOperandName::Field);
    let result = plan
        .outputs
        .iter()
        .find(|output| output.name == CicsOutputName::Field);
    !inputs.contains(&CicsOperandName::Field)
        || !inputs.is_subset(&BTreeSet::from([
            CicsOperandName::Field,
            CicsOperandName::Length,
        ]))
        || !matches!(
            (field.map(|operand| &operand.value), result),
            (Some(CicsOperandValue::Storage(slot)), Some(binding)) if slot == &binding.target
        )
        || scheduling_options
}

pub(super) fn invalid_bif_digest_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
    scheduling_options: bool,
) -> bool {
    let selectors = [
        CicsPlanOption::DigestHex,
        CicsPlanOption::DigestBinary,
        CicsPlanOption::DigestBase64,
    ]
    .into_iter()
    .filter(|selector| plan.options.contains(selector))
    .count();
    let record = plan
        .operands
        .iter()
        .find(|operand| operand.name == CicsOperandName::Record);
    let record_length = plan
        .operands
        .iter()
        .find(|operand| operand.name == CicsOperandName::RecordLength);
    let digest_type = plan
        .operands
        .iter()
        .find(|operand| operand.name == CicsOperandName::DigestType);
    !inputs.contains(&CicsOperandName::Record)
        || !inputs.contains(&CicsOperandName::RecordLength)
        || !inputs.is_subset(&BTreeSet::from([
            CicsOperandName::Record,
            CicsOperandName::RecordLength,
            CicsOperandName::DigestType,
        ]))
        || !matches!(
            record.map(|operand| &operand.value),
            Some(CicsOperandValue::Storage(_) | CicsOperandValue::Literal(_))
        )
        || !matches!(
            record_length.map(|operand| &operand.value),
            Some(
                CicsOperandValue::Integer(_)
                    | CicsOperandValue::Storage(_)
                    | CicsOperandValue::LengthOf(_)
            )
        )
        || digest_type.is_some_and(|operand| {
            !matches!(
                &operand.value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            )
        })
        || selectors + usize::from(digest_type.is_some()) != 1
        || !outputs.contains(&CicsOutputName::DigestResult)
        || scheduling_options
}

pub(super) fn invalid_extract_certificate_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
    scheduling_options: bool,
) -> bool {
    !inputs.is_empty()
        || !outputs.contains(&CicsOutputName::Certificate(
            CicsCertificateOutput::Certificate,
        ))
        || plan.options.contains(&CicsPlanOption::CertificateOwner)
            && plan.options.contains(&CicsPlanOption::CertificateIssuer)
        || scheduling_options
}

pub(super) fn invalid_extract_tcpip_shape(
    _plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
    scheduling_options: bool,
) -> bool {
    !inputs.is_empty()
        || !outputs
            .iter()
            .any(|output| matches!(output, CicsOutputName::Tcpip(_)))
        || scheduling_options
}
