use super::{CicsEffectPlan, CicsPlanOperation, CicsPlanOption};

pub(super) fn has_unsupported(plan: &CicsEffectPlan) -> bool {
    plan.options.iter().any(|option| match plan.operation {
        CicsPlanOperation::BifDigest => !matches!(
            option,
            CicsPlanOption::NoHandle
                | CicsPlanOption::DigestHex
                | CicsPlanOption::DigestBinary
                | CicsPlanOption::DigestBase64
        ),
        CicsPlanOperation::Post => !matches!(
            option,
            CicsPlanOption::NoHandle | CicsPlanOption::After | CicsPlanOption::At
        ),
        CicsPlanOperation::WriteOperator => !matches!(
            option,
            CicsPlanOption::NoHandle
                | CicsPlanOption::OperatorImmediate
                | CicsPlanOption::OperatorEventual
                | CicsPlanOption::OperatorCritical
        ),
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
        CicsPlanOperation::ReceiveMap => {
            !matches!(option, CicsPlanOption::NoHandle | CicsPlanOption::Terminal)
        }
        CicsPlanOperation::InvokeApplication => !matches!(
            option,
            CicsPlanOption::NoHandle | CicsPlanOption::ExactMatch | CicsPlanOption::Minimum
        ),
        CicsPlanOperation::Load => {
            !matches!(option, CicsPlanOption::NoHandle | CicsPlanOption::Hold)
        }
        CicsPlanOperation::SpoolClose => !matches!(
            option,
            CicsPlanOption::NoHandle | CicsPlanOption::SpoolKeep | CicsPlanOption::SpoolDelete
        ),
        CicsPlanOperation::SpoolOpenInput => !matches!(option, CicsPlanOption::NoHandle),
        CicsPlanOperation::SpoolOpenOutput => !matches!(
            option,
            CicsPlanOption::NoHandle
                | CicsPlanOption::SpoolNoCc
                | CicsPlanOption::SpoolAsa
                | CicsPlanOption::SpoolMcc
                | CicsPlanOption::SpoolPrint
                | CicsPlanOption::SpoolPunch
        ),
        CicsPlanOperation::SpoolRead => !matches!(option, CicsPlanOption::NoHandle),
        CicsPlanOperation::SpoolWrite => !matches!(
            option,
            CicsPlanOption::NoHandle | CicsPlanOption::SpoolLine | CicsPlanOption::SpoolPage
        ),
        CicsPlanOperation::Read => !matches!(
            option,
            CicsPlanOption::Generic
                | CicsPlanOption::Gteq
                | CicsPlanOption::Equal
                | CicsPlanOption::NoHandle
                | CicsPlanOption::Update
        ),
        CicsPlanOperation::DocumentCreate => {
            !matches!(option, CicsPlanOption::NoHandle | CicsPlanOption::Unescaped)
        }
        CicsPlanOperation::DocumentRetrieve => !matches!(
            option,
            CicsPlanOption::NoHandle | CicsPlanOption::DocumentDataOnly
        ),
        CicsPlanOperation::DocumentSet => {
            !matches!(option, CicsPlanOption::NoHandle | CicsPlanOption::Unescaped)
        }
        CicsPlanOperation::TransformDataToJson
        | CicsPlanOperation::TransformDataToXml
        | CicsPlanOperation::TransformJsonToData
        | CicsPlanOperation::TransformXmlToData => !matches!(option, CicsPlanOption::NoHandle),
        _ => !matches!(option, CicsPlanOption::NoHandle),
    })
}
