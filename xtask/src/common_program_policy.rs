//! Bind existing catalog execution roles to the finite batch control declarations.

use crate::{TaskResult, text};
use serde_json::Value;
use std::path::Path;

#[derive(Debug)]
enum BuiltinRole {
    Idcams,
    Iebcompr,
    Iebcopy,
    Iebdg,
    Iebedit,
    Iebgener,
    Iebupdte,
    Iefbr14,
    Sort,
}

impl BuiltinRole {
    fn decode(value: &str) -> TaskResult<Self> {
        match value {
            "idcams" => Ok(Self::Idcams),
            "iebcompr" => Ok(Self::Iebcompr),
            "iebcopy" => Ok(Self::Iebcopy),
            "iebdg" => Ok(Self::Iebdg),
            "iebedit" => Ok(Self::Iebedit),
            "iebgener" => Ok(Self::Iebgener),
            "iebupdte" => Ok(Self::Iebupdte),
            "iefbr14" => Ok(Self::Iefbr14),
            "sort" => Ok(Self::Sort),
            _ => Err(format!("unknown common program builtin binding {value:?}")),
        }
    }

    fn policy(self, execution: ExecutionRole) -> TaskResult<ControlPolicy> {
        match (self, execution) {
            (Self::Idcams, ExecutionRole::Idcams) => Ok(ControlPolicy::Idcams),
            (Self::Iebcompr | Self::Iebgener, ExecutionRole::ProgramService) => {
                Ok(ControlPolicy::ReadsNone)
            }
            (Self::Iebcopy, ExecutionRole::ProgramService) => Ok(ControlPolicy::CopyCards),
            (Self::Iebdg, ExecutionRole::ProgramService) => Ok(ControlPolicy::GenerateCards),
            (Self::Iebedit, ExecutionRole::ProgramService) => Ok(ControlPolicy::EditCards),
            (Self::Iebupdte, ExecutionRole::ProgramService) => Ok(ControlPolicy::UpdateCards),
            (Self::Iefbr14, ExecutionRole::ProgramService) => Ok(ControlPolicy::IgnoreInput),
            (Self::Sort, ExecutionRole::ProgramService) => Ok(ControlPolicy::Sort),
            (builtin, execution) => Err(format!(
                "common program builtin/execution pairing is invalid: {builtin:?}/{execution:?}"
            )),
        }
    }
}

#[derive(Debug)]
enum ExecutionRole {
    ProgramService,
    Idcams,
    Sdsf,
    Db2Tso,
    ImsController,
    Unsupported,
}

impl ExecutionRole {
    fn decode(value: &str) -> TaskResult<Self> {
        match value {
            "program-service" => Ok(Self::ProgramService),
            "idcams" => Ok(Self::Idcams),
            "sdsf" => Ok(Self::Sdsf),
            "db2-tso" => Ok(Self::Db2Tso),
            "ims-controller" => Ok(Self::ImsController),
            "unsupported" => Ok(Self::Unsupported),
            _ => Err(format!(
                "unknown common program execution binding {value:?}"
            )),
        }
    }

    fn policy(self) -> TaskResult<ControlPolicy> {
        match self {
            Self::ProgramService | Self::Idcams => {
                Err("common program execution is missing its builtin binding".into())
            }
            Self::Sdsf => Ok(ControlPolicy::Sdsf),
            Self::Db2Tso => Ok(ControlPolicy::Tso),
            Self::ImsController => Ok(ControlPolicy::Ims),
            Self::Unsupported => Ok(ControlPolicy::Unavailable),
        }
    }
}

enum TsoRole {
    ExecuteScript,
    Extract,
}

impl TsoRole {
    fn decode(value: &str) -> TaskResult<Self> {
        match value {
            "execute-script" => Ok(Self::ExecuteScript),
            "extract" => Ok(Self::Extract),
            _ => Err(format!("unknown common program TSO binding {value:?}")),
        }
    }

    fn policy(self) -> ControlPolicy {
        match self {
            Self::ExecuteScript | Self::Extract => ControlPolicy::Tso,
        }
    }
}

pub(super) enum ControlPolicy {
    IgnoreInput,
    ReadsNone,
    CopyCards,
    GenerateCards,
    EditCards,
    UpdateCards,
    Sort,
    Idcams,
    Sdsf,
    Tso,
    Ims,
    Unavailable,
}

impl ControlPolicy {
    pub(super) fn utility(program: &Value, path: &Path) -> TaskResult<Self> {
        let execution = ExecutionRole::decode(text(program, "execution", path)?)?;
        if let Some(builtin) = program["builtin"].as_str() {
            BuiltinRole::decode(builtin)?.policy(execution)
        } else {
            execution.policy()
        }
    }

    pub(super) fn tso(action: &str) -> TaskResult<Self> {
        TsoRole::decode(action).map(TsoRole::policy)
    }

    pub(super) fn render(&self, source: &mut String, indent: &str) {
        let sources = match self {
            Self::IgnoreInput | Self::Unavailable => "&[]",
            Self::ReadsNone
            | Self::CopyCards
            | Self::GenerateCards
            | Self::EditCards
            | Self::UpdateCards => "&[\"SYSIN\"]",
            Self::Sort => "&[\"SYSIN\", \"SYMNAMES\"]",
            Self::Idcams => "&[\"SYSIN\", \"PARM\"]",
            Self::Sdsf => "&[\"ISFIN\"]",
            Self::Tso => "&[\"SYSTSIN\", \"SYSIN\"]",
            Self::Ims => "&[\"PARM\", \"SYSIN\"]",
        };
        source.push_str("super::ControlDeclaration {\n");
        source.push_str(&format!("{indent}    sources: {sources},\n"));
        source.push_str(&format!("{indent}    grammar: super::ControlGrammar::"));
        match self {
            Self::CopyCards => source.push_str("Cards(&[(\"COPY\", &[\"INDD\", \"OUTDD\"])]),\n"),
            Self::GenerateCards => {
                source.push_str("Cards(&[\n");
                for card in [
                    "(\"DSD\", &[\"OUTPUT\"])",
                    "(\"FD\", &[\"NAME\", \"LENGTH\", \"VALUE\"])",
                    "(\"CREATE\", &[\"QUANTITY\", \"RECORDS\", \"LENGTH\", \"VALUE\"])",
                    "(\"END\", &[])",
                ] {
                    source.push_str(&format!("{indent}        {card},\n"));
                }
                source.push_str(&format!("{indent}    ]),\n"));
            }
            Self::EditCards => {
                source.push_str("Cards(&[(\n");
                source.push_str(&format!("{indent}        \"EDIT\",\n"));
                source.push_str(&format!(
                    "{indent}        &[\"START\", \"STOP\", \"END\", \"STEPNAME\"],\n"
                ));
                source.push_str(&format!("{indent}    )]),\n"));
            }
            Self::IgnoreInput => source.push_str("IgnoreInput,\n"),
            Self::ReadsNone => source.push_str("ReadsNone,\n"),
            Self::UpdateCards => source.push_str("UpdateCards,\n"),
            Self::Sort => source.push_str("Sort,\n"),
            Self::Idcams => source.push_str("Idcams,\n"),
            Self::Sdsf => source.push_str("Sdsf,\n"),
            Self::Tso => source.push_str("Tso,\n"),
            Self::Ims => source.push_str("Ims,\n"),
            Self::Unavailable => source.push_str("Unavailable,\n"),
        }
        source.push_str(&format!("{indent}}}"));
    }
}
