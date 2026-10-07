use super::{Result, required_text, validate_repository_path};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const DELIVERY: &str = "docs/delivery/subsystems/";
const PROMPTS: &str = "docs/prompts/subsystems/";

#[derive(Debug)]
pub(super) struct Subsystem {
    id: String,
    label: String,
    phases: Vec<Phase>,
}

#[derive(Debug)]
struct Phase {
    id: String,
    label: String,
    plan: String,
    status: Option<String>,
    prompt: Option<String>,
    dependencies: Vec<String>,
}

pub(super) fn parse(source: &Value) -> Result<Vec<Subsystem>> {
    let rows = source["subsystems"]
        .as_array()
        .filter(|rows| !rows.is_empty())
        .ok_or("documentation registry omits subsystems")?;
    let mut subsystems = Vec::new();
    let mut ids = BTreeSet::new();
    for row in rows {
        let id = token(required_text(row, "id")?)?;
        if !ids.insert(id.to_string()) {
            return Err(format!("documentation registry repeats subsystem {id}"));
        }
        let subsystem_label = label(required_text(row, "label")?)?;
        let values = row["phases"]
            .as_array()
            .filter(|values| !values.is_empty())
            .ok_or_else(|| format!("subsystem {id} omits phases"))?;
        let mut phases = Vec::new();
        let mut phase_ids = BTreeSet::new();
        for value in values {
            let phase_id = token(required_text(value, "id")?)?;
            if !phase_ids.insert(phase_id.to_string()) {
                return Err(format!("subsystem {id} repeats phase {phase_id}"));
            }
            let plan = required_text(value, "plan")?;
            owned_path(plan, DELIVERY, id)?;
            if plan != format!("{DELIVERY}{id}/{phase_id}-plan.md") {
                return Err(format!("{id}.{phase_id} plan must be named by phase"));
            }
            let status = optional_path(value, "status", DELIVERY, id)?;
            if status
                .as_ref()
                .is_some_and(|path| path != &format!("{DELIVERY}{id}/{phase_id}-status.md"))
            {
                return Err(format!("{id}.{phase_id} status must be named by phase"));
            }
            let prompt = optional_path(value, "prompt", PROMPTS, id)?;
            let dependencies = value["dependencies"]
                .as_array()
                .ok_or_else(|| format!("{id}.{phase_id} omits dependencies"))?
                .iter()
                .map(|dependency| {
                    dependency
                        .as_str()
                        .map(str::to_string)
                        .ok_or_else(|| format!("{id}.{phase_id} has a non-string dependency"))
                })
                .collect::<Result<Vec<_>>>()?;
            phases.push(Phase {
                id: phase_id.into(),
                label: label(required_text(value, "label")?)?.into(),
                plan: plan.into(),
                status,
                prompt,
                dependencies,
            });
        }
        subsystems.push(Subsystem {
            id: id.into(),
            label: subsystem_label.into(),
            phases,
        });
    }
    validate_dependencies(&subsystems)?;
    Ok(subsystems)
}

fn token(value: &str) -> Result<&str> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        || !value.as_bytes()[0].is_ascii_lowercase()
    {
        return Err(format!("invalid subsystem or phase id {value:?}"));
    }
    Ok(value)
}

fn label(value: &str) -> Result<&str> {
    if value.contains(['\n', '\r', '|', '[', ']']) {
        return Err(format!("invalid subsystem label {value:?}"));
    }
    Ok(value)
}

fn owned_path(path: &str, prefix: &str, id: &str) -> Result {
    validate_repository_path(path)?;
    let directory = format!("{prefix}{id}/");
    let name = path
        .strip_prefix(&directory)
        .filter(|name| name.ends_with(".md") && !name.contains('/'))
        .ok_or_else(|| format!("subsystem {id} does not own {path}"))?;
    if name.len() <= 3 {
        return Err(format!("subsystem {id} has an empty document name"));
    }
    Ok(())
}

fn optional_path(value: &Value, key: &str, prefix: &str, id: &str) -> Result<Option<String>> {
    if value.get(key).is_none() {
        return Ok(None);
    }
    let path = required_text(value, key)?;
    owned_path(path, prefix, id)?;
    Ok(Some(path.into()))
}

fn validate_dependencies(subsystems: &[Subsystem]) -> Result {
    let phases = subsystems
        .iter()
        .flat_map(|subsystem| {
            subsystem
                .phases
                .iter()
                .map(|phase| (format!("{}.{}", subsystem.id, phase.id), phase))
        })
        .collect::<BTreeMap<_, _>>();
    for (id, phase) in &phases {
        let mut seen = BTreeSet::new();
        for dependency in &phase.dependencies {
            if dependency == id || !phases.contains_key(dependency) || !seen.insert(dependency) {
                return Err(format!(
                    "{id} has invalid or repeated dependency {dependency}"
                ));
            }
        }
    }
    let mut remaining = phases.keys().cloned().collect::<BTreeSet<_>>();
    while !remaining.is_empty() {
        let ready = remaining
            .iter()
            .filter(|id| {
                phases[*id]
                    .dependencies
                    .iter()
                    .all(|dependency| !remaining.contains(dependency))
            })
            .cloned()
            .collect::<Vec<_>>();
        if ready.is_empty() {
            return Err(format!("subsystem dependency cycle: {remaining:?}"));
        }
        for id in ready {
            remaining.remove(&id);
        }
    }
    Ok(())
}

pub(super) fn navigation(subsystems: &[Subsystem]) -> Vec<(String, String)> {
    subsystems
        .iter()
        .flat_map(|subsystem| {
            subsystem.phases.iter().map(|phase| {
                (
                    format!("{} — {}", subsystem.label, phase.label),
                    phase.status.as_ref().unwrap_or(&phase.plan).clone(),
                )
            })
        })
        .collect()
}

pub(super) fn generate(
    subsystems: &[Subsystem],
    documents: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, (String, String)>> {
    let mut owned = BTreeSet::new();
    let mut plans = String::from("| Subsystem | Phase | Plan | Progress |\n|---|---|---|---|\n");
    let mut progress = String::from("| Subsystem | Phase | Recorded progress |\n|---|---|---|\n");
    let mut prompts = String::from("| Subsystem | Phase | Prompt |\n|---|---|---|\n");
    let mut dependencies =
        String::from("| Subsystem phase | Completion dependencies |\n|---|---|\n");
    for subsystem in subsystems {
        for phase in &subsystem.phases {
            for path in std::iter::once(&phase.plan)
                .chain(phase.status.iter())
                .chain(phase.prompt.iter())
            {
                if !owned.insert(path.clone()) {
                    return Err(format!("subsystem document has multiple owners: {path}"));
                }
                let text = documents
                    .get(path)
                    .ok_or_else(|| format!("subsystem document is missing: {path}"))?;
                let header = text.lines().take(16).collect::<Vec<_>>().join("\n");
                if !header.contains(&format!("Subsystem: **{}**", subsystem.id))
                    || !header.contains(&format!("Phase: **{}**", phase.id))
                {
                    return Err(format!(
                        "subsystem document metadata disagrees with registry: {path}"
                    ));
                }
                if path == &phase.plan || phase.prompt.as_ref() == Some(path) {
                    let expected = if phase.dependencies.is_empty() {
                        "none".to_string()
                    } else {
                        phase.dependencies.join(", ")
                    };
                    if !header.lines().any(|line| {
                        line.strip_prefix("Completion dependencies: ") == Some(expected.as_str())
                    }) {
                        return Err(format!(
                            "subsystem completion dependencies disagree with registry: {path}"
                        ));
                    }
                }
            }
            let plan = phase.plan.strip_prefix(DELIVERY).unwrap();
            let record = phase.status.as_ref().map_or_else(
                || "No progress record".to_string(),
                |path| format!("[Progress]({})", path.strip_prefix(DELIVERY).unwrap()),
            );
            plans.push_str(&format!(
                "| {} | {} | [Plan]({plan}) | {record} |\n",
                subsystem.label, phase.label,
            ));
            let state = if let Some(path) = &phase.status {
                let state = recorded_status(path, &documents[path])?;
                let relative = path.strip_prefix("docs/delivery/").unwrap();
                format!("[{state}]({relative})")
            } else {
                format!("[No progress record](subsystems/{plan})")
            };
            progress.push_str(&format!(
                "| {} | {} | {state} |\n",
                subsystem.label, phase.label,
            ));
            if let Some(path) = &phase.prompt {
                prompts.push_str(&format!(
                    "| {} | {} | [Implement]({}) |\n",
                    subsystem.label,
                    phase.label,
                    path.strip_prefix(PROMPTS).unwrap(),
                ));
            }
            let required = if phase.dependencies.is_empty() {
                "Accepted initial baseline".to_string()
            } else {
                phase.dependencies.join(", ")
            };
            dependencies.push_str(&format!(
                "| [{}.{}]({plan}) | {required} |\n",
                subsystem.id, phase.id,
            ));
        }
    }
    for path in documents.keys() {
        let managed = path.starts_with(DELIVERY)
            && (path.ends_with("-plan.md") || path.ends_with("-status.md"))
            || path.starts_with(PROMPTS)
                && path
                    .rsplit('/')
                    .next()
                    .is_some_and(|name| name.starts_with("IMPLEMENT_"));
        if managed && !owned.contains(path) {
            return Err(format!(
                "subsystem document is absent from registry: {path}"
            ));
        }
    }
    let mut output = BTreeMap::new();
    for (path, content) in [
        ("docs/delivery/subsystems/README.md", plans),
        ("docs/delivery/IMPLEMENTATION-STATUS.md", progress),
        ("docs/prompts/subsystems/README.md", prompts),
        ("docs/delivery/subsystems/DEPENDENCIES.md", dependencies),
    ] {
        let actual = documents
            .get(path)
            .ok_or_else(|| format!("subsystem index is missing: {path}"))?;
        let expected = replace_region(actual, content.trim_end())?;
        output.insert(path.to_string(), (actual.clone(), expected));
    }
    Ok(output)
}

fn recorded_status(path: &str, text: &str) -> Result<String> {
    let header = text.lines().take(16).collect::<Vec<_>>().join("\n");
    let tail = header
        .split_once("\nStatus: **")
        .map(|(_, tail)| tail)
        .ok_or_else(|| format!("{path} omits a bounded Status header"))?;
    let status = tail
        .split_once("**")
        .map(|(status, _)| status.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|status| !status.is_empty())
        .ok_or_else(|| format!("{path} has an unterminated Status header"))?;
    Ok(status
        .replace('|', "&#124;")
        .replace('[', "&#91;")
        .replace(']', "&#93;"))
}

fn replace_region(actual: &str, content: &str) -> Result<String> {
    let begin = "<!-- BEGIN GENERATED SUBSYSTEM INDEX -->";
    let end = "<!-- END GENERATED SUBSYSTEM INDEX -->";
    if actual.matches(begin).count() != 1 || actual.matches(end).count() != 1 {
        return Err("subsystem index must contain one pair of generation markers".into());
    }
    let (before, tail) = actual.split_once(begin).unwrap();
    let (_, after) = tail
        .split_once(end)
        .ok_or("subsystem index markers are out of order")?;
    Ok(format!("{before}{begin}\n{content}\n{end}{after}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn source() -> Value {
        json!({"subsystems":[{
            "id":"cics", "label":"CICS", "phases":[{
                "id":"application-api", "label":"Application API", "target_subsystem":"cics.application-api",
                "plan":"docs/delivery/subsystems/cics/application-api-plan.md",
                "status":"docs/delivery/subsystems/cics/application-api-status.md",
                "prompt":"docs/prompts/subsystems/cics/IMPLEMENT_APPLICATION_API.md",
                "dependencies":[]
            }]
        }]})
    }

    fn documents() -> BTreeMap<String, String> {
        let mut documents = BTreeMap::new();
        for path in [
            "docs/delivery/subsystems/README.md",
            "docs/delivery/IMPLEMENTATION-STATUS.md",
            "docs/prompts/subsystems/README.md",
            "docs/delivery/subsystems/DEPENDENCIES.md",
        ] {
            documents.insert(path.into(), "before\n<!-- BEGIN GENERATED SUBSYSTEM INDEX -->\nold\n<!-- END GENERATED SUBSYSTEM INDEX -->\nafter\n".into());
        }
        let text = "# CICS\n\nSubsystem: **cics**\nPhase: **application-api**\nTarget release: **0.9.0**\nCompletion dependencies: none\n\nStatus: **In progress;\nlicensed pending**\n";
        for path in [
            "docs/delivery/subsystems/cics/application-api-plan.md",
            "docs/delivery/subsystems/cics/application-api-status.md",
            "docs/prompts/subsystems/cics/IMPLEMENT_APPLICATION_API.md",
        ] {
            documents.insert(path.into(), text.into());
        }
        documents
    }

    #[test]
    fn progress_comes_from_the_record_instead_of_the_release_number() {
        let subsystems = parse(&source()).unwrap();
        let mut documents = documents();
        let generated = generate(&subsystems, &documents).unwrap();
        let status = generated["docs/delivery/IMPLEMENTATION-STATUS.md"]
            .1
            .clone();
        assert!(status.contains(
            "[In progress; licensed pending](subsystems/cics/application-api-status.md)"
        ));
        assert!(status.starts_with("before\n"));
        assert!(status.ends_with("\nafter\n"));
        for (path, (_, expected)) in generated {
            documents.insert(path, expected);
        }
        assert!(
            generate(&subsystems, &documents)
                .unwrap()
                .values()
                .all(|(actual, expected)| actual == expected)
        );
        documents
            .get_mut("docs/delivery/subsystems/cics/application-api-status.md")
            .unwrap()
            .push_str("\nHistorical release was published.\n");
        assert_eq!(
            generate(&subsystems, &documents).unwrap()["docs/delivery/IMPLEMENTATION-STATUS.md"].1,
            status
        );
    }

    #[test]
    fn phase_paths_cannot_escape_or_belong_to_another_owner() {
        for path in [
            "../outside.md",
            "docs/delivery/subsystems/db2/core-plan.md",
            "docs/delivery/subsystems/cics/0.9.0.md",
        ] {
            let mut value = source();
            value["subsystems"][0]["phases"][0]["plan"] = json!(path);
            assert!(parse(&value).is_err(), "accepted {path}");
        }
        let mut value = source();
        value["subsystems"][0]["phases"][0]["status"] = Value::Null;
        assert!(parse(&value).is_err());
    }

    #[test]
    fn missing_mismatched_and_unregistered_records_are_rejected() {
        let subsystems = parse(&source()).unwrap();
        let mut docs = documents();
        docs.remove("docs/delivery/subsystems/cics/application-api-status.md");
        assert!(
            generate(&subsystems, &docs)
                .unwrap_err()
                .contains("missing")
        );
        let mut docs = documents();
        docs.get_mut("docs/delivery/subsystems/cics/application-api-status.md")
            .unwrap()
            .replace_range(.., "# CICS\nStatus: **Complete**\n");
        assert!(
            generate(&subsystems, &docs)
                .unwrap_err()
                .contains("metadata")
        );
        let mut docs = documents();
        docs.insert(
            "docs/delivery/subsystems/cics/unregistered-status.md".into(),
            "# Extra\n".into(),
        );
        assert!(
            generate(&subsystems, &docs)
                .unwrap_err()
                .contains("absent from registry")
        );
    }

    #[test]
    fn duplicate_identities_and_invalid_dependencies_are_rejected() {
        let mut value = source();
        let duplicate = value["subsystems"][0].clone();
        value["subsystems"].as_array_mut().unwrap().push(duplicate);
        assert!(parse(&value).unwrap_err().contains("repeats subsystem"));
        for dependency in ["cics.application-api", "missing.phase"] {
            let mut value = source();
            value["subsystems"][0]["phases"][0]["dependencies"] = json!([dependency]);
            assert!(parse(&value).is_err());
        }
        let mut value = source();
        let mut second = value["subsystems"][0]["phases"][0].clone();
        second["id"] = json!("system-api");
        second["plan"] = json!("docs/delivery/subsystems/cics/system-api-plan.md");
        second["status"] = json!("docs/delivery/subsystems/cics/system-api-status.md");
        second["dependencies"] = json!(["cics.application-api"]);
        value["subsystems"][0]["phases"]
            .as_array_mut()
            .unwrap()
            .push(second);
        value["subsystems"][0]["phases"][0]["dependencies"] = json!(["cics.system-api"]);
        assert!(parse(&value).unwrap_err().contains("cycle"));
    }

    #[test]
    fn absent_progress_is_not_inferred_from_a_plan_or_a_release() {
        let mut value = source();
        value["subsystems"][0]["phases"][0]
            .as_object_mut()
            .unwrap()
            .remove("status");
        let mut docs = documents();
        docs.remove("docs/delivery/subsystems/cics/application-api-status.md");
        let generated = generate(&parse(&value).unwrap(), &docs).unwrap();
        assert!(
            generated["docs/delivery/IMPLEMENTATION-STATUS.md"]
                .1
                .contains("No progress record")
        );
    }

    #[test]
    fn plan_dependency_drift_and_broken_generation_markers_fail_closed() {
        let subsystems = parse(&source()).unwrap();
        let mut docs = documents();
        let path = "docs/delivery/subsystems/cics/application-api-plan.md";
        docs.insert(
            path.into(),
            docs[path].replace("dependencies: none", "dependencies: missing.phase"),
        );
        assert!(
            generate(&subsystems, &docs)
                .unwrap_err()
                .contains("dependencies disagree")
        );
        let mut docs = documents();
        docs.insert(
            "docs/delivery/subsystems/README.md".into(),
            "# Missing markers\n".into(),
        );
        assert!(
            generate(&subsystems, &docs)
                .unwrap_err()
                .contains("generation markers")
        );
        assert!(
            replace_region(
                "<!-- END GENERATED SUBSYSTEM INDEX -->\n<!-- BEGIN GENERATED SUBSYSTEM INDEX -->",
                "table"
            )
            .is_err()
        );
    }
}
