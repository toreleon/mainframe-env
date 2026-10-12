//! Complete cataloged-procedure input for JCL submission.
use super::*;

impl ProductServer {
    pub(super) fn jcl_bundle(
        &self,
        principal: &str,
        jcl: Vec<u8>,
    ) -> Result<JclBundle, GatewayProblem> {
        let primary =
            String::from_utf8(jcl).map_err(|_| gateway_problem(HostProblem::Malformed))?;
        let mut cataloged_procedures = BTreeMap::new();
        for library in jcl_library_names(&primary).map_err(gateway_problem)? {
            self.authorize_resource(principal, "DATASET", &library, AccessIntent::Read)?;
            let dataset = DatasetName::new(library, 128)
                .map_err(|_| gateway_problem(HostProblem::Malformed))?;
            let (ccsid, members) = match (
                self.dataset
                    .invoke(DatasetRequest::Attributes {
                        dataset: dataset.clone(),
                    })
                    .map_err(gateway_problem)?,
                self.dataset
                    .invoke(DatasetRequest::ListMembers {
                        dataset: dataset.clone(),
                        start: None,
                        max_items: 4_096,
                    })
                    .map_err(gateway_problem)?,
            ) {
                (
                    DatasetResult::Attributes { attributes, .. },
                    DatasetResult::Members { names, more: false },
                ) => (attributes.ccsid, names),
                _ => return Err(gateway_problem(HostProblem::ProviderFailure)),
            };
            for member in members {
                let records = match self
                    .dataset
                    .invoke(DatasetRequest::Read {
                        dataset: dataset.clone(),
                        member: Some(member.clone()),
                        key: None,
                        max_records: 4_096,
                        control: Default::default(),
                    })
                    .and_then(DatasetResult::require_complete)
                    .map_err(gateway_problem)?
                {
                    DatasetResult::Records { records, .. } => records,
                    _ => return Err(gateway_problem(HostProblem::ProviderFailure)),
                };
                let mut source = String::new();
                for record in records {
                    let mut line = match ccsid {
                        None | Some(1208) => String::from_utf8(record)
                            .map_err(|_| gateway_problem(HostProblem::Malformed))?,
                        Some(37) => CodePage::Cp037
                            .decode(&record, record.len().saturating_mul(4).max(1))
                            .map_err(|_| gateway_problem(HostProblem::Malformed))?,
                        Some(_) => return Err(gateway_problem(HostProblem::Unsupported)),
                    };
                    while line.ends_with(' ') {
                        line.pop();
                    }
                    source.push_str(&line);
                    source.push('\n');
                }
                if cataloged_procedures
                    .insert(member.as_str().to_ascii_uppercase(), source)
                    .is_some()
                {
                    return Err(gateway_problem(HostProblem::IdempotencyConflict));
                }
            }
        }
        Ok(JclBundle {
            primary,
            cataloged_procedures,
            ..Default::default()
        })
    }
}
