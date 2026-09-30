use super::*;

pub(super) struct SystemClockProvider {
    pub(super) descriptor: CapabilityDescriptor,
}

pub(super) struct BatchTerminalProvider {
    descriptor: CapabilityDescriptor,
}

impl BatchTerminalProvider {
    pub(super) fn new(limits: InvocationLimits) -> Self {
        Self {
            descriptor: CapabilityDescriptor {
                capability: CapabilityId::new("host.terminal", limits)
                    .expect("static terminal capability"),
                provider_id: "mainframe-env-batch-terminal".into(),
                generation: "1".into(),
                request_schema: "mainframe-env.terminal-request@1".into(),
                result_schema: "mainframe-env.terminal-result@1".into(),
                max_request_bytes: 64 * 1024,
                max_result_bytes: 4 * 1024 * 1024,
                ready: true,
            },
        }
    }
}

impl HostProvider for BatchTerminalProvider {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }

    fn invoke(&self, invocation: &Invocation, effect: EffectRequest) -> EffectResult {
        let outcome = match effect.request {
            HostRequest::Terminal(TerminalRequest::Read { .. }) => invocation
                .bindings
                .get("cobol.terminal.input")
                .cloned()
                .map(HostResult::Terminal)
                .ok_or(HostProblem::NotFound),
            _ => Err(HostProblem::Unsupported),
        };
        EffectResult {
            sequence: effect.sequence,
            outcome,
        }
    }
}

impl SystemClockProvider {
    pub(super) fn new(limits: InvocationLimits) -> Self {
        Self {
            descriptor: CapabilityDescriptor {
                capability: CapabilityId::new("host.clock", limits)
                    .expect("static clock capability"),
                provider_id: "mainframe-env-system-clock".into(),
                generation: "1".into(),
                request_schema: "mainframe-env.clock.request@1".into(),
                result_schema: "mainframe-env.clock.response@1".into(),
                max_request_bytes: 256,
                max_result_bytes: 256,
                ready: true,
            },
        }
    }
}

impl HostProvider for SystemClockProvider {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }

    fn invoke(&self, _: &Invocation, effect: EffectRequest) -> EffectResult {
        let outcome = match effect.request {
            HostRequest::Clock(request) => system_clock_value(request).map(HostResult::Clock),
            _ => Err(HostProblem::Malformed),
        };
        EffectResult {
            sequence: effect.sequence,
            outcome,
        }
    }
}

pub(super) fn system_clock_value(request: ClockRequest) -> Result<String, HostProblem> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let seconds = i64::try_from(duration.as_secs()).map_err(|_| HostProblem::ResourceExhausted)?;
    let days = seconds / 86_400;
    let rest = seconds % 86_400;
    let (year, month, day) = civil_from_unix_days(days);
    let hour = rest / 3_600;
    let minute = (rest / 60) % 60;
    let second = rest % 60;
    let milliseconds = duration.subsec_millis();
    Ok(match request {
        ClockRequest::UtcTimestamp => {
            format!("{year:04}{month:02}{day:02}{hour:02}{minute:02}{second:02}{milliseconds:03}")
        }
        ClockRequest::Date => format!("{year:04}{month:02}{day:02}"),
        ClockRequest::Time => format!("{hour:02}{minute:02}{second:02}{milliseconds:03}"),
    })
}

pub(super) fn civil_from_unix_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}
