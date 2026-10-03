//! Existing batch terminal-problem projections, independent of state transitions.

use mainframe_env_host_api::HostProblem;

pub(super) fn ams_condition_code(problem: &HostProblem) -> u8 {
    match problem {
        HostProblem::NotFound => 8,
        HostProblem::Condition { response, .. } if *response <= 4 => 4,
        HostProblem::Condition { response, .. } if *response <= 8 => 8,
        HostProblem::Condition { response, .. } if *response <= 12 => 12,
        HostProblem::Unsupported
        | HostProblem::UnsupportedCapability { .. }
        | HostProblem::Malformed
        | HostProblem::Unauthorized
        | HostProblem::IdempotencyConflict => 12,
        _ => 16,
    }
}

pub(super) fn abend_code(problem: &HostProblem) -> Option<String> {
    match problem {
        HostProblem::Condition { name, .. } => name.strip_prefix("ABEND:").and_then(|code| {
            (!code.is_empty()
                && code.len() <= 16
                && code
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit()))
            .then(|| code.to_string())
        }),
        _ => None,
    }
}

pub(super) fn problem_category(problem: &HostProblem) -> &'static str {
    match problem {
        HostProblem::Malformed => "malformed",
        HostProblem::Unsupported | HostProblem::UnsupportedCapability { .. } => "unsupported",
        HostProblem::NotFound => "not-found",
        HostProblem::Condition { .. } => "condition",
        HostProblem::Unauthorized => "unauthorized",
        HostProblem::Cancelled => "cancelled",
        HostProblem::TimedOut => "timed-out",
        HostProblem::ResourceExhausted => "resource-exhausted",
        HostProblem::ProviderFailure => "provider-failure",
        HostProblem::InfrastructureFailure => "infrastructure-failure",
        HostProblem::MissingIdempotency => "missing-idempotency",
        HostProblem::IdempotencyConflict => "idempotency-conflict",
        HostProblem::UnknownOutcome => "unknown-outcome",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn condition(name: &str, response: i32) -> HostProblem {
        HostProblem::Condition {
            name: name.into(),
            response,
            response2: 0,
        }
    }

    #[test]
    fn existing_condition_code_thresholds_are_exact() {
        for (response, expected) in [(-1, 4), (4, 4), (5, 8), (8, 8), (9, 12), (12, 12), (13, 16)] {
            assert_eq!(ams_condition_code(&condition("TEST", response)), expected);
        }
        assert_eq!(ams_condition_code(&HostProblem::NotFound), 8);
        assert_eq!(ams_condition_code(&HostProblem::Unsupported), 12);
        assert_eq!(ams_condition_code(&HostProblem::UnknownOutcome), 16);
    }

    #[test]
    fn abend_projection_remains_bounded_and_case_sensitive() {
        for code in ["S0C7", "1234567890123456"] {
            assert_eq!(
                abend_code(&condition(&format!("ABEND:{code}"), -1)),
                Some(code.into())
            );
        }
        for name in [
            "ABEND:",
            "ABEND:s0c7",
            "ABEND:S0 C7",
            "ABEND:12345678901234567",
            "XABEND:S0C7",
        ] {
            assert_eq!(abend_code(&condition(name, -1)), None);
        }
        assert_eq!(abend_code(&HostProblem::UnknownOutcome), None);
    }

    #[test]
    fn uncertainty_and_cleanup_failure_keep_distinct_categories() {
        assert_eq!(
            problem_category(&HostProblem::UnknownOutcome),
            "unknown-outcome"
        );
        assert_eq!(
            problem_category(&HostProblem::ProviderFailure),
            "provider-failure"
        );
        assert_eq!(problem_category(&HostProblem::Cancelled), "cancelled");
        assert_eq!(problem_category(&condition("ABEND:S0C7", -1)), "condition");
    }
}
