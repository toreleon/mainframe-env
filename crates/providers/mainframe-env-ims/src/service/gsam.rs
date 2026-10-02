//! Additive replay output; historical receipts omit this field unchanged.
use super::*;
use mainframe_env_host_api::{ImsGsamAddress, ImsGsamRequest, ImsGsamResult};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReplayOutput {
    pub(crate) address: Option<ImsGsamAddress>,
}

impl ImsService {
    pub fn execute_gsam(
        &self,
        invocation: &Invocation,
        request: &ImsGsamRequest,
    ) -> Result<ImsGsamResult, HostProblem> {
        self.execute_operands_at(
            invocation,
            &request.request,
            invocation.deadline_tick,
            None,
            Some(request),
        )
    }
}

impl RecordedResult {
    pub(crate) fn host_result(&self) -> HostResult {
        if let Some(gsam) = &self.gsam {
            HostResult::ImsGsam(ImsGsamResult {
                result: self.result(),
                address: gsam.address.clone(),
            })
        } else {
            HostResult::Ims(self.result())
        }
    }
}
