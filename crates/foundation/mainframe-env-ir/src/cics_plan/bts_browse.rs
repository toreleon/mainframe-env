//! Frozen BTS browse field identities and plan shapes.

use super::{
    CicsEffectPlan, CicsOperandName, CicsOperandValue, CicsOutputName, CicsPlanOperation,
    CicsPlanOption,
};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum BtsBrowseInput {
    ActivityId,
    Process,
    ProcessType,
    BrowseToken,
    Event,
    Timer,
}

impl BtsBrowseInput {
    pub const fn tag(self) -> u16 {
        960 + self as u16
    }

    pub const fn from_tag(tag: u16) -> Option<Self> {
        match tag {
            960 => Some(Self::ActivityId),
            961 => Some(Self::Process),
            962 => Some(Self::ProcessType),
            963 => Some(Self::BrowseToken),
            964 => Some(Self::Event),
            965 => Some(Self::Timer),
            _ => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::ActivityId => "ACTIVITYID",
            Self::Process => "PROCESS",
            Self::ProcessType => "PROCESSTYPE",
            Self::BrowseToken => "BROWSETOKEN",
            Self::Event => "EVENT",
            Self::Timer => "TIMER",
        }
    }

    pub const fn width(self) -> usize {
        match self {
            Self::ActivityId => 52,
            Self::Process => 36,
            Self::ProcessType => 8,
            Self::BrowseToken => 4,
            Self::Event | Self::Timer => 16,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum BtsBrowseOutput {
    BrowseToken,
    Activity,
    ActivityId,
    Level,
    Process,
    Abcode,
    Abprogram,
    Event,
    ProcessType,
    Program,
    TransId,
    UserId,
}

impl BtsBrowseOutput {
    pub const fn tag(self) -> u16 {
        1016 + self as u16
    }

    pub const fn from_tag(tag: u16) -> Option<Self> {
        match tag {
            1016 => Some(Self::BrowseToken),
            1017 => Some(Self::Activity),
            1018 => Some(Self::ActivityId),
            1019 => Some(Self::Level),
            1020 => Some(Self::Process),
            1021 => Some(Self::Abcode),
            1022 => Some(Self::Abprogram),
            1023 => Some(Self::Event),
            1024 => Some(Self::ProcessType),
            1025 => Some(Self::Program),
            1026 => Some(Self::TransId),
            1027 => Some(Self::UserId),
            _ => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::BrowseToken => "BROWSETOKEN",
            Self::Activity => "ACTIVITY",
            Self::ActivityId => "ACTIVITYID",
            Self::Level => "LEVEL",
            Self::Process => "PROCESS",
            Self::Abcode => "ABCODE",
            Self::Abprogram => "ABPROGRAM",
            Self::Event => "EVENT",
            Self::ProcessType => "PROCESSTYPE",
            Self::Program => "PROGRAM",
            Self::TransId => "TRANSID",
            Self::UserId => "USERID",
        }
    }

    pub const fn width(self) -> usize {
        match self {
            Self::BrowseToken | Self::Level => 4,
            Self::Activity => 16,
            Self::ActivityId => 52,
            Self::Process => 36,
            Self::Abcode => 4,
            Self::Abprogram | Self::ProcessType | Self::Program | Self::UserId => 8,
            Self::Event => 16,
            Self::TransId => 4,
        }
    }
}

pub(super) const fn is_operation(operation: CicsPlanOperation) -> bool {
    matches!(
        operation,
        CicsPlanOperation::BtsStartBrowseActivity
            | CicsPlanOperation::BtsEndBrowseEvent
            | CicsPlanOperation::BtsGetNextEvent
            | CicsPlanOperation::BtsInquireEvent
            | CicsPlanOperation::BtsStartBrowseEvent
            | CicsPlanOperation::BtsEndBrowseTimer
            | CicsPlanOperation::BtsInquireTimer
            | CicsPlanOperation::BtsStartBrowseTimer
            | CicsPlanOperation::BtsGetNextActivity
            | CicsPlanOperation::BtsEndBrowseActivity
            | CicsPlanOperation::BtsInquireActivity
            | CicsPlanOperation::BtsStartBrowseProcess
            | CicsPlanOperation::BtsGetNextProcess
            | CicsPlanOperation::BtsEndBrowseProcess
            | CicsPlanOperation::BtsInquireProcess
    )
}

pub(super) fn invalid_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    use BtsBrowseInput as I;
    use BtsBrowseOutput as O;
    use CicsPlanOperation as P;
    let (allowed_inputs, required_inputs, allowed_outputs, required_outputs): (
        &[I],
        &[I],
        &[O],
        &[O],
    ) = match plan.operation {
        P::BtsStartBrowseEvent => (&[I::ActivityId], &[], &[O::BrowseToken], &[O::BrowseToken]),
        P::BtsStartBrowseTimer => (
            &[I::ActivityId, I::Timer],
            &[I::Timer],
            &[O::BrowseToken],
            &[O::BrowseToken],
        ),
        P::BtsGetNextEvent => (
            &[I::BrowseToken],
            &[I::BrowseToken],
            &[O::Event],
            &[O::Event],
        ),
        P::BtsEndBrowseEvent | P::BtsEndBrowseTimer => {
            (&[I::BrowseToken], &[I::BrowseToken], &[], &[])
        }
        P::BtsInquireEvent => (&[I::ActivityId, I::Event], &[I::Event], &[], &[]),
        P::BtsInquireTimer => (&[I::ActivityId, I::Timer], &[I::Timer], &[], &[]),
        P::BtsStartBrowseActivity => (
            &[I::ActivityId, I::Process, I::ProcessType],
            &[],
            &[O::BrowseToken],
            &[O::BrowseToken],
        ),
        P::BtsStartBrowseProcess => (
            &[I::ProcessType],
            &[I::ProcessType],
            &[O::BrowseToken],
            &[O::BrowseToken],
        ),
        P::BtsGetNextActivity => (
            &[I::BrowseToken],
            &[I::BrowseToken],
            &[O::Activity, O::ActivityId, O::Level],
            &[O::Activity],
        ),
        P::BtsGetNextProcess => (
            &[I::BrowseToken],
            &[I::BrowseToken],
            &[O::Process, O::ActivityId],
            &[O::Process],
        ),
        P::BtsEndBrowseActivity | P::BtsEndBrowseProcess => {
            (&[I::BrowseToken], &[I::BrowseToken], &[], &[])
        }
        P::BtsInquireActivity => (
            &[I::ActivityId],
            &[I::ActivityId],
            &[
                O::Abcode,
                O::Abprogram,
                O::Activity,
                O::Event,
                O::Process,
                O::ProcessType,
                O::Program,
                O::TransId,
                O::UserId,
            ],
            &[],
        ),
        P::BtsInquireProcess => (
            &[I::Process, I::ProcessType],
            &[I::Process, I::ProcessType],
            &[O::ActivityId],
            &[],
        ),
        _ => return true,
    };
    if !plan.options.is_empty() && plan.options != BTreeSet::from([CicsPlanOption::NoHandle]) {
        return true;
    }
    let actual_inputs = inputs
        .iter()
        .filter_map(|name| match name {
            CicsOperandName::BtsBrowse(name) => Some(*name),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let actual_outputs = outputs
        .iter()
        .filter_map(|name| match name {
            CicsOutputName::BtsBrowse(name) => Some(*name),
            CicsOutputName::Resp | CicsOutputName::Resp2 => None,
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    actual_inputs.len() != inputs.len()
        || plan.operands.iter().any(|operand| {
            let CicsOperandName::BtsBrowse(field) = operand.name else {
                return true;
            };
            match (&operand.value, field) {
                (CicsOperandValue::Storage(_), _) => false,
                (CicsOperandValue::Integer(value), I::BrowseToken) => {
                    !(1..=i32::MAX as i64).contains(value)
                }
                (
                    CicsOperandValue::Literal(bytes),
                    I::ActivityId | I::Process | I::ProcessType | I::Event | I::Timer,
                ) => std::str::from_utf8(bytes).map_or(true, |value| {
                    value.is_empty()
                        || value.chars().count() > field.width()
                        || field == I::Event && value.to_ascii_uppercase().starts_with("DFH")
                }),
                _ => true,
            }
        })
        || actual_outputs.len()
            + outputs
                .intersection(&BTreeSet::from([
                    CicsOutputName::Resp,
                    CicsOutputName::Resp2,
                ]))
                .count()
            != outputs.len()
        || actual_inputs
            .iter()
            .any(|name| !allowed_inputs.contains(name))
        || actual_outputs
            .iter()
            .any(|name| !allowed_outputs.contains(name))
        || required_inputs
            .iter()
            .any(|name| !actual_inputs.contains(name))
        || required_outputs
            .iter()
            .any(|name| !actual_outputs.contains(name))
        || plan.operation == P::BtsStartBrowseActivity
            && (actual_inputs.contains(&I::Process) != actual_inputs.contains(&I::ProcessType)
                || actual_inputs.contains(&I::Process) && actual_inputs.contains(&I::ActivityId))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CicsCondition, CicsNamedOperand, CicsOperandValue, CicsOutputBinding, CicsPlanLimits,
        CicsStorageSlot, StorageId, decode_cics_effect_plan, encode_cics_effect_plan,
    };

    fn slot(index: usize) -> CicsStorageSlot {
        CicsStorageSlot {
            storage: StorageId::from_index(index).unwrap(),
            qualified_layout_name: format!("BTS-BROWSE-{index}"),
        }
    }

    fn plan(
        operation: CicsPlanOperation,
        inputs: &[BtsBrowseInput],
        outputs: &[BtsBrowseOutput],
    ) -> CicsEffectPlan {
        CicsEffectPlan {
            operation,
            operands: inputs
                .iter()
                .enumerate()
                .map(|(index, name)| CicsNamedOperand {
                    name: CicsOperandName::BtsBrowse(*name),
                    value: CicsOperandValue::Storage(slot(index)),
                })
                .collect(),
            options: BTreeSet::new(),
            outputs: outputs
                .iter()
                .enumerate()
                .map(|(index, name)| CicsOutputBinding {
                    name: CicsOutputName::BtsBrowse(*name),
                    target: slot(index + 8),
                })
                .collect(),
            condition: CicsCondition::Default,
        }
    }

    #[test]
    fn bts_browse_all_eight_minimal_plans_roundtrip_and_reject_cross_resource_fields() {
        use BtsBrowseInput as I;
        use BtsBrowseOutput as O;
        use CicsPlanOperation as P;
        for (operation, inputs, outputs) in [
            (P::BtsStartBrowseActivity, vec![], vec![O::BrowseToken]),
            (
                P::BtsGetNextActivity,
                vec![I::BrowseToken],
                vec![O::Activity],
            ),
            (P::BtsEndBrowseActivity, vec![I::BrowseToken], vec![]),
            (P::BtsInquireActivity, vec![I::ActivityId], vec![]),
            (
                P::BtsStartBrowseProcess,
                vec![I::ProcessType],
                vec![O::BrowseToken],
            ),
            (P::BtsGetNextProcess, vec![I::BrowseToken], vec![O::Process]),
            (P::BtsEndBrowseProcess, vec![I::BrowseToken], vec![]),
            (
                P::BtsInquireProcess,
                vec![I::Process, I::ProcessType],
                vec![],
            ),
        ] {
            let valid = plan(operation, &inputs, &outputs);
            let encoded = encode_cics_effect_plan(&valid, CicsPlanLimits::default())
                .unwrap_or_else(|error| panic!("{operation:?}: {error:?}"));
            assert!(
                super::super::encode_cics_effect_plan_version(
                    &valid,
                    CicsPlanLimits::default(),
                    super::super::LEGACY_VERSION,
                )
                .is_err()
            );
            assert_eq!(
                decode_cics_effect_plan(&encoded, CicsPlanLimits::default()).unwrap(),
                valid
            );
            let mut invalid = valid.clone();
            invalid.outputs.push(CicsOutputBinding {
                name: CicsOutputName::BtsBrowse(O::UserId),
                target: slot(20),
            });
            if operation != P::BtsInquireActivity {
                assert!(encode_cics_effect_plan(&invalid, CicsPlanLimits::default()).is_err());
            }
        }
        assert!(BtsBrowseInput::from_tag(966).is_none());
        assert!(BtsBrowseOutput::from_tag(1028).is_none());
        let mut future_tag = encode_cics_effect_plan(
            &plan(P::BtsEndBrowseActivity, &[I::BrowseToken], &[]),
            CicsPlanLimits::default(),
        )
        .unwrap();
        future_tag[6..8].copy_from_slice(&197u16.to_be_bytes());
        assert!(decode_cics_effect_plan(&future_tag, CicsPlanLimits::default()).is_err());
        let mut bad_token = plan(P::BtsGetNextActivity, &[I::BrowseToken], &[O::Activity]);
        bad_token.operands[0].value = CicsOperandValue::Literal(b"1".to_vec());
        assert!(encode_cics_effect_plan(&bad_token, CicsPlanLimits::default()).is_err());
    }

    #[test]
    fn bts_browse_event_timer_tags_are_reserved_without_getnext_timer() {
        assert_eq!(
            super::super::codec_tags::operation_tag(CicsPlanOperation::BtsEndBrowseEvent),
            198
        );
        assert_eq!(
            super::super::codec_tags::operation_tag(CicsPlanOperation::BtsStartBrowseTimer),
            215
        );
        assert!(super::super::codec_tags::operation_from_tag(205).is_err());
        use BtsBrowseInput as I;
        use BtsBrowseOutput as O;
        use CicsPlanOperation as P;
        for (operation, inputs, outputs) in [
            (P::BtsEndBrowseEvent, vec![I::BrowseToken], vec![]),
            (P::BtsGetNextEvent, vec![I::BrowseToken], vec![O::Event]),
            (P::BtsInquireEvent, vec![I::Event], vec![]),
            (P::BtsStartBrowseEvent, vec![], vec![O::BrowseToken]),
            (P::BtsEndBrowseTimer, vec![I::BrowseToken], vec![]),
            (P::BtsInquireTimer, vec![I::Timer], vec![]),
            (P::BtsStartBrowseTimer, vec![I::Timer], vec![O::BrowseToken]),
        ] {
            let valid = plan(operation, &inputs, &outputs);
            let encoded = encode_cics_effect_plan(&valid, CicsPlanLimits::default())
                .unwrap_or_else(|error| panic!("{operation:?}: {error:?}"));
            assert_eq!(
                decode_cics_effect_plan(&encoded, CicsPlanLimits::default()).unwrap(),
                valid
            );
            let mut bad = valid.clone();
            bad.outputs.push(CicsOutputBinding {
                name: CicsOutputName::BtsBrowse(O::UserId),
                target: slot(24),
            });
            assert!(encode_cics_effect_plan(&bad, CicsPlanLimits::default()).is_err());
        }
        let mut system = plan(P::BtsInquireEvent, &[I::Event], &[]);
        system.operands[0].value = CicsOperandValue::Literal(b"DFHINITIAL".to_vec());
        assert!(encode_cics_effect_plan(&system, CicsPlanLimits::default()).is_err());
    }
}
