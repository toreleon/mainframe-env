//! Additive replay output; historical receipts omit this field unchanged.
use super::*;
use mainframe_env_host_api::{ImsGsamAddress, ImsGsamRequest, ImsGsamResult};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReplayOutput {
    pub(crate) address: Option<ImsGsamAddress>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) undefined_length: Option<u32>,
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
            None,
        )
        .map(feedback::ExecutionOutput::gsam_result)
    }
}

impl RecordedResult {
    pub(crate) fn host_result(&self) -> HostResult {
        if let Some(feedback) = &self.pcb_feedback_v1 {
            HostResult::ImsPcbFeedbackV1(mainframe_env_host_api::ImsPcbFeedbackResultV1 {
                result: self.result(),
                feedback: feedback.clone(),
            })
        } else if let Some(gsam) = &self.gsam {
            HostResult::ImsGsam(ImsGsamResult {
                undefined_length: gsam.undefined_length,
                result: self.result(),
                address: gsam.address.clone(),
            })
        } else {
            HostResult::Ims(self.result())
        }
    }
}
