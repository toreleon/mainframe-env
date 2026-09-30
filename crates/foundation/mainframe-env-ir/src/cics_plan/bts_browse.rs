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
    Container,
    Channel,
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
            966 => Some(Self::Container),
            967 => Some(Self::Channel),
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
            Self::Container => "CONTAINER",
            Self::Channel => "CHANNEL",
        }
    }

    pub const fn width(self) -> usize {
        match self {
            Self::ActivityId => 52,
            Self::Process => 36,
            Self::ProcessType => 8,
            Self::BrowseToken => 4,
            Self::Event | Self::Timer | Self::Container | Self::Channel => 16,
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
    Container,
    DataLength,
    Set,
    CompStatus,
    Mode,
    SuspStatus,
    EventType,
    FireStatus,
    Composite,
    Predicate,
    Timer,
    Status,
    Abstime,
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
            1028 => Some(Self::Container),
            1029 => Some(Self::DataLength),
            1030 => Some(Self::Set),
            1031 => Some(Self::CompStatus),
            1032 => Some(Self::Mode),
            1033 => Some(Self::SuspStatus),
            1034 => Some(Self::EventType),
            1035 => Some(Self::FireStatus),
            1036 => Some(Self::Composite),
            1037 => Some(Self::Predicate),
            1038 => Some(Self::Timer),
            1039 => Some(Self::Status),
            1040 => Some(Self::Abstime),
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
            Self::Container => "CONTAINER",
            Self::DataLength => "DATALENGTH",
            Self::Set => "SET",
            Self::CompStatus => "COMPSTATUS",
            Self::Mode => "MODE",
            Self::SuspStatus => "SUSPSTATUS",
            Self::EventType => "EVENTTYPE",
            Self::FireStatus => "FIRESTATUS",
            Self::Composite => "COMPOSITE",
            Self::Predicate => "PREDICATE",
            Self::Timer => "TIMER",
            Self::Status => "STATUS",
            Self::Abstime => "ABSTIME",
        }
    }

    pub const fn width(self) -> usize {
        match self {
            Self::BrowseToken
            | Self::Level
            | Self::DataLength
            | Self::Set
            | Self::CompStatus
            | Self::Mode
            | Self::SuspStatus
            | Self::EventType
            | Self::FireStatus
            | Self::Predicate
            | Self::Status => 4,
            Self::Abstime => 8,
            Self::Activity => 16,
            Self::ActivityId => 52,
            Self::Process => 36,
            Self::Abcode => 4,
            Self::Abprogram | Self::ProcessType | Self::Program | Self::UserId => 8,
            Self::Event | Self::Container | Self::Composite | Self::Timer => 16,
            Self::TransId => 4,
        }
    }
}

pub(super) const fn is_operation(operation: CicsPlanOperation) -> bool {
    matches!(
        operation,
        CicsPlanOperation::BtsEndBrowseContainer
            | CicsPlanOperation::BtsGetNextContainer
            | CicsPlanOperation::BtsInquireContainer
            | CicsPlanOperation::BtsStartBrowseContainer
            | CicsPlanOperation::BtsStartBrowseActivity
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
        P::BtsStartBrowseContainer => (
            &[I::ActivityId, I::Process, I::ProcessType, I::Channel],
            &[],
            &[O::BrowseToken],
            &[O::BrowseToken],
        ),
        P::BtsGetNextContainer => (
            &[I::BrowseToken],
            &[I::BrowseToken],
            &[O::Container],
            &[O::Container],
        ),
        P::BtsEndBrowseContainer => (&[I::BrowseToken], &[I::BrowseToken], &[], &[]),
        P::BtsInquireContainer => (
            &[I::Container, I::ActivityId, I::Process, I::ProcessType],
            &[I::Container],
            &[O::DataLength, O::Set],
            &[],
        ),
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
            &[
                O::Event,
                O::EventType,
                O::FireStatus,
                O::Composite,
                O::Predicate,
                O::Timer,
            ],
            &[O::Event],
        ),
        P::BtsEndBrowseEvent | P::BtsEndBrowseTimer => {
            (&[I::BrowseToken], &[I::BrowseToken], &[], &[])
        }
        P::BtsInquireEvent => (
            &[I::ActivityId, I::Event],
            &[I::Event],
            &[
                O::EventType,
                O::FireStatus,
                O::Composite,
                O::Predicate,
                O::Timer,
            ],
            &[],
        ),
        P::BtsInquireTimer => (
            &[I::ActivityId, I::Timer],
            &[I::Timer],
            &[O::Event, O::Status, O::Abstime],
            &[],
        ),
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
                O::CompStatus,
                O::Mode,
                O::SuspStatus,
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
                    I::ActivityId
                    | I::Process
                    | I::ProcessType
                    | I::Event
                    | I::Timer
                    | I::Container
                    | I::Channel,
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
        || plan.operation == P::BtsInquireActivity
            && actual_outputs
                .iter()
                .any(|name| matches!(name, O::CompStatus | O::Mode | O::SuspStatus))
            && actual_outputs.len() != 1
        || plan.operation == P::BtsGetNextEvent
            && actual_outputs
                .iter()
                .filter(|name| **name != O::Event)
                .count()
                > 1
        || plan.operation == P::BtsStartBrowseActivity
            && (actual_inputs.contains(&I::Process) != actual_inputs.contains(&I::ProcessType)
                || actual_inputs.contains(&I::Process) && actual_inputs.contains(&I::ActivityId))
        || matches!(
            plan.operation,
            P::BtsStartBrowseContainer | P::BtsInquireContainer
        ) && (actual_inputs.contains(&I::Process) != actual_inputs.contains(&I::ProcessType)
            || actual_inputs.contains(&I::Process) && actual_inputs.contains(&I::ActivityId)
            || actual_inputs.contains(&I::Channel)
                && (actual_inputs.contains(&I::Process) || actual_inputs.contains(&I::ActivityId)))
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
        assert!(BtsBrowseInput::from_tag(968).is_none());
        assert!(BtsBrowseOutput::from_tag(1041).is_none());
        let mut future_tag = encode_cics_effect_plan(
            &plan(P::BtsEndBrowseActivity, &[I::BrowseToken], &[]),
            CicsPlanLimits::default(),
        )
        .unwrap();
        future_tag[6..8].copy_from_slice(&205u16.to_be_bytes());
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

    #[test]
    fn bts_browse_inquiry_output_tags_roundtrip_and_ambiguous_activity_is_fenced() {
        use BtsBrowseInput as I;
        use BtsBrowseOutput as O;
        use CicsPlanOperation as P;
        for (operation, input, fields) in [
            (
                P::BtsInquireActivity,
                I::ActivityId,
                vec![O::CompStatus, O::Mode, O::SuspStatus],
            ),
            (
                P::BtsInquireEvent,
                I::Event,
                vec![
                    O::EventType,
                    O::FireStatus,
                    O::Composite,
                    O::Predicate,
                    O::Timer,
                ],
            ),
            (
                P::BtsInquireTimer,
                I::Timer,
                vec![O::Event, O::Status, O::Abstime],
            ),
        ] {
            for field in fields {
                let valid = plan(operation, &[input], &[field]);
                let bytes = encode_cics_effect_plan(&valid, CicsPlanLimits::default()).unwrap();
                assert_eq!(
                    decode_cics_effect_plan(&bytes, CicsPlanLimits::default()),
                    Ok(valid.clone())
                );
                assert_eq!(O::from_tag(field.tag()), Some(field));
                if operation == P::BtsInquireActivity {
                    let ambiguous = plan(operation, &[input], &[field, O::Activity]);
                    assert!(
                        encode_cics_effect_plan(&ambiguous, CicsPlanLimits::default()).is_err()
                    );
                }
            }
        }
        assert!(
            encode_cics_effect_plan(
                &plan(
                    P::BtsInquireTimer,
                    &[I::Timer],
                    &[O::Event, O::Status, O::Abstime]
                ),
                CicsPlanLimits::default()
            )
            .is_ok()
        );
    }

    #[test]
    fn bts_getnext_event_metadata_is_individual() {
        use BtsBrowseInput as I;
        use BtsBrowseOutput as O;
        use CicsPlanOperation as P;
        for field in [
            O::EventType,
            O::FireStatus,
            O::Composite,
            O::Predicate,
            O::Timer,
        ] {
            let individual = plan(P::BtsGetNextEvent, &[I::BrowseToken], &[O::Event, field]);
            assert!(encode_cics_effect_plan(&individual, CicsPlanLimits::default()).is_ok());
        }
        let combined = plan(
            P::BtsGetNextEvent,
            &[I::BrowseToken],
            &[O::Event, O::EventType, O::FireStatus],
        );
        assert!(encode_cics_effect_plan(&combined, CicsPlanLimits::default()).is_err());
    }

    #[test]
    fn bts_browse_container_reserved_operation_tags() {
        use CicsPlanOperation as P;
        for (operation, tag) in [
            (P::BtsEndBrowseContainer, 197),
            (P::BtsGetNextContainer, 202),
            (P::BtsInquireContainer, 207),
            (P::BtsStartBrowseContainer, 212),
        ] {
            assert_eq!(super::super::codec_tags::operation_tag(operation), tag);
            assert_eq!(
                super::super::codec_tags::operation_from_tag(tag).unwrap(),
                operation
            );
        }
        use BtsBrowseInput as I;
        use BtsBrowseOutput as O;
        for (operation, inputs, outputs) in [
            (P::BtsEndBrowseContainer, vec![I::BrowseToken], vec![]),
            (
                P::BtsGetNextContainer,
                vec![I::BrowseToken],
                vec![O::Container],
            ),
            (
                P::BtsInquireContainer,
                vec![I::Container],
                vec![O::DataLength, O::Set],
            ),
            (
                P::BtsStartBrowseContainer,
                vec![I::Channel],
                vec![O::BrowseToken],
            ),
        ] {
            let valid = plan(operation, &inputs, &outputs);
            let bytes = encode_cics_effect_plan(&valid, CicsPlanLimits::default()).unwrap();
            assert_eq!(
                decode_cics_effect_plan(&bytes, CicsPlanLimits::default()).unwrap(),
                valid
            );
            let mut invalid = valid.clone();
            invalid.outputs.push(CicsOutputBinding {
                name: CicsOutputName::BtsBrowse(O::Event),
                target: slot(29),
            });
            assert!(encode_cics_effect_plan(&invalid, CicsPlanLimits::default()).is_err());
        }
    }
}
