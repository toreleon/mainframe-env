//! Deterministic JES lifecycle and scheduling contracts.

use mainframe_env_host_api::HostProblem;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const JES_RUNTIME_CONTRACT: &str = "mainframe-env.jes-runtime@1";
pub const JES_DURABLE_JOB_CONTRACT: &str = "mainframe-env.jes-durable-job@2";
pub const JES_CHECKPOINT_CONTRACT: &str = "mainframe-env.jes-checkpoint@1";
pub const JES_SPOOL_CONTRACT: &str = "mainframe-env.jes-spool@1";
pub const JES_OUTPUT_CONTRACT: &str = "mainframe-env.jes-output@1";
pub const JES_TOPOLOGY_CONTRACT: &str = "mainframe-env.jes-topology@1";
pub const JES_UTILITY_REGISTRY_CONTRACT: &str = "mainframe-env.jes-utility-registry@1";

const MAX_INITIATOR_NAME_BYTES: usize = 16;
const MAX_CLASSES: usize = 36;
const MAX_INITIATORS: usize = 4_096;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum JesJobKind {
    #[default]
    Batch,
    StartedTask,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum JesSubmissionOrigin {
    #[default]
    External,
    InternalReader {
        parent_job_id: String,
        step_name: String,
    },
    StartedTask {
        task_name: String,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JesJobRoute {
    pub origin_node: String,
    pub execution_node: String,
    pub output_node: String,
    pub owner_member: Option<String>,
}

impl Default for JesJobRoute {
    fn default() -> Self {
        Self {
            origin_node: "LOCAL".into(),
            execution_node: "LOCAL".into(),
            output_node: "LOCAL".into(),
            owner_member: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JesNodeDefinition {
    pub name: String,
    pub connected: bool,
    pub enabled: bool,
    pub max_inbound_jobs: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JesMasMemberDefinition {
    pub name: String,
    pub node: String,
    pub enabled: bool,
    pub max_active: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JesTopology {
    pub schema_version: String,
    pub local_node: String,
    pub nodes: BTreeMap<String, JesNodeDefinition>,
    pub members: BTreeMap<String, JesMasMemberDefinition>,
}

impl JesTopology {
    #[must_use]
    pub fn single_node(max_active: usize) -> Self {
        Self {
            schema_version: JES_TOPOLOGY_CONTRACT.into(),
            local_node: "LOCAL".into(),
            nodes: BTreeMap::from([(
                "LOCAL".into(),
                JesNodeDefinition {
                    name: "LOCAL".into(),
                    connected: true,
                    enabled: true,
                    max_inbound_jobs: 16_384,
                },
            )]),
            members: BTreeMap::from([(
                "MEMBER1".into(),
                JesMasMemberDefinition {
                    name: "MEMBER1".into(),
                    node: "LOCAL".into(),
                    enabled: true,
                    max_active,
                },
            )]),
        }
    }

    pub fn validate(&self, max_nodes: usize, max_members: usize) -> Result<(), HostProblem> {
        if self.schema_version != JES_TOPOLOGY_CONTRACT
            || self.nodes.is_empty()
            || self.nodes.len() > max_nodes
            || self.members.is_empty()
            || self.members.len() > max_members
            || !self.nodes.contains_key(&self.local_node)
        {
            return Err(HostProblem::Malformed);
        }
        for (name, node) in &self.nodes {
            if name != &node.name || !valid_topology_name(name) || node.max_inbound_jobs == 0 {
                return Err(HostProblem::Malformed);
            }
        }
        for (name, member) in &self.members {
            if name != &member.name
                || !valid_topology_name(name)
                || !self.nodes.contains_key(&member.node)
                || member.max_active == 0
            {
                return Err(HostProblem::Malformed);
            }
        }
        Ok(())
    }

    pub fn validate_route(&self, route: &JesJobRoute) -> Result<(), HostProblem> {
        if !self.nodes.contains_key(&route.origin_node)
            || !self.node_available(&route.execution_node)
            || !self.node_available(&route.output_node)
            || route.owner_member.as_ref().is_some_and(|member| {
                self.members.get(member).is_none_or(|definition| {
                    !definition.enabled || definition.node != route.execution_node
                })
            })
        {
            Err(HostProblem::UnsupportedCapability {
                capability: "jes.nje-mas.route".into(),
                detail: "node or MAS member is unavailable".into(),
            })
        } else {
            Ok(())
        }
    }

    #[must_use]
    pub fn node_available(&self, name: &str) -> bool {
        self.nodes
            .get(name)
            .is_some_and(|node| node.connected && node.enabled)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum JesQueue {
    Conversion,
    Execution,
    Output,
    Held,
    Terminal,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum JobState {
    #[serde(alias = "Submitted")]
    Submitted,
    #[serde(alias = "Held")]
    Held,
    #[serde(alias = "Queued")]
    Queued,
    #[serde(alias = "Selected")]
    Selected,
    #[serde(alias = "Running")]
    Running,
    #[serde(alias = "Output")]
    Output,
    #[serde(alias = "Completed")]
    Completed,
    #[serde(alias = "Failed")]
    Failed,
    #[serde(alias = "Cancelled")]
    Cancelled,
}

impl JobState {
    #[must_use]
    pub const fn queue(self) -> JesQueue {
        match self {
            Self::Submitted => JesQueue::Conversion,
            Self::Held => JesQueue::Held,
            Self::Queued | Self::Selected | Self::Running => JesQueue::Execution,
            Self::Output => JesQueue::Output,
            Self::Completed | Self::Failed | Self::Cancelled => JesQueue::Terminal,
        }
    }

    #[must_use]
    pub const fn terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    #[must_use]
    pub const fn can_transition_to(self, next: Self) -> bool {
        use JobState as S;
        matches!(
            (self, next),
            (S::Submitted, S::Held | S::Queued | S::Cancelled)
                | (S::Held, S::Queued | S::Cancelled)
                | (S::Queued, S::Held | S::Selected | S::Cancelled)
                | (S::Selected, S::Running | S::Queued | S::Cancelled)
                | (S::Running, S::Output | S::Failed | S::Cancelled)
                | (S::Output, S::Completed | S::Failed | S::Cancelled)
        )
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StepState {
    #[default]
    Pending,
    BypassedRestart,
    SkippedCondition,
    Allocating,
    Running,
    Disposing,
    Completed,
    Abended,
    Failed,
    Cancelled,
}

impl StepState {
    #[must_use]
    pub const fn terminal(self) -> bool {
        matches!(
            self,
            Self::BypassedRestart
                | Self::SkippedCondition
                | Self::Completed
                | Self::Abended
                | Self::Failed
                | Self::Cancelled
        )
    }

    #[must_use]
    pub const fn can_transition_to(self, next: Self) -> bool {
        use StepState as S;
        matches!(
            (self, next),
            (
                S::Pending,
                S::BypassedRestart | S::SkippedCondition | S::Allocating | S::Cancelled
            ) | (
                S::Allocating,
                S::Running | S::Failed | S::Abended | S::Cancelled
            ) | (
                S::Running,
                S::Disposing | S::Failed | S::Abended | S::Cancelled
            ) | (
                S::Disposing,
                S::Completed | S::Failed | S::Abended | S::Cancelled
            )
        )
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum StepTermination {
    ReturnCode { code: i32 },
    Abend { code: String, system: bool },
    Cancelled { reason: String },
    Failed { category: String },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum JesSpoolState {
    Open,
    Closed,
    Held,
    Released,
    Selected,
    Complete,
    Cancelled,
    Purged,
}

impl JesSpoolState {
    #[must_use]
    pub const fn can_transition_to(self, next: Self) -> bool {
        use JesSpoolState as S;
        matches!(
            (self, next),
            (S::Open, S::Closed | S::Held | S::Cancelled | S::Purged)
                | (
                    S::Closed,
                    S::Held | S::Selected | S::Complete | S::Cancelled | S::Purged
                )
                | (S::Held, S::Released | S::Cancelled | S::Purged)
                | (
                    S::Released,
                    S::Held | S::Selected | S::Complete | S::Cancelled | S::Purged
                )
                | (S::Selected, S::Complete | S::Cancelled | S::Purged)
                | (S::Complete | S::Cancelled, S::Purged)
        )
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JesSpoolDescriptor {
    pub schema_version: String,
    pub id: String,
    pub job_id: String,
    pub step_name: Option<String>,
    pub dd_name: String,
    pub class: char,
    pub destination: String,
    pub writer: Option<String>,
    pub forms: Option<String>,
    pub record_count: usize,
    pub byte_count: usize,
    pub created_tick: u64,
    pub retain_until_tick: u64,
    pub state: JesSpoolState,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum JesOutputState {
    AwaitingSelection,
    Held,
    Released,
    Selected,
    Printing,
    Complete,
    Cancelled,
    Purged,
}

impl JesOutputState {
    #[must_use]
    pub const fn can_transition_to(self, next: Self) -> bool {
        use JesOutputState as S;
        matches!(
            (self, next),
            (
                S::AwaitingSelection,
                S::Held | S::Selected | S::Cancelled | S::Purged
            ) | (S::Held, S::Released | S::Cancelled | S::Purged)
                | (
                    S::Released,
                    S::Held | S::Selected | S::Cancelled | S::Purged
                )
                | (
                    S::Selected,
                    S::Printing | S::Complete | S::Cancelled | S::Purged
                )
                | (S::Printing, S::Complete | S::Cancelled | S::Purged)
                | (S::Complete | S::Cancelled, S::Purged)
        )
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JesOutputGroup {
    pub schema_version: String,
    pub id: String,
    pub job_id: String,
    pub class: char,
    pub destination: String,
    pub writer: Option<String>,
    pub forms: Option<String>,
    pub copies: u16,
    pub retain_until_tick: u64,
    pub state: JesOutputState,
    pub spool_files: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UtilityFamily {
    Allocation,
    Catalog,
    Copy,
    Compare,
    Generate,
    Edit,
    Update,
    Sort,
    Diagnostic,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UtilityHandler {
    Iefbr14,
    Iebgener,
    Iebcopy,
    Iebcompr,
    Iebdg,
    Iebedit,
    Iebupdte,
    Idcams,
    Sort,
}

impl UtilityHandler {
    #[must_use]
    pub const fn family(self) -> UtilityFamily {
        match self {
            Self::Iefbr14 => UtilityFamily::Allocation,
            Self::Iebgener => UtilityFamily::Copy,
            Self::Iebcopy => UtilityFamily::Copy,
            Self::Iebcompr => UtilityFamily::Compare,
            Self::Iebdg => UtilityFamily::Generate,
            Self::Iebedit => UtilityFamily::Edit,
            Self::Iebupdte => UtilityFamily::Update,
            Self::Idcams => UtilityFamily::Catalog,
            Self::Sort => UtilityFamily::Sort,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CancellationState {
    Requested,
    Observed,
    Completed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JesCancellation {
    pub id: String,
    pub requested_by: String,
    pub reason: String,
    pub requested_tick: u64,
    pub state: CancellationState,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum JesControlOperation {
    HoldJob,
    ReleaseJob,
    CancelJob,
    PurgeJob,
    ChangeClass,
    ChangePriority,
    HoldOutput,
    ReleaseOutput,
    RouteOutput,
    SelectOutput,
    CompleteOutput,
    PurgeOutput,
    RouteJob,
    InstallTopology,
    StartInitiator,
    StopInitiator,
    StartTask,
    StopTask,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JesCheckpoint {
    pub schema_version: String,
    pub job_id: String,
    pub job_version: u64,
    pub attempt: u32,
    pub active_step: Option<String>,
    pub effect_sequence: u64,
    pub committed_steps: Vec<String>,
    pub dataset_resolutions: BTreeMap<String, String>,
    pub cancellation: Option<JesCancellation>,
    pub state_digest: String,
}

impl JesCheckpoint {
    pub fn validate(&self, max_steps: usize, max_resolutions: usize) -> Result<(), HostProblem> {
        if self.schema_version != JES_CHECKPOINT_CONTRACT
            || self.job_id.is_empty()
            || self.job_id.len() > 32
            || self.job_version == 0
            || self.attempt == 0
            || self.committed_steps.len() > max_steps
            || self.dataset_resolutions.len() > max_resolutions
            || !valid_sha256(&self.state_digest)
        {
            return Err(HostProblem::Malformed);
        }
        let unique = self.committed_steps.iter().collect::<BTreeSet<_>>();
        if unique.len() != self.committed_steps.len()
            || self
                .committed_steps
                .iter()
                .any(|name| name.is_empty() || name.len() > 128)
        {
            return Err(HostProblem::Malformed);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct StepExecution {
    pub name: String,
    pub state: StepState,
    pub attempt: u32,
    pub termination: Option<StepTermination>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JesClassDefinition {
    pub class: char,
    pub priority_floor: u8,
    pub priority_ceiling: u8,
    pub held_by_default: bool,
    pub max_active: usize,
    pub output_class: char,
}

impl JesClassDefinition {
    pub fn validate(&self) -> Result<(), HostProblem> {
        if !valid_class(self.class)
            || !valid_class(self.output_class)
            || self.priority_floor > self.priority_ceiling
            || self.max_active == 0
        {
            return Err(HostProblem::Malformed);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct InitiatorDefinition {
    pub name: String,
    pub classes: BTreeSet<char>,
    pub minimum_priority: u8,
    pub max_active: usize,
    pub enabled: bool,
}

impl InitiatorDefinition {
    pub fn validate(&self) -> Result<(), HostProblem> {
        if self.name.is_empty()
            || self.name.len() > MAX_INITIATOR_NAME_BYTES
            || !self
                .name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'@' | b'#' | b'$'))
            || self.classes.is_empty()
            || self.classes.len() > MAX_CLASSES
            || self.classes.iter().any(|class| !valid_class(*class))
            || self.max_active == 0
        {
            return Err(HostProblem::Malformed);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JesSchedulerConfiguration {
    pub schema_version: String,
    pub classes: BTreeMap<char, JesClassDefinition>,
    pub initiators: BTreeMap<String, InitiatorDefinition>,
}

impl JesSchedulerConfiguration {
    pub fn validate(&self) -> Result<(), HostProblem> {
        if self.schema_version != JES_RUNTIME_CONTRACT
            || self.classes.is_empty()
            || self.classes.len() > MAX_CLASSES
            || self.initiators.is_empty()
            || self.initiators.len() > MAX_INITIATORS
        {
            return Err(HostProblem::Malformed);
        }
        for (class, definition) in &self.classes {
            definition.validate()?;
            if class != &definition.class {
                return Err(HostProblem::Malformed);
            }
        }
        for (name, definition) in &self.initiators {
            definition.validate()?;
            if name != &definition.name || !definition.classes.is_subset(&self.class_names()) {
                return Err(HostProblem::Malformed);
            }
        }
        Ok(())
    }

    #[must_use]
    pub fn single_node(max_active: usize) -> Self {
        let classes = (b'A'..=b'Z')
            .chain(b'0'..=b'9')
            .map(|byte| {
                let class = char::from(byte);
                (
                    class,
                    JesClassDefinition {
                        class,
                        priority_floor: 0,
                        priority_ceiling: u8::MAX,
                        held_by_default: false,
                        max_active: max_active.max(1),
                        output_class: class,
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let initiator = InitiatorDefinition {
            name: "INIT0001".into(),
            classes: classes.keys().copied().collect(),
            minimum_priority: 0,
            max_active: max_active.max(1),
            enabled: true,
        };
        Self {
            schema_version: JES_RUNTIME_CONTRACT.into(),
            classes,
            initiators: BTreeMap::from([(initiator.name.clone(), initiator)]),
        }
    }

    fn class_names(&self) -> BTreeSet<char> {
        self.classes.keys().copied().collect()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JobSelectionCandidate<'a> {
    pub id: &'a str,
    pub class: char,
    pub priority: u8,
    pub state: JobState,
}

/// Selects highest priority, then the oldest JES numeric identity.
///
/// The caller supplies durable active counts so this pure kernel never becomes
/// a second authority for queue or initiator state.
pub fn select_job<'a>(
    configuration: &JesSchedulerConfiguration,
    initiator_name: &str,
    initiator_active: usize,
    class_active: &BTreeMap<char, usize>,
    candidates: impl IntoIterator<Item = JobSelectionCandidate<'a>>,
) -> Result<Option<&'a str>, HostProblem> {
    configuration.validate()?;
    let initiator = configuration
        .initiators
        .get(initiator_name)
        .ok_or(HostProblem::NotFound)?;
    if !initiator.enabled || initiator_active >= initiator.max_active {
        return Ok(None);
    }
    Ok(candidates
        .into_iter()
        .filter(|candidate| {
            candidate.state.queue() == JesQueue::Execution
                && candidate.state.can_transition_to(JobState::Selected)
                && candidate.priority >= initiator.minimum_priority
                && initiator.classes.contains(&candidate.class)
                && configuration
                    .classes
                    .get(&candidate.class)
                    .is_some_and(|class| {
                        candidate.priority >= class.priority_floor
                            && candidate.priority <= class.priority_ceiling
                            && class_active.get(&candidate.class).copied().unwrap_or(0)
                                < class.max_active
                    })
        })
        .max_by(|left, right| {
            left.priority
                .cmp(&right.priority)
                .then_with(|| right.id.cmp(left.id))
        })
        .map(|candidate| candidate.id))
}

fn valid_class(class: char) -> bool {
    class.is_ascii_uppercase() || class.is_ascii_digit()
}

fn valid_topology_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 16
        && value.bytes().all(|byte| {
            byte.is_ascii_uppercase() || byte.is_ascii_digit() || matches!(byte, b'@' | b'#' | b'$')
        })
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_rejects_shortcuts_and_terminal_transitions() {
        assert_eq!(JobState::Submitted.queue(), JesQueue::Conversion);
        assert_eq!(JobState::Held.queue(), JesQueue::Held);
        assert_eq!(JobState::Queued.queue(), JesQueue::Execution);
        assert_eq!(JobState::Selected.queue(), JesQueue::Execution);
        assert_eq!(JobState::Running.queue(), JesQueue::Execution);
        assert_eq!(JobState::Output.queue(), JesQueue::Output);
        assert_eq!(JobState::Completed.queue(), JesQueue::Terminal);
        assert_eq!(JobState::Failed.queue(), JesQueue::Terminal);
        assert_eq!(JobState::Cancelled.queue(), JesQueue::Terminal);
        assert!(JobState::Submitted.can_transition_to(JobState::Queued));
        assert!(JobState::Queued.can_transition_to(JobState::Selected));
        assert!(JobState::Selected.can_transition_to(JobState::Running));
        assert!(JobState::Running.can_transition_to(JobState::Output));
        assert!(JobState::Output.can_transition_to(JobState::Completed));
        assert!(!JobState::Queued.can_transition_to(JobState::Completed));
        assert!(!JobState::Completed.can_transition_to(JobState::Running));
        assert!(JobState::Cancelled.terminal());
        assert!(StepState::Abended.terminal());
        assert!(!StepState::Disposing.terminal());
        assert!(StepState::Pending.can_transition_to(StepState::Allocating));
        assert!(!StepState::Pending.can_transition_to(StepState::Completed));
    }

    #[test]
    fn initiator_selects_class_priority_then_fifo_and_honors_capacity() {
        let mut configuration = JesSchedulerConfiguration::single_node(2);
        let initiator = configuration.initiators.get_mut("INIT0001").unwrap();
        initiator.classes = BTreeSet::from(['A']);
        initiator.minimum_priority = 4;
        configuration.classes.get_mut(&'A').unwrap().max_active = 1;
        let candidates = [
            JobSelectionCandidate {
                id: "JOB00003",
                class: 'A',
                priority: 7,
                state: JobState::Queued,
            },
            JobSelectionCandidate {
                id: "JOB00001",
                class: 'A',
                priority: 7,
                state: JobState::Queued,
            },
            JobSelectionCandidate {
                id: "JOB00002",
                class: 'B',
                priority: 9,
                state: JobState::Queued,
            },
            JobSelectionCandidate {
                id: "JOB00000",
                class: 'A',
                priority: 3,
                state: JobState::Queued,
            },
        ];
        assert_eq!(
            select_job(&configuration, "INIT0001", 0, &BTreeMap::new(), candidates),
            Ok(Some("JOB00001"))
        );
        assert_eq!(
            select_job(
                &configuration,
                "INIT0001",
                0,
                &BTreeMap::from([('A', 1)]),
                candidates,
            ),
            Ok(None)
        );
        assert_eq!(
            select_job(&configuration, "INIT0001", 2, &BTreeMap::new(), candidates),
            Ok(None)
        );
    }

    #[test]
    fn configuration_fails_closed_on_unknown_classes_and_invalid_bounds() {
        let mut configuration = JesSchedulerConfiguration::single_node(1);
        configuration
            .initiators
            .get_mut("INIT0001")
            .unwrap()
            .classes
            .insert('!');
        assert_eq!(configuration.validate(), Err(HostProblem::Malformed));

        let mut configuration = JesSchedulerConfiguration::single_node(1);
        configuration.classes.get_mut(&'A').unwrap().priority_floor = 10;
        configuration
            .classes
            .get_mut(&'A')
            .unwrap()
            .priority_ceiling = 9;
        assert_eq!(configuration.validate(), Err(HostProblem::Malformed));

        let mut topology = JesTopology::single_node(1);
        topology.members.get_mut("MEMBER1").unwrap().node = "MISSING".into();
        assert_eq!(topology.validate(2, 2), Err(HostProblem::Malformed));

        let mut topology = JesTopology::single_node(1);
        topology.nodes.get_mut("LOCAL").unwrap().connected = false;
        assert!(matches!(
            topology.validate_route(&JesJobRoute::default()),
            Err(HostProblem::UnsupportedCapability { .. })
        ));
    }

    #[test]
    fn checkpoint_and_utility_contracts_are_closed_and_bounded() {
        assert_eq!(UtilityHandler::Iebcopy.family(), UtilityFamily::Copy);
        assert_eq!(UtilityHandler::Idcams.family(), UtilityFamily::Catalog);
        let mut checkpoint = JesCheckpoint {
            schema_version: JES_CHECKPOINT_CONTRACT.into(),
            job_id: "JOB00001".into(),
            job_version: 4,
            attempt: 1,
            active_step: Some("STEP1".into()),
            effect_sequence: 7,
            committed_steps: vec!["STEP0".into()],
            dataset_resolutions: BTreeMap::new(),
            cancellation: None,
            state_digest: format!("sha256:{}", "a".repeat(64)),
        };
        assert_eq!(checkpoint.validate(2, 2), Ok(()));
        checkpoint.committed_steps.push("STEP0".into());
        assert_eq!(checkpoint.validate(2, 2), Err(HostProblem::Malformed));
    }

    #[test]
    fn normative_schemas_compile_and_validate_runtime_and_migration_examples() {
        let schema: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../conformance/0.8/schemas/jes-runtime.schema.json"
        ))
        .unwrap();
        let validator = jsonschema::draft202012::options()
            .offline()
            .build(&schema)
            .unwrap();
        validator
            .validate(&serde_json::to_value(JesSchedulerConfiguration::single_node(1)).unwrap())
            .unwrap();
        validator
            .validate(&serde_json::to_value(JesTopology::single_node(1)).unwrap())
            .unwrap();
        validator
            .validate(
                &serde_json::to_value(JesCheckpoint {
                    schema_version: JES_CHECKPOINT_CONTRACT.into(),
                    job_id: "JOB00001".into(),
                    job_version: 4,
                    attempt: 1,
                    active_step: None,
                    effect_sequence: 0,
                    committed_steps: Vec::new(),
                    dataset_resolutions: BTreeMap::new(),
                    cancellation: None,
                    state_digest: format!("sha256:{}", "0".repeat(64)),
                })
                .unwrap(),
            )
            .unwrap();

        let migration_schema: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../conformance/0.8/schemas/jes-state-migration.schema.json"
        ))
        .unwrap();
        let migration: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../conformance/0.8/migrations/jes-durable-job-v1-to-v2.json"
        ))
        .unwrap();
        jsonschema::draft202012::options()
            .offline()
            .build(&migration_schema)
            .unwrap()
            .validate(&migration)
            .unwrap();

        let differential_schema: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../conformance/0.8/schemas/jes-licensed-differential-adapter.schema.json"
        ))
        .unwrap();
        let differential: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../conformance/0.8/oracles/jes-licensed-differential.json"
        ))
        .unwrap();
        jsonschema::draft202012::options()
            .offline()
            .build(&differential_schema)
            .unwrap()
            .validate(&differential)
            .unwrap();
        assert_eq!(differential["candidate_identity"]["source"], "git-index");
        assert_eq!(
            differential["candidate_identity"]["command"],
            "cargo xtask jes-oracle-candidate"
        );
        assert_eq!(
            differential["required_scenarios"].as_array().unwrap().len(),
            16
        );
        assert_eq!(
            differential["generated_historical_or_local_result_counts_as_pass"],
            false
        );
    }
}
