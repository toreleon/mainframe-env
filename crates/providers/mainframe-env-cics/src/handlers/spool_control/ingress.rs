use crate::service::{CicsService, CicsSpoolReportSnapshot, handlers};
use mainframe_env_host_api::HostProblem;

impl CicsService {
    /// Stage one bounded local report for `SPOOLOPEN INPUT` selection.
    ///
    /// This is the protocol-neutral ingress boundary used by a JES adapter or
    /// deterministic test harness. CICS command mutations use the same durable
    /// state and compare-and-swap writer.
    pub fn stage_spool_input(
        &self,
        user_id: &str,
        class: u8,
        records: &[Vec<u8>],
    ) -> Result<String, HostProblem> {
        let user_id = user_id.trim().to_ascii_uppercase();
        if user_id.is_empty()
            || user_id.len() > 8
            || !user_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'*')
            || !class.is_ascii_alphanumeric()
            || records.iter().any(|record| record.len() > 32_760)
        {
            return Err(HostProblem::Malformed);
        }
        let mut state = self.lock()?;
        let current_version = state.spool.version;
        let mut next = state.spool.clone();
        if next.reports.len() >= self.limits.max_spool_reports
            || next
                .reports
                .values()
                .map(|report| report.records.len())
                .sum::<usize>()
                .checked_add(records.len())
                .is_none_or(|count| count > self.limits.max_spool_records)
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let token = next.allocate_token()?;
        let report = handlers::SpoolReport {
            token: token.clone(),
            state: handlers::SpoolReportState::AvailableInput,
            user_id,
            node: "LOCAL".into(),
            class: class.to_ascii_uppercase(),
            record_length: 32_760,
            owner_run_unit: None,
            owner_principal: None,
            records: records
                .iter()
                .cloned()
                .map(|bytes| handlers::SpoolRecord {
                    mode: handlers::SpoolRecordMode::Line,
                    bytes,
                })
                .collect(),
            next_record: 0,
            eof_seen: false,
            carriage_control: 0,
            punch: false,
            out_descriptor: Vec::new(),
        };
        next.reports.insert(token.clone(), report);
        handlers::persist_spool_state(self, current_version, &mut next)?;
        state.spool = next;
        Ok(token)
    }
}

impl CicsService {
    pub fn spool_report_snapshot(
        &self,
        token: &str,
    ) -> Result<CicsSpoolReportSnapshot, HostProblem> {
        let token = token.trim().to_ascii_uppercase();
        let state = self.lock()?;
        let report = state
            .spool
            .reports
            .get(&token)
            .ok_or(HostProblem::NotFound)?;
        Ok(CicsSpoolReportSnapshot {
            token: report.token.clone(),
            state: match report.state {
                handlers::SpoolReportState::AvailableInput => "available-input",
                handlers::SpoolReportState::OpenInput => "open-input",
                handlers::SpoolReportState::OpenOutput => "open-output",
            }
            .into(),
            user_id: report.user_id.clone(),
            node: report.node.clone(),
            class: report.class,
            record_length: report.record_length,
            owner_run_unit: report.owner_run_unit.clone(),
            records: report
                .records
                .iter()
                .map(|record| record.bytes.clone())
                .collect(),
            next_record: report.next_record,
        })
    }
}
