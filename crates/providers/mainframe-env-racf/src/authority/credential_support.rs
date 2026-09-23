use super::CredentialPolicyProblem;
use mainframe_env_host_api::HostProblem;

pub(super) fn credential_host_problem(problem: CredentialPolicyProblem) -> HostProblem {
    match problem {
        CredentialPolicyProblem::Invalid => HostProblem::Malformed,
        CredentialPolicyProblem::Reused => HostProblem::IdempotencyConflict,
        CredentialPolicyProblem::Infrastructure => HostProblem::ProviderFailure,
    }
}
