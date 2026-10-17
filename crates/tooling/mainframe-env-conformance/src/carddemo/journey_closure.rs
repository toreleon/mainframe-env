//! Private admission of the existing CardDemo closure authorities and observations.
use super::CorpusProblem;
use serde_json::Value;
use std::fs::File;
use std::io::Read;
use std::path::Path;

pub(super) const MAX_AUTHORITY_BYTES: usize = 64 * 1024;
pub(super) const MAX_ROWS: usize = 32;
pub(super) const MAX_REQUIREMENTS: usize = 16;
pub(super) const MAX_TEXT_BYTES: usize = 1024;
pub(super) const MAX_TOTAL_TEXT_BYTES: usize = 32 * 1024;

const JOURNEYS: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../conformance/profiles/carddemo/workloads/carddemo-journeys.json"
));
const ISSUES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../conformance/profiles/carddemo/inventory/carddemo-gap-matrix.json"
));

#[derive(Clone, Copy)]
pub(super) enum AuthorityKind {
    Journey,
    Issue,
}

impl AuthorityKind {
    fn rows(self) -> &'static str {
        match self {
            Self::Journey => "journeys",
            Self::Issue => "issues",
        }
    }

    fn requirements(self) -> &'static str {
        match self {
            Self::Journey => "observations",
            Self::Issue => "acceptance",
        }
    }

    fn selected(self, row: &Value) -> bool {
        match self {
            Self::Journey => true,
            Self::Issue => row["id"] != "CD-027",
        }
    }
}

fn invalid(detail: impl Into<String>) -> CorpusProblem {
    CorpusProblem::new("carddemo.full.closure_authority_invalid", detail)
}

fn text<'a>(value: &'a Value, total: &mut usize) -> Result<&'a str, CorpusProblem> {
    let text = value.as_str().ok_or_else(|| invalid("expected text"))?;
    admit_text(text, total)?;
    Ok(text)
}

pub(super) fn admit_text(text: &str, total: &mut usize) -> Result<(), CorpusProblem> {
    if text.trim().is_empty() || text.len() > MAX_TEXT_BYTES || text.chars().any(char::is_control) {
        return Err(invalid("empty, malformed or oversized closure text"));
    }
    *total = total
        .checked_add(text.len())
        .filter(|total| *total <= MAX_TOTAL_TEXT_BYTES)
        .ok_or_else(|| invalid("aggregate closure text bound exceeded"))?;
    Ok(())
}

fn object_fields(value: &Value, expected: &[&str]) -> Result<(), CorpusProblem> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid("expected object"))?;
    if object.len() != expected.len() || !object.keys().all(|key| expected.contains(&key.as_str()))
    {
        return Err(invalid("missing or unknown closure authority field"));
    }
    Ok(())
}

fn identity(kind: AuthorityKind, id: &str) -> Result<usize, CorpusProblem> {
    let (prefix, width, last) = match kind {
        AuthorityKind::Journey => ("CD.J", 2, 20),
        AuthorityKind::Issue => ("CD-", 3, 27),
    };
    let digits = id
        .strip_prefix(prefix)
        .ok_or_else(|| invalid("invalid closure identity"))?;
    if digits.len() != width || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid("invalid closure identity"));
    }
    let number = digits
        .bytes()
        .fold(0, |number, digit| number * 10 + usize::from(digit - b'0'));
    if number == 0 || number > last {
        return Err(invalid("unknown closure identity"));
    }
    Ok(number)
}

fn strings(value: &Value, allow_empty: bool, total: &mut usize) -> Result<(), CorpusProblem> {
    let strings = value
        .as_array()
        .ok_or_else(|| invalid("expected text array"))?;
    if (!allow_empty && strings.is_empty()) || strings.len() > MAX_REQUIREMENTS {
        return Err(invalid("closure requirement count bound exceeded"));
    }
    for (index, string) in strings.iter().enumerate() {
        text(string, total)?;
        if strings[..index].contains(string) {
            return Err(invalid("duplicate closure requirement or dependency"));
        }
    }
    Ok(())
}

fn preflight(kind: AuthorityKind, value: &Value) -> Result<&[Value], CorpusProblem> {
    let mut total = 0;
    match kind {
        AuthorityKind::Journey => {
            object_fields(value, &["schema_version", "status", "journeys"])?;
            if value["schema_version"] != "mainframe-env.carddemo-journeys@1"
                || value["status"] != "planned_not_executed"
            {
                return Err(invalid("journey authority version or status differs"));
            }
        }
        AuthorityKind::Issue => {
            object_fields(
                value,
                &["schema_version", "status", "commit_policy", "issues"],
            )?;
            if value["schema_version"] != "mainframe-env.carddemo-gap-matrix@1"
                || value["status"] != "open"
            {
                return Err(invalid("issue authority version or status differs"));
            }
            text(&value["commit_policy"], &mut total)?;
        }
    }
    let rows = value[kind.rows()]
        .as_array()
        .ok_or_else(|| invalid("expected closure rows"))?;
    if rows.is_empty() || rows.len() > MAX_ROWS {
        return Err(invalid("closure row count bound exceeded"));
    }
    for (index, row) in rows.iter().enumerate() {
        match kind {
            AuthorityKind::Journey => {
                object_fields(row, &["id", "profile", "name", "observations"])?;
                text(&row["profile"], &mut total)?;
                text(&row["name"], &mut total)?;
            }
            AuthorityKind::Issue => {
                object_fields(
                    row,
                    &[
                        "id",
                        "priority",
                        "domain",
                        "title",
                        "depends_on",
                        "commit_subject",
                        "acceptance",
                    ],
                )?;
                for field in ["priority", "domain", "title", "commit_subject"] {
                    text(&row[field], &mut total)?;
                }
                if row["priority"] != "P0" && row["priority"] != "P1" {
                    return Err(invalid("unknown issue priority"));
                }
                strings(&row["depends_on"], true, &mut total)?;
            }
        }
        identity(kind, text(&row["id"], &mut total)?)?;
        if rows[..index]
            .iter()
            .any(|previous| previous["id"] == row["id"])
        {
            return Err(invalid("duplicate expected closure identity"));
        }
        strings(&row[kind.requirements()], false, &mut total)?;
    }
    if let AuthorityKind::Issue = kind {
        // Select the declared surviving identities from the existing input. The
        // aggregate row cannot fill a missing issue or a selected dependency.
        for number in 1..27 {
            if !rows
                .iter()
                .any(|row| identity(kind, row["id"].as_str().unwrap()) == Ok(number))
            {
                return Err(invalid("missing surviving issue authority"));
            }
        }
        for row in rows {
            for dependency in row["depends_on"].as_array().unwrap() {
                if dependency == &row["id"]
                    || !rows.iter().any(|other| &other["id"] == dependency)
                    || (kind.selected(row) && dependency == "CD-027")
                {
                    return Err(invalid("unknown, self or aggregate issue dependency"));
                }
            }
        }
        let mut marks = [0; MAX_ROWS];
        for index in 0..rows.len() {
            visit_dependencies(rows, index, &mut marks)?;
        }
    }
    Ok(rows)
}

fn visit_dependencies(
    rows: &[Value],
    index: usize,
    marks: &mut [u8; MAX_ROWS],
) -> Result<(), CorpusProblem> {
    if marks[index] == 1 {
        return Err(invalid("cyclic issue dependency"));
    }
    if marks[index] == 2 {
        return Ok(());
    }
    marks[index] = 1;
    for dependency in rows[index]["depends_on"].as_array().unwrap() {
        let dependency_index = rows
            .iter()
            .position(|row| &row["id"] == dependency)
            .unwrap();
        visit_dependencies(rows, dependency_index, marks)?;
    }
    marks[index] = 2;
    Ok(())
}

pub(super) fn preflight_observed(observed: &[(String, Vec<String>)]) -> Result<(), CorpusProblem> {
    if observed.len() > MAX_ROWS {
        return Err(invalid("observed row count bound exceeded"));
    }
    let mut total = 0;
    for (index, (id, requirements)) in observed.iter().enumerate() {
        admit_text(id, &mut total)?;
        if observed[..index].iter().any(|(previous, _)| previous == id) {
            return Err(invalid("duplicate observed closure identity"));
        }
        if requirements.is_empty() || requirements.len() > MAX_REQUIREMENTS {
            return Err(invalid("empty or oversized observed requirements"));
        }
        for (requirement_index, requirement) in requirements.iter().enumerate() {
            admit_text(requirement, &mut total)?;
            if requirements[..requirement_index].contains(requirement) {
                return Err(invalid("duplicate observed requirement"));
            }
        }
    }
    Ok(())
}

fn close(
    kind: AuthorityKind,
    value: &Value,
    observed: &[(String, Vec<String>)],
) -> Result<usize, CorpusProblem> {
    let expected = preflight(kind, value)?;
    preflight_observed(observed)?;
    let total = expected.iter().filter(|row| kind.selected(row)).count();
    if observed.len() != total {
        return Err(CorpusProblem::new(
            "carddemo.full.closure_incomplete",
            "observed identities differ",
        ));
    }
    for row in expected.iter().filter(|row| kind.selected(row)) {
        let id = row["id"].as_str().unwrap();
        let actual = observed
            .iter()
            .find(|(observed_id, _)| observed_id == id)
            .ok_or_else(|| invalid(format!("missing observed {id}")))?;
        let required = row[kind.requirements()].as_array().unwrap();
        if required.len() != actual.1.len()
            || !required
                .iter()
                .all(|item| actual.1.iter().any(|observed| item == observed))
        {
            return Err(CorpusProblem::new(
                "carddemo.full.closure_incomplete",
                format!("mandatory observations differ for {id}"),
            ));
        }
    }
    Ok(total)
}

pub(super) fn close_carddemo_journeys(
    manifest: &Value,
    observed: &[(String, Vec<String>)],
) -> Result<usize, CorpusProblem> {
    close(AuthorityKind::Journey, manifest, observed)
}

pub(super) fn close_carddemo_issues(
    matrix: &Value,
    observed: &[(String, Vec<String>)],
) -> Result<usize, CorpusProblem> {
    close(AuthorityKind::Issue, matrix, observed)
}

pub(super) struct ClosureAuthority {
    journeys: Value,
    issues: Value,
}

impl ClosureAuthority {
    pub(super) fn load(inventory: &Path) -> Result<Self, CorpusProblem> {
        if !inventory.is_file()
            || inventory
                .file_name()
                .is_none_or(|name| name != "carddemo-corpus.json")
        {
            return Err(invalid("expected the real owning CardDemo inventory"));
        }
        let profile = inventory
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| invalid("inventory has no owning profile"))?;
        let journeys = read_authority(&profile.join("workloads/carddemo-journeys.json"), JOURNEYS)?;
        let issues = read_authority(&profile.join("inventory/carddemo-gap-matrix.json"), ISSUES)?;
        preflight(AuthorityKind::Journey, &journeys)?;
        preflight(AuthorityKind::Issue, &issues)?;
        Ok(Self { journeys, issues })
    }

    pub(super) fn finish(
        &self,
        observed: &super::journey_observations::RouteObservations,
    ) -> Result<(usize, usize), CorpusProblem> {
        let journeys = close_carddemo_journeys(&self.journeys, observed.journeys())?;
        let issues = close_carddemo_issues(&self.issues, observed.issues())?;
        Ok((journeys, issues))
    }
}

fn read_authority(path: &Path, expected: &[u8]) -> Result<Value, CorpusProblem> {
    let file = File::open(path)
        .map_err(|error| invalid(format!("cannot read closure authority: {error}")))?;
    if file
        .metadata()
        .map_err(|error| invalid(error.to_string()))?
        .len()
        > MAX_AUTHORITY_BYTES as u64
    {
        return Err(invalid("closure authority byte bound exceeded"));
    }
    let mut bytes = Vec::new();
    file.take((MAX_AUTHORITY_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| invalid(error.to_string()))?;
    if bytes.len() > MAX_AUTHORITY_BYTES {
        return Err(invalid("closure authority byte bound exceeded"));
    }
    if bytes != expected {
        return Err(invalid(
            "owning closure authority differs from committed input",
        ));
    }
    serde_json::from_slice(&bytes).map_err(|error| invalid(error.to_string()))
}
