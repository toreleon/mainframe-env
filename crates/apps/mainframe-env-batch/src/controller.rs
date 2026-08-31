use mainframe_env_host_api::HostProblem;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const BATCH_CONTROLLER_REGISTRY_CONTRACT: &str = "mainframe-env.batch-controller-registry@1";
pub(crate) const BATCH_CONTROLLER_STATE_CONTRACT: &str = "mainframe-env.batch-controller-state@1";
const MAX_APPLICATIONS: usize = 1_024;
const MAX_ACTIVE_CONTROLLERS: usize = 16_384;
const MAX_RETAINED_GENERATIONS: usize = 64;

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "launcher", rename_all = "kebab-case")]
pub enum BatchControllerSelector {
    TsoRun {
        program: String,
    },
    ImsController {
        mode: String,
        program: String,
        qualifier: Option<String>,
    },
}

impl BatchControllerSelector {
    pub(crate) fn tso(program: &str) -> Result<Self, HostProblem> {
        Ok(Self::TsoRun {
            program: normalized_name(program, 128)?,
        })
    }

    pub(crate) fn ims(
        mode: &str,
        program: &str,
        qualifier: Option<&str>,
    ) -> Result<Self, HostProblem> {
        Ok(Self::ImsController {
            mode: normalized_name(mode, 16)?,
            program: normalized_name(program, 128)?,
            qualifier: qualifier
                .map(|value| normalized_name(value, 128))
                .transpose()?,
        })
    }

    pub fn program(&self) -> &str {
        match self {
            Self::TsoRun { program } | Self::ImsController { program, .. } => program,
        }
    }

    pub(crate) fn mode(&self) -> Option<&str> {
        match self {
            Self::TsoRun { .. } => None,
            Self::ImsController { mode, .. } => Some(mode),
        }
    }

    fn normalized(&self) -> Result<Self, HostProblem> {
        match self {
            Self::TsoRun { program } => Self::tso(program),
            Self::ImsController {
                mode,
                program,
                qualifier,
            } => Self::ims(mode, program, qualifier.as_deref()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "behavior", rename_all = "kebab-case")]
pub enum BatchControllerPlan {
    ProgramCall,
    ImsLoad {
        database: String,
        root_dd: String,
        child_dd: String,
        root_record_bytes: usize,
        child_record_bytes: usize,
        parent_key_bytes: usize,
    },
    ImsUnload {
        database: String,
        root_segment: String,
        child_segment: String,
        root_output_dd: Option<String>,
        child_output_dd: Option<String>,
        combined_output_dd: Option<String>,
    },
    ImsPurge {
        psb: String,
        root_segment: String,
        child_segment: String,
        control_dd: String,
        required_expiry_days: String,
        checkpoint_prefix: String,
        summary_field: String,
    },
}

impl BatchControllerPlan {
    fn validate(&self) -> Result<(), HostProblem> {
        match self {
            Self::ProgramCall => Ok(()),
            Self::ImsLoad {
                database,
                root_dd,
                child_dd,
                root_record_bytes,
                child_record_bytes,
                parent_key_bytes,
            } => {
                validate_name(database, 128)?;
                validate_name(root_dd, 8)?;
                validate_name(child_dd, 8)?;
                if *root_record_bytes == 0
                    || *child_record_bytes == 0
                    || *root_record_bytes > 1_048_576
                    || *child_record_bytes > 1_048_576
                    || *parent_key_bytes == 0
                    || *parent_key_bytes >= *child_record_bytes
                {
                    return Err(HostProblem::Malformed);
                }
                Ok(())
            }
            Self::ImsUnload {
                database,
                root_segment,
                child_segment,
                root_output_dd,
                child_output_dd,
                combined_output_dd,
            } => {
                validate_name(database, 128)?;
                validate_name(root_segment, 128)?;
                validate_name(child_segment, 128)?;
                let split = root_output_dd.is_some() && child_output_dd.is_some();
                let combined = combined_output_dd.is_some();
                if split == combined || root_output_dd.is_some() != child_output_dd.is_some() {
                    return Err(HostProblem::Malformed);
                }
                for dd in [root_output_dd, child_output_dd, combined_output_dd]
                    .into_iter()
                    .flatten()
                {
                    validate_name(dd, 8)?;
                }
                Ok(())
            }
            Self::ImsPurge {
                psb,
                root_segment,
                child_segment,
                control_dd,
                required_expiry_days,
                checkpoint_prefix,
                summary_field,
            } => {
                validate_name(psb, 128)?;
                validate_name(root_segment, 128)?;
                validate_name(child_segment, 128)?;
                validate_name(control_dd, 8)?;
                validate_token(required_expiry_days, 16)?;
                validate_token(checkpoint_prefix, 16)?;
                validate_token(summary_field, 128)
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BatchControllerProgram {
    pub path: String,
    pub identity: String,
}

impl BatchControllerProgram {
    fn normalized(&self) -> Result<(Self, String), HostProblem> {
        validate_identity(&self.identity)?;
        if self.path.starts_with('/')
            || self
                .path
                .split('/')
                .any(|component| component.is_empty() || matches!(component, "." | ".."))
            || !self.path.starts_with("program/")
        {
            return Err(HostProblem::Malformed);
        }
        let name = self
            .path
            .rsplit('/')
            .next()
            .ok_or(HostProblem::Malformed)
            .and_then(|name| normalized_name(name, 128))?;
        Ok((
            Self {
                path: self.path.clone(),
                identity: self.identity.to_ascii_lowercase(),
            },
            name,
        ))
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BatchControllerDefinition {
    pub name: String,
    pub selector: BatchControllerSelector,
    pub program: BatchControllerProgram,
    pub plan: BatchControllerPlan,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BatchControllerGeneration {
    pub schema_version: String,
    pub application: String,
    pub generation: u64,
    pub identity: String,
    pub controllers: Vec<BatchControllerDefinition>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchControllerInstallReceipt {
    pub application: String,
    pub generation: u64,
    pub identity: String,
    pub controllers: usize,
    pub replayed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResolvedBatchController {
    pub name: String,
    pub selector: BatchControllerSelector,
    pub program: BatchControllerProgram,
    pub plan: BatchControllerPlan,
}

#[derive(Clone, Debug)]
struct InstalledGeneration {
    identity: String,
    controllers: BTreeMap<BatchControllerSelector, ResolvedBatchController>,
    generation: BatchControllerGeneration,
}

#[derive(Clone, Debug, Default)]
struct InstalledApplication {
    selected: Option<u64>,
    generations: BTreeMap<u64, InstalledGeneration>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct RetainedBatchControllerApplication {
    application: String,
    selected: u64,
    generations: Vec<BatchControllerGeneration>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct BatchControllerRegistryState {
    schema_version: String,
    applications: Vec<RetainedBatchControllerApplication>,
}

#[derive(Clone, Default)]
pub(crate) struct BatchControllerRegistry {
    applications: BTreeMap<String, InstalledApplication>,
    controllers: BTreeMap<BatchControllerSelector, (String, ResolvedBatchController)>,
}

impl BatchControllerRegistry {
    pub(crate) fn install(
        &mut self,
        generation: BatchControllerGeneration,
    ) -> Result<BatchControllerInstallReceipt, HostProblem> {
        let application = normalized_name(&generation.application, 128)?;
        validate_identity(&generation.identity)?;
        if generation.schema_version != BATCH_CONTROLLER_REGISTRY_CONTRACT
            || generation.generation == 0
            || generation.controllers.is_empty()
            || generation.controllers.len() > 4096
        {
            return Err(HostProblem::Malformed);
        }
        if !self.applications.contains_key(&application)
            && self.applications.len() >= MAX_APPLICATIONS
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let generation_number = generation.generation;
        let generation_identity = generation.identity.clone();
        let mut names = BTreeSet::new();
        let mut replacement = BTreeMap::new();
        let mut normalized_definitions = Vec::with_capacity(generation.controllers.len());
        for definition in generation.controllers {
            let name = normalized_name(&definition.name, 128)?;
            let selector = definition.selector.normalized()?;
            let (program, program_name) = definition.program.normalized()?;
            definition.plan.validate()?;
            if selector.program() != program_name {
                return Err(HostProblem::Malformed);
            }
            if !matches!(
                (&selector, &definition.plan),
                (
                    BatchControllerSelector::TsoRun { .. },
                    BatchControllerPlan::ProgramCall
                ) | (
                    BatchControllerSelector::ImsController { .. },
                    BatchControllerPlan::ImsLoad { .. }
                        | BatchControllerPlan::ImsUnload { .. }
                        | BatchControllerPlan::ImsPurge { .. }
                )
            ) {
                return Err(HostProblem::Malformed);
            }
            if !names.insert(name.clone())
                || replacement
                    .insert(
                        selector.clone(),
                        ResolvedBatchController {
                            name: name.clone(),
                            selector: selector.clone(),
                            program: program.clone(),
                            plan: definition.plan.clone(),
                        },
                    )
                    .is_some()
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            normalized_definitions.push(BatchControllerDefinition {
                name,
                selector,
                program,
                plan: definition.plan,
            });
        }
        let retained_generation = BatchControllerGeneration {
            schema_version: BATCH_CONTROLLER_REGISTRY_CONTRACT.into(),
            application: application.clone(),
            generation: generation_number,
            identity: generation_identity.clone(),
            controllers: normalized_definitions,
        };
        let selected = self
            .applications
            .get(&application)
            .and_then(|installed| installed.selected);
        if let Some(installed) = self
            .applications
            .get(&application)
            .and_then(|installed| installed.generations.get(&generation_number))
        {
            if installed.identity != generation_identity || installed.controllers != replacement {
                return Err(HostProblem::IdempotencyConflict);
            }
            if selected == Some(generation_number) {
                return Ok(BatchControllerInstallReceipt {
                    application,
                    generation: generation_number,
                    identity: generation_identity,
                    controllers: installed.controllers.len(),
                    replayed: true,
                });
            }
        } else if self
            .applications
            .get(&application)
            .and_then(|installed| installed.generations.keys().next_back().copied())
            .is_some_and(|latest| generation_number < latest)
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let retained = self
            .applications
            .get(&application)
            .map_or(0, |installed| installed.generations.len());
        if !self
            .applications
            .get(&application)
            .is_some_and(|installed| installed.generations.contains_key(&generation_number))
            && retained >= MAX_RETAINED_GENERATIONS
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let previous_count = selected
            .and_then(|selected| {
                self.applications
                    .get(&application)
                    .and_then(|installed| installed.generations.get(&selected))
            })
            .map_or(0, |installed| installed.controllers.len());
        if self
            .controllers
            .len()
            .saturating_sub(previous_count)
            .saturating_add(replacement.len())
            > MAX_ACTIVE_CONTROLLERS
        {
            return Err(HostProblem::ResourceExhausted);
        }
        for selector in replacement.keys() {
            if self
                .controllers
                .get(selector)
                .is_some_and(|(owner, _)| owner != &application)
            {
                return Err(HostProblem::IdempotencyConflict);
            }
        }

        // Publish only after the full generation validates. Readers therefore
        // see either the prior complete generation or the new complete one.
        if let Some(previous) = selected.and_then(|selected| {
            self.applications
                .get(&application)
                .and_then(|installed| installed.generations.get(&selected))
        }) {
            for selector in previous.controllers.keys() {
                self.controllers.remove(selector);
            }
        }
        for (selector, controller) in &replacement {
            self.controllers
                .insert(selector.clone(), (application.clone(), controller.clone()));
        }
        let installed = self.applications.entry(application.clone()).or_default();
        installed
            .generations
            .entry(generation_number)
            .or_insert_with(|| InstalledGeneration {
                identity: generation_identity.clone(),
                controllers: replacement,
                generation: retained_generation,
            });
        installed.selected = Some(generation_number);
        Ok(BatchControllerInstallReceipt {
            application,
            generation: generation_number,
            identity: generation_identity,
            controllers: names.len(),
            replayed: false,
        })
    }

    pub(crate) fn select(
        &mut self,
        application: &str,
        generation: u64,
    ) -> Result<BatchControllerInstallReceipt, HostProblem> {
        let application = normalized_name(application, 128)?;
        let retained = self
            .applications
            .get(&application)
            .and_then(|installed| installed.generations.get(&generation))
            .map(|installed| installed.generation.clone())
            .ok_or(HostProblem::NotFound)?;
        self.install(retained)
    }

    pub(crate) fn state(&self) -> BatchControllerRegistryState {
        BatchControllerRegistryState {
            schema_version: BATCH_CONTROLLER_STATE_CONTRACT.into(),
            applications: self
                .applications
                .iter()
                .filter_map(|(application, installed)| {
                    installed
                        .selected
                        .map(|selected| RetainedBatchControllerApplication {
                            application: application.clone(),
                            selected,
                            generations: installed
                                .generations
                                .values()
                                .map(|generation| generation.generation.clone())
                                .collect(),
                        })
                })
                .collect(),
        }
    }

    pub(crate) fn from_state(state: BatchControllerRegistryState) -> Result<Self, HostProblem> {
        if state.schema_version != BATCH_CONTROLLER_STATE_CONTRACT
            || state.applications.len() > MAX_APPLICATIONS
        {
            return Err(HostProblem::Malformed);
        }
        let mut registry = Self::default();
        let mut applications = BTreeSet::new();
        for application in state.applications {
            let normalized = normalized_name(&application.application, 128)?;
            if !applications.insert(normalized.clone())
                || application.generations.is_empty()
                || application.generations.len() > MAX_RETAINED_GENERATIONS
            {
                return Err(HostProblem::Malformed);
            }
            let mut generations = application.generations;
            generations.sort_by_key(|generation| generation.generation);
            let mut isolated = Self::default();
            for generation in generations {
                if normalized_name(&generation.application, 128)? != normalized {
                    return Err(HostProblem::Malformed);
                }
                isolated.install(generation)?;
            }
            isolated.select(&normalized, application.selected)?;
            let installed = isolated
                .applications
                .remove(&normalized)
                .ok_or(HostProblem::Malformed)?;
            registry.applications.insert(normalized, installed);
        }
        let selected = registry
            .applications
            .iter()
            .map(|(application, installed)| {
                let generation = installed
                    .selected
                    .and_then(|selected| installed.generations.get(&selected))
                    .ok_or(HostProblem::Malformed)?;
                Ok((application.clone(), generation.controllers.clone()))
            })
            .collect::<Result<Vec<_>, HostProblem>>()?;
        for (application, controllers) in selected {
            if registry.controllers.len().saturating_add(controllers.len()) > MAX_ACTIVE_CONTROLLERS
            {
                return Err(HostProblem::ResourceExhausted);
            }
            for (selector, controller) in controllers {
                if registry
                    .controllers
                    .insert(selector, (application.clone(), controller))
                    .is_some()
                {
                    return Err(HostProblem::IdempotencyConflict);
                }
            }
        }
        Ok(registry)
    }

    pub(crate) fn resolve(
        &self,
        selector: &BatchControllerSelector,
    ) -> Option<ResolvedBatchController> {
        self.controllers
            .get(selector)
            .map(|(_, controller)| controller.clone())
    }
}

fn normalized_name(value: &str, max_bytes: usize) -> Result<String, HostProblem> {
    let value = value.trim().to_ascii_uppercase();
    validate_name(&value, max_bytes)?;
    Ok(value)
}

fn validate_name(value: &str, max_bytes: usize) -> Result<(), HostProblem> {
    if value.is_empty()
        || value.len() > max_bytes
        || value != value.to_ascii_uppercase()
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'$' | b'@' | b'#')
        })
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn validate_token(value: &str, max_bytes: usize) -> Result<(), HostProblem> {
    if value.is_empty()
        || value.len() > max_bytes
        || value != value.to_ascii_uppercase()
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b'=')
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn validate_identity(value: &str) -> Result<(), HostProblem> {
    let digest = value
        .strip_prefix("sha256:")
        .ok_or(HostProblem::Malformed)?;
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generation(generation: u64, program: &str) -> BatchControllerGeneration {
        BatchControllerGeneration {
            schema_version: BATCH_CONTROLLER_REGISTRY_CONTRACT.into(),
            application: "example".into(),
            generation,
            identity: format!("sha256:{:064x}", generation),
            controllers: vec![BatchControllerDefinition {
                name: "maintenance".into(),
                selector: BatchControllerSelector::TsoRun {
                    program: program.into(),
                },
                program: BatchControllerProgram {
                    path: format!("program/{program}"),
                    identity: format!("sha256:{:064x}", generation + 100),
                },
                plan: BatchControllerPlan::ProgramCall,
            }],
        }
    }

    #[test]
    fn generation_is_validated_then_atomically_replaced() {
        let mut registry = BatchControllerRegistry::default();
        let first = generation(1, "FIRST");
        assert!(!registry.install(first.clone()).unwrap().replayed);
        assert!(registry.install(first).unwrap().replayed);

        let mut invalid = generation(2, "SECOND");
        invalid.controllers.push(invalid.controllers[0].clone());
        assert_eq!(
            registry.install(invalid),
            Err(HostProblem::IdempotencyConflict)
        );
        assert!(
            registry
                .resolve(&BatchControllerSelector::tso("FIRST").unwrap())
                .is_some()
        );
        assert!(
            registry
                .resolve(&BatchControllerSelector::tso("SECOND").unwrap())
                .is_none()
        );

        registry.install(generation(2, "SECOND")).unwrap();
        assert!(
            registry
                .resolve(&BatchControllerSelector::tso("FIRST").unwrap())
                .is_none()
        );
        assert!(
            registry
                .resolve(&BatchControllerSelector::tso("SECOND").unwrap())
                .is_some()
        );
        let rollback = registry.install(generation(1, "FIRST")).unwrap();
        assert!(!rollback.replayed);
        assert!(
            registry
                .resolve(&BatchControllerSelector::tso("FIRST").unwrap())
                .is_some()
        );
    }

    #[test]
    fn selector_conflicts_fail_closed_across_applications() {
        let mut registry = BatchControllerRegistry::default();
        registry.install(generation(1, "SHARED")).unwrap();
        let mut other = generation(1, "SHARED");
        other.application = "other".into();
        other.identity = format!("sha256:{:064x}", 99);
        assert_eq!(
            registry.install(other),
            Err(HostProblem::IdempotencyConflict)
        );
    }

    #[test]
    fn selector_program_must_be_the_validated_package_artifact() {
        let mut registry = BatchControllerRegistry::default();
        let mut bypass = generation(1, "EXPECTED");
        bypass.controllers[0].program.path = "program/DUMMY-MANIFEST".into();
        assert_eq!(registry.install(bypass), Err(HostProblem::Malformed));
        assert!(
            registry
                .resolve(&BatchControllerSelector::tso("EXPECTED").unwrap())
                .is_none()
        );
    }

    #[test]
    fn durable_state_rebuilds_selected_generation_and_retains_rollback() {
        let mut registry = BatchControllerRegistry::default();
        registry.install(generation(1, "FIRST")).unwrap();
        registry.install(generation(2, "SECOND")).unwrap();
        registry.select("EXAMPLE", 1).unwrap();

        let restored = BatchControllerRegistry::from_state(registry.state()).unwrap();
        assert!(
            restored
                .resolve(&BatchControllerSelector::tso("FIRST").unwrap())
                .is_some()
        );
        assert!(
            restored
                .resolve(&BatchControllerSelector::tso("SECOND").unwrap())
                .is_none()
        );
    }
}
