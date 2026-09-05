use crate::{DdPlan, Disposition};
use mainframe_env_host_api::HostProblem;

pub const JES_DD_ALLOCATION_CONTRACT: &str = "mainframe-env.jes-dd-allocation@1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DdStatusDisposition {
    Old,
    Shared,
    New,
    Modify,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DdTerminalDisposition {
    Pass,
    Keep,
    Catalog,
    Delete,
    Uncatalog,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DdDispositionPlan {
    pub status: DdStatusDisposition,
    pub normal: DdTerminalDisposition,
    pub abnormal: DdTerminalDisposition,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DdSourceKind {
    Dataset,
    Inline,
    Dummy,
    Sysout,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DdAllocationPlan {
    pub ordinal: usize,
    pub name: String,
    pub source: DdSourceKind,
    pub disposition: Option<DdDispositionPlan>,
    pub concatenation: bool,
    pub temporary: bool,
}

pub fn plan_dd_allocations(dds: &[DdPlan]) -> Result<Vec<DdAllocationPlan>, HostProblem> {
    let mut plans: Vec<DdAllocationPlan> = Vec::with_capacity(dds.len());
    for (ordinal, dd) in dds.iter().enumerate() {
        let dummy = dd
            .parameters
            .iter()
            .any(|parameter| parameter.identity().keyword() == "DUMMY");
        let source_count = usize::from(dd.dataset.is_some())
            + usize::from(!dd.inline_data.is_empty())
            + usize::from(dummy)
            + usize::from(dd.sysout.is_some());
        if source_count > 1 || (dd.concatenation && ordinal == 0) {
            return Err(HostProblem::Malformed);
        }
        let source = if dummy {
            DdSourceKind::Dummy
        } else if dd.sysout.is_some() {
            DdSourceKind::Sysout
        } else if dd.dataset.is_some() {
            DdSourceKind::Dataset
        } else {
            DdSourceKind::Inline
        };
        if source != DdSourceKind::Dataset && !dd.disposition.is_empty() {
            return Err(HostProblem::Malformed);
        }
        if dd.concatenation {
            let previous = plans.last().ok_or(HostProblem::Malformed)?;
            if previous.name != dd.name
                || matches!(previous.source, DdSourceKind::Sysout)
                || previous.disposition.is_some_and(|plan| {
                    matches!(
                        plan.status,
                        DdStatusDisposition::New | DdStatusDisposition::Modify
                    )
                })
                || matches!(source, DdSourceKind::Sysout)
                || matches!(
                    status_disposition(dd)?,
                    DdStatusDisposition::New | DdStatusDisposition::Modify
                )
            {
                return Err(HostProblem::Malformed);
            }
        }
        let disposition = (source == DdSourceKind::Dataset)
            .then(|| disposition_plan(dd))
            .transpose()?;
        if dd.member.is_some()
            && disposition.is_some_and(|plan| {
                matches!(
                    plan.normal,
                    DdTerminalDisposition::Catalog | DdTerminalDisposition::Uncatalog
                ) || matches!(
                    plan.abnormal,
                    DdTerminalDisposition::Catalog | DdTerminalDisposition::Uncatalog
                )
            })
        {
            return Err(HostProblem::Malformed);
        }
        if dd.temporary
            && disposition.is_some_and(|plan| {
                matches!(
                    plan.normal,
                    DdTerminalDisposition::Keep
                        | DdTerminalDisposition::Catalog
                        | DdTerminalDisposition::Uncatalog
                ) || matches!(
                    plan.abnormal,
                    DdTerminalDisposition::Keep
                        | DdTerminalDisposition::Catalog
                        | DdTerminalDisposition::Uncatalog
                )
            })
        {
            return Err(HostProblem::Malformed);
        }
        plans.push(DdAllocationPlan {
            ordinal,
            name: dd.name.to_ascii_uppercase(),
            source,
            disposition,
            concatenation: dd.concatenation,
            temporary: dd.temporary,
        });
    }
    Ok(plans)
}

fn disposition_plan(dd: &DdPlan) -> Result<DdDispositionPlan, HostProblem> {
    if dd.disposition.len() > 3 {
        return Err(HostProblem::Malformed);
    }
    let status = status_disposition(dd)?;
    let default_terminal = if status == DdStatusDisposition::New {
        DdTerminalDisposition::Delete
    } else {
        DdTerminalDisposition::Keep
    };
    let normal = dd
        .disposition
        .get(1)
        .map(terminal_disposition)
        .transpose()?
        .unwrap_or(default_terminal);
    let abnormal = dd
        .disposition
        .get(2)
        .map(terminal_disposition)
        .transpose()?
        .unwrap_or_else(|| {
            if normal == DdTerminalDisposition::Pass {
                default_terminal
            } else {
                normal
            }
        });
    if abnormal == DdTerminalDisposition::Pass {
        return Err(HostProblem::Malformed);
    }
    Ok(DdDispositionPlan {
        status,
        normal,
        abnormal,
    })
}

fn status_disposition(dd: &DdPlan) -> Result<DdStatusDisposition, HostProblem> {
    match dd.disposition.first() {
        Some(Disposition::Old) => Ok(DdStatusDisposition::Old),
        Some(Disposition::Shared) => Ok(DdStatusDisposition::Shared),
        Some(Disposition::New) => Ok(DdStatusDisposition::New),
        Some(Disposition::Modify) => Ok(DdStatusDisposition::Modify),
        Some(_) => Err(HostProblem::Malformed),
        None if dd.temporary => Ok(DdStatusDisposition::New),
        None => Ok(DdStatusDisposition::Old),
    }
}

fn terminal_disposition(value: &Disposition) -> Result<DdTerminalDisposition, HostProblem> {
    match value {
        Disposition::Pass => Ok(DdTerminalDisposition::Pass),
        Disposition::Keep => Ok(DdTerminalDisposition::Keep),
        Disposition::Catalog => Ok(DdTerminalDisposition::Catalog),
        Disposition::Delete => Ok(DdTerminalDisposition::Delete),
        Disposition::Uncatalog => Ok(DdTerminalDisposition::Uncatalog),
        Disposition::Old | Disposition::Shared | Disposition::New | Disposition::Modify => {
            Err(HostProblem::Malformed)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{JclBundle, parse_jcl};

    #[test]
    fn defaults_and_dummy_are_typed() {
        let plan = parse_jcl(
            &JclBundle {
                primary:
                    "//J JOB CLASS=A\n//S EXEC PGM=IEFBR14\n//D1 DD DSN=USER.DATA\n//D2 DD DUMMY\n"
                        .into(),
                ..Default::default()
            },
            Default::default(),
        )
        .unwrap();
        let allocations = plan_dd_allocations(&plan.steps[0].dds).unwrap();
        assert_eq!(allocations[0].source, DdSourceKind::Dataset);
        assert_eq!(
            allocations[0].disposition,
            Some(DdDispositionPlan {
                status: DdStatusDisposition::Old,
                normal: DdTerminalDisposition::Keep,
                abnormal: DdTerminalDisposition::Keep,
            })
        );
        assert_eq!(allocations[1].source, DdSourceKind::Dummy);
        assert_eq!(allocations[1].disposition, None);
    }

    #[test]
    fn malformed_disposition_positions_fail_closed() {
        let mut dd = parse_jcl(
            &JclBundle {
                primary: "//J JOB CLASS=A\n//S EXEC PGM=IEFBR14\n//D DD DSN=USER.DATA,DISP=OLD\n"
                    .into(),
                ..Default::default()
            },
            Default::default(),
        )
        .unwrap()
        .steps
        .remove(0)
        .dds
        .remove(0);
        dd.disposition = vec![Disposition::Keep];
        assert_eq!(plan_dd_allocations(&[dd]), Err(HostProblem::Malformed));
    }

    #[test]
    fn abnormal_disposition_defaults_to_normal_except_pass() {
        let plan = parse_jcl(
            &JclBundle {
                primary: "//J JOB CLASS=A\n//S EXEC PGM=IEFBR14\n//OLD DD DSN=USER.OLD,DISP=(OLD,DELETE)\n//NEW DD DSN=USER.NEW,DISP=(NEW,CATLG)\n//PASS DD DSN=USER.PASS,DISP=(NEW,PASS)\n"
                    .into(),
                ..Default::default()
            },
            Default::default(),
        )
        .unwrap();
        let allocations = plan_dd_allocations(&plan.steps[0].dds).unwrap();
        assert_eq!(
            allocations[0].disposition.unwrap().abnormal,
            DdTerminalDisposition::Delete
        );
        assert_eq!(
            allocations[1].disposition.unwrap().abnormal,
            DdTerminalDisposition::Catalog
        );
        assert_eq!(
            allocations[2].disposition.unwrap().abnormal,
            DdTerminalDisposition::Delete
        );
    }
}
