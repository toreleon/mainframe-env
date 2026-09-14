use super::{CicsOutputName, CicsPlanOperation};

pub(super) const fn allowed(operation: CicsPlanOperation) -> &'static [CicsOutputName] {
    match operation {
        CicsPlanOperation::Asktime => &[
            CicsOutputName::Abstime,
            CicsOutputName::Resp,
            CicsOutputName::Resp2,
        ],
        CicsPlanOperation::Read => &[
            CicsOutputName::Into,
            CicsOutputName::Resp,
            CicsOutputName::Resp2,
        ],
        CicsPlanOperation::FormatTime => &[
            CicsOutputName::Milliseconds,
            CicsOutputName::Mmddyy,
            CicsOutputName::Mmddyyyy,
            CicsOutputName::Time,
            CicsOutputName::Yyddd,
            CicsOutputName::Yymmdd,
            CicsOutputName::Yyyymmdd,
            CicsOutputName::Resp,
            CicsOutputName::Resp2,
        ],
        CicsPlanOperation::Link => &[
            CicsOutputName::Commarea,
            CicsOutputName::Resp,
            CicsOutputName::Resp2,
        ],
        _ => &[CicsOutputName::Resp, CicsOutputName::Resp2],
    }
}
