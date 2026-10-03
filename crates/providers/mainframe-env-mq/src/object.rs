//! Typed MQ object catalog, deterministic resolution, and model-queue lifecycle.

mod codec;
mod definition;
mod native_attributes;

pub use definition::*;
pub use native_attributes::*;

use crate::service::MqQueueDefinition;
use mainframe_env_host_api::HostProblem;
use std::collections::{BTreeMap, BTreeSet};

/// Service-facing MQ name rule (#342): significant bytes keep their case;
/// only trailing blanks, or a null ending significant data, are dropped.
pub(crate) fn canonical_name(value: &str) -> Result<String, HostProblem> {
    MqObjectName::new(value)
        .map(|name| name.as_str().to_owned())
        .map_err(|_| HostProblem::Malformed)
}

pub(crate) fn is_canonical_name(value: &str) -> bool {
    canonical_name(value).as_deref() == Ok(value)
}

pub(crate) fn canonical_definition(
    definition: MqQueueDefinition,
) -> Result<MqQueueDefinition, HostProblem> {
    Ok(MqQueueDefinition {
        name: canonical_name(&definition.name)?,
        trigger_program: (definition.trigger_program.as_deref())
            .map(canonical_name)
            .transpose()?,
    })
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum ObjectNamespace {
    Queue,
    Topic,
    Subscription,
    Process,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ObjectKey {
    namespace: ObjectNamespace,
    name: MqObjectName,
}

fn definition_key(definition: &MqObjectDefinition) -> ObjectKey {
    let namespace = match definition.kind() {
        MqObjectKind::LocalQueue
        | MqObjectKind::AliasQueue
        | MqObjectKind::RemoteQueue
        | MqObjectKind::ModelQueue => ObjectNamespace::Queue,
        MqObjectKind::Topic => ObjectNamespace::Topic,
        MqObjectKind::Subscription => ObjectNamespace::Subscription,
        MqObjectKind::Process => ObjectNamespace::Process,
        MqObjectKind::QueueManager => unreachable!("queue manager is catalog metadata"),
    };
    ObjectKey {
        namespace,
        name: definition.name().clone(),
    }
}

fn lookup_key(lookup: &MqObjectLookup) -> Option<ObjectKey> {
    let (namespace, name) = match lookup {
        MqObjectLookup::QueueManager => return None,
        MqObjectLookup::Queue(name) => (ObjectNamespace::Queue, name),
        MqObjectLookup::Topic(name) => (ObjectNamespace::Topic, name),
        MqObjectLookup::Subscription(name) => (ObjectNamespace::Subscription, name),
        MqObjectLookup::Process(name) => (ObjectNamespace::Process, name),
    };
    Some(ObjectKey {
        namespace,
        name: name.clone(),
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqObjectCatalog {
    queue_manager: MqQueueManagerDefinition,
    objects: BTreeMap<ObjectKey, MqObjectDefinition>,
    instances: BTreeMap<MqObjectName, MqModelInstance>,
    next_dynamic_id: u64,
    limits: MqObjectLimits,
    native_attributes: Option<MqNativeAttributes>,
}

impl MqObjectCatalog {
    pub fn new(
        queue_manager: MqQueueManagerDefinition,
        definitions: Vec<MqObjectDefinition>,
        limits: MqObjectLimits,
    ) -> Result<Self, MqObjectError> {
        validate_limits(limits)?;
        if definitions.len() > limits.max_objects {
            return Err(MqObjectError::ResourceExhausted);
        }
        let mut objects = BTreeMap::new();
        for definition in definitions {
            if objects
                .insert(definition_key(&definition), definition)
                .is_some()
            {
                return Err(MqObjectError::DuplicateObject);
            }
        }
        let catalog = Self {
            queue_manager,
            objects,
            instances: BTreeMap::new(),
            next_dynamic_id: 1,
            limits,
            native_attributes: None,
        };
        catalog.validate_references()?;
        Ok(catalog)
    }

    /// Deterministically migrate the provider's historical queue-only install shape.
    pub fn from_queue_definitions(
        queue_manager: MqQueueManagerDefinition,
        definitions: &[MqQueueDefinition],
        limits: MqObjectLimits,
    ) -> Result<Self, MqObjectError> {
        let mut processes = BTreeSet::new();
        let mut migrated = Vec::new();
        for definition in definitions {
            let trigger_process = definition
                .trigger_program
                .as_deref()
                .map(MqObjectName::new)
                .transpose()?;
            if let Some(process) = &trigger_process {
                processes.insert(process.clone());
            }
            migrated.push(MqObjectDefinition::LocalQueue {
                name: MqObjectName::new(&definition.name)?,
                usage: MqLocalQueueUsage::Normal,
                trigger_process,
            });
        }
        migrated.extend(
            processes
                .into_iter()
                .map(|name| MqObjectDefinition::Process { name }),
        );
        Self::new(queue_manager, migrated, limits)
    }

    #[must_use]
    pub fn queue_manager(&self) -> &MqQueueManagerDefinition {
        &self.queue_manager
    }

    /// Definitions supplied by package topology, in stable namespace/name order.
    pub fn definitions(&self) -> impl Iterator<Item = &MqObjectDefinition> {
        self.objects.values()
    }

    /// Dynamic queues are part of the same authority as the static definitions.
    pub fn model_instances(&self) -> impl Iterator<Item = &MqModelInstance> {
        self.instances.values()
    }

    pub fn kind(&self, lookup: &MqObjectLookup) -> Result<MqObjectKind, MqObjectError> {
        if matches!(lookup, MqObjectLookup::QueueManager) {
            return Ok(MqObjectKind::QueueManager);
        }
        if let MqObjectLookup::Queue(name) = lookup
            && self.instances.contains_key(name)
        {
            return Ok(MqObjectKind::LocalQueue);
        }
        self.objects
            .get(&lookup_key(lookup).ok_or(MqObjectError::UnknownObject)?)
            .map(MqObjectDefinition::kind)
            .ok_or(MqObjectError::UnknownObject)
    }

    pub fn capabilities(
        &self,
        lookup: &MqObjectLookup,
    ) -> Result<BTreeSet<MqObjectCapability>, MqObjectError> {
        if matches!(lookup, MqObjectLookup::QueueManager) {
            return Ok(capability_set(&[MqObjectCapability::Inquire]));
        }
        if let MqObjectLookup::Queue(name) = lookup
            && self.instances.contains_key(name)
        {
            return Ok(local_queue_capabilities());
        }
        let definition = self
            .objects
            .get(&lookup_key(lookup).ok_or(MqObjectError::UnknownObject)?)
            .ok_or(MqObjectError::UnknownObject)?;
        match definition {
            MqObjectDefinition::LocalQueue { .. } => Ok(local_queue_capabilities()),
            MqObjectDefinition::AliasQueue { target, .. } => match target {
                MqAliasTarget::Queue(name) => {
                    self.capabilities(&MqObjectLookup::Queue(name.clone()))
                }
                MqAliasTarget::Topic(name) => {
                    self.capabilities(&MqObjectLookup::Topic(name.clone()))
                }
            },
            MqObjectDefinition::RemoteQueue { remote_queue, .. } => {
                let mut values = vec![MqObjectCapability::Inquire, MqObjectCapability::Set];
                if remote_queue.is_some() {
                    values.push(MqObjectCapability::Output);
                }
                Ok(capability_set(&values))
            }
            MqObjectDefinition::ModelQueue { .. } => Ok(local_queue_capabilities()),
            MqObjectDefinition::Topic { .. } => Ok(capability_set(&[
                MqObjectCapability::Publish,
                MqObjectCapability::Subscribe,
            ])),
            MqObjectDefinition::Subscription { .. } => {
                Ok(capability_set(&[MqObjectCapability::RequestPublications]))
            }
            MqObjectDefinition::Process { .. } => {
                Ok(capability_set(&[MqObjectCapability::Inquire]))
            }
        }
    }

    pub fn resolve(
        &self,
        lookup: &MqObjectLookup,
        capability: MqObjectCapability,
    ) -> Result<MqResolution, MqObjectError> {
        let available = self.capabilities(lookup)?;
        if !available.contains(&capability) {
            return Err(MqObjectError::UnsupportedCapability);
        }
        let mut context = ResolutionContext::new(self.limits.max_resolution_depth);
        if matches!(
            capability,
            MqObjectCapability::Inquire | MqObjectCapability::Set
        ) || matches!(
            (lookup, capability),
            (
                MqObjectLookup::Subscription(_),
                MqObjectCapability::RequestPublications
            ) | (MqObjectLookup::Process(_), MqObjectCapability::Inquire)
                | (MqObjectLookup::QueueManager, MqObjectCapability::Inquire)
        ) {
            let identity = self.identity(lookup)?;
            context.enter(lookup_key(lookup), identity.clone())?;
            return Ok(MqResolution {
                target: MqResolvedTarget::Definition { identity },
                path: context.path,
            });
        }
        let target = match lookup {
            MqObjectLookup::Queue(name) => self.resolve_queue(name, capability, &mut context)?,
            MqObjectLookup::Topic(name) => self.resolve_topic(name, capability, &mut context)?,
            _ => return Err(MqObjectError::UnsupportedCapability),
        };
        Ok(MqResolution {
            target,
            path: context.path,
        })
    }

    pub fn create_model_instance(
        &mut self,
        model: &MqObjectName,
        pattern: &MqDynamicQueuePattern,
        creator: MqLifecycleOwner,
    ) -> Result<MqModelInstance, MqObjectError> {
        if self.instances.len() >= self.limits.max_dynamic_instances {
            return Err(MqObjectError::ResourceExhausted);
        }
        let (definition_type, trigger_process) = match self.queue_definition(model) {
            Some(MqObjectDefinition::ModelQueue {
                definition_type,
                trigger_process,
                ..
            }) => (*definition_type, trigger_process.clone()),
            Some(_) => return Err(MqObjectError::InvalidReferenceKind),
            None => return Err(MqObjectError::UnknownObject),
        };
        let (instance_id, instance_name, next_dynamic_id) = self.allocate_dynamic_name(pattern)?;
        let instance = MqModelInstance {
            name: instance_name.clone(),
            model: model.clone(),
            definition_type,
            trigger_process,
            creator,
            instance_id,
        };
        self.instances.insert(instance_name, instance.clone());
        self.next_dynamic_id = next_dynamic_id;
        Ok(instance)
    }

    pub fn close_model_instance(
        &mut self,
        name: &MqObjectName,
        requester: &MqLifecycleOwner,
        mode: MqCloseMode,
        state: MqDynamicQueueState,
        delete_authorized: bool,
    ) -> Result<MqCloseOutcome, MqObjectError> {
        let instance = self
            .instances
            .get(name)
            .cloned()
            .ok_or(MqObjectError::UnknownObject)?;
        match instance.definition_type {
            MqDynamicQueueKind::Temporary if &instance.creator != requester => {
                if mode == MqCloseMode::Retain {
                    return Ok(MqCloseOutcome::Retained);
                }
                return Err(MqObjectError::InvalidCloseMode);
            }
            MqDynamicQueueKind::Temporary => {
                self.instances.remove(name);
                return Ok(MqCloseOutcome::Deleted {
                    purged_messages: state.messages,
                });
            }
            MqDynamicQueueKind::Permanent if mode == MqCloseMode::Retain => {
                return Ok(MqCloseOutcome::Retained);
            }
            MqDynamicQueueKind::Permanent => {}
        }
        if &instance.creator != requester && !delete_authorized {
            return Err(MqObjectError::NotAuthorized);
        }
        if state.pending_updates != 0 || (mode == MqCloseMode::Delete && state.messages != 0) {
            return Err(MqObjectError::ObjectInUse);
        }
        self.instances.remove(name);
        Ok(MqCloseOutcome::Deleted {
            purged_messages: if mode == MqCloseMode::DeletePurge {
                state.messages
            } else {
                0
            },
        })
    }

    fn identity(&self, lookup: &MqObjectLookup) -> Result<MqObjectIdentity, MqObjectError> {
        let name = match lookup {
            MqObjectLookup::QueueManager => self.queue_manager.name.clone(),
            MqObjectLookup::Queue(name)
            | MqObjectLookup::Topic(name)
            | MqObjectLookup::Subscription(name)
            | MqObjectLookup::Process(name) => name.clone(),
        };
        Ok(MqObjectIdentity {
            kind: self.kind(lookup)?,
            name,
        })
    }

    fn resolve_queue(
        &self,
        name: &MqObjectName,
        capability: MqObjectCapability,
        context: &mut ResolutionContext,
    ) -> Result<MqResolvedTarget, MqObjectError> {
        if let Some(instance) = self.instances.get(name) {
            if !local_queue_capabilities().contains(&capability) {
                return Err(MqObjectError::UnsupportedCapability);
            }
            context.enter(
                Some(ObjectKey {
                    namespace: ObjectNamespace::Queue,
                    name: name.clone(),
                }),
                MqObjectIdentity {
                    kind: MqObjectKind::LocalQueue,
                    name: name.clone(),
                },
            )?;
            return Ok(MqResolvedTarget::Queue {
                name: name.clone(),
                dynamic: true,
                model: Some(instance.model.clone()),
            });
        }
        let definition = self
            .queue_definition(name)
            .ok_or(MqObjectError::UnknownObject)?;
        context.enter(
            Some(definition_key(definition)),
            MqObjectIdentity {
                kind: definition.kind(),
                name: name.clone(),
            },
        )?;
        match definition {
            MqObjectDefinition::LocalQueue { .. } => {
                if !local_queue_capabilities().contains(&capability) {
                    return Err(MqObjectError::UnsupportedCapability);
                }
                Ok(MqResolvedTarget::Queue {
                    name: name.clone(),
                    dynamic: false,
                    model: None,
                })
            }
            MqObjectDefinition::AliasQueue { target, .. } => match target {
                MqAliasTarget::Queue(target) => self.resolve_queue(target, capability, context),
                MqAliasTarget::Topic(target) => self.resolve_topic(target, capability, context),
            },
            MqObjectDefinition::RemoteQueue {
                remote_queue: Some(remote_queue),
                remote_queue_manager,
                transmission_queue,
                ..
            } if capability == MqObjectCapability::Output => self.resolve_remote(
                name,
                remote_queue,
                remote_queue_manager,
                transmission_queue,
                context,
            ),
            MqObjectDefinition::RemoteQueue { .. } => Err(MqObjectError::UnsupportedCapability),
            MqObjectDefinition::ModelQueue {
                definition_type, ..
            } if matches!(
                capability,
                MqObjectCapability::Input | MqObjectCapability::Browse | MqObjectCapability::Output
            ) =>
            {
                Ok(MqResolvedTarget::Model {
                    name: name.clone(),
                    definition_type: *definition_type,
                })
            }
            MqObjectDefinition::ModelQueue { .. } => Err(MqObjectError::UnsupportedCapability),
            _ => Err(MqObjectError::InvalidReferenceKind),
        }
    }

    fn resolve_topic(
        &self,
        name: &MqObjectName,
        capability: MqObjectCapability,
        context: &mut ResolutionContext,
    ) -> Result<MqResolvedTarget, MqObjectError> {
        let definition = self
            .objects
            .get(&ObjectKey {
                namespace: ObjectNamespace::Topic,
                name: name.clone(),
            })
            .ok_or(MqObjectError::UnknownObject)?;
        context.enter(
            Some(definition_key(definition)),
            MqObjectIdentity {
                kind: MqObjectKind::Topic,
                name: name.clone(),
            },
        )?;
        if !matches!(
            capability,
            MqObjectCapability::Publish | MqObjectCapability::Subscribe
        ) {
            return Err(MqObjectError::UnsupportedCapability);
        }
        Ok(MqResolvedTarget::Topic { name: name.clone() })
    }

    fn resolve_remote(
        &self,
        local_definition: &MqObjectName,
        remote_queue: &MqObjectName,
        initial_manager: &MqObjectName,
        initial_transmission: &Option<MqObjectName>,
        context: &mut ResolutionContext,
    ) -> Result<MqResolvedTarget, MqObjectError> {
        let mut remote_manager = initial_manager.clone();
        let mut transmission_queue = initial_transmission.clone();
        while let Some(definition) = self.queue_definition(&remote_manager) {
            let MqObjectDefinition::RemoteQueue {
                name,
                remote_queue: None,
                remote_queue_manager,
                transmission_queue: alias_transmission,
            } = definition
            else {
                return Err(MqObjectError::InvalidReferenceKind);
            };
            context.enter(
                Some(definition_key(definition)),
                MqObjectIdentity {
                    kind: MqObjectKind::RemoteQueue,
                    name: name.clone(),
                },
            )?;
            remote_manager = remote_queue_manager.clone();
            if alias_transmission.is_some() {
                transmission_queue = alias_transmission.clone();
            }
        }
        let transmission_queue = transmission_queue
            .or_else(|| self.queue_manager.default_transmission_queue.clone())
            .ok_or(MqObjectError::MissingReference)?;
        let transmission = self
            .queue_definition(&transmission_queue)
            .ok_or(MqObjectError::MissingReference)?;
        match transmission {
            MqObjectDefinition::LocalQueue {
                name,
                usage: MqLocalQueueUsage::Transmission,
                ..
            } => context.enter(
                Some(definition_key(transmission)),
                MqObjectIdentity {
                    kind: MqObjectKind::LocalQueue,
                    name: name.clone(),
                },
            )?,
            _ => return Err(MqObjectError::InvalidReferenceKind),
        }
        Ok(MqResolvedTarget::Remote(MqChannelRoute {
            local_definition: local_definition.clone(),
            remote_queue: remote_queue.clone(),
            remote_queue_manager: remote_manager,
            transmission_queue,
        }))
    }

    fn queue_definition(&self, name: &MqObjectName) -> Option<&MqObjectDefinition> {
        self.objects.get(&ObjectKey {
            namespace: ObjectNamespace::Queue,
            name: name.clone(),
        })
    }

    fn process_exists(&self, name: &MqObjectName) -> bool {
        self.objects.contains_key(&ObjectKey {
            namespace: ObjectNamespace::Process,
            name: name.clone(),
        })
    }

    fn validate_references(&self) -> Result<(), MqObjectError> {
        if let Some(transmission) = &self.queue_manager.default_transmission_queue {
            self.require_transmission_queue(transmission)?;
        }
        for definition in self.objects.values() {
            match definition {
                MqObjectDefinition::LocalQueue {
                    trigger_process, ..
                }
                | MqObjectDefinition::ModelQueue {
                    trigger_process, ..
                } => {
                    if trigger_process
                        .as_ref()
                        .is_some_and(|name| !self.process_exists(name))
                    {
                        return Err(MqObjectError::MissingReference);
                    }
                }
                MqObjectDefinition::AliasQueue { target, .. } => {
                    let exists = match target {
                        MqAliasTarget::Queue(name) => self.queue_definition(name).is_some(),
                        MqAliasTarget::Topic(name) => self.objects.contains_key(&ObjectKey {
                            namespace: ObjectNamespace::Topic,
                            name: name.clone(),
                        }),
                    };
                    if !exists {
                        return Err(MqObjectError::MissingReference);
                    }
                }
                MqObjectDefinition::RemoteQueue {
                    transmission_queue, ..
                } => {
                    if let Some(transmission_queue) = transmission_queue {
                        self.require_transmission_queue(transmission_queue)?;
                    }
                }
                MqObjectDefinition::Subscription {
                    topic, destination, ..
                } => {
                    if !self.objects.contains_key(&ObjectKey {
                        namespace: ObjectNamespace::Topic,
                        name: topic.clone(),
                    }) {
                        return Err(MqObjectError::MissingReference);
                    }
                    if let MqSubscriptionDestination::Queue(queue) = destination {
                        let mut context = ResolutionContext::new(self.limits.max_resolution_depth);
                        if self
                            .resolve_queue(queue, MqObjectCapability::Input, &mut context)
                            .is_err()
                        {
                            let mut context =
                                ResolutionContext::new(self.limits.max_resolution_depth);
                            self.resolve_queue(queue, MqObjectCapability::Output, &mut context)
                                .map_err(|problem| match problem {
                                    MqObjectError::UnknownObject => MqObjectError::MissingReference,
                                    other => other,
                                })?;
                        }
                    }
                }
                MqObjectDefinition::Topic { .. } | MqObjectDefinition::Process { .. } => {}
            }
        }
        for definition in self.objects.values() {
            match definition {
                MqObjectDefinition::AliasQueue { name, target } => {
                    let capability = match target {
                        MqAliasTarget::Queue(_) => MqObjectCapability::Output,
                        MqAliasTarget::Topic(_) => MqObjectCapability::Publish,
                    };
                    let mut context = ResolutionContext::new(self.limits.max_resolution_depth);
                    self.resolve_queue(name, capability, &mut context)?;
                }
                MqObjectDefinition::RemoteQueue {
                    name,
                    remote_queue: Some(_),
                    ..
                } => {
                    let mut context = ResolutionContext::new(self.limits.max_resolution_depth);
                    self.resolve_queue(name, MqObjectCapability::Output, &mut context)?;
                }
                MqObjectDefinition::RemoteQueue {
                    name,
                    remote_queue: None,
                    ..
                } => self.validate_manager_alias(name)?,
                _ => {}
            }
        }
        Ok(())
    }

    fn validate_manager_alias(&self, name: &MqObjectName) -> Result<(), MqObjectError> {
        let mut context = ResolutionContext::new(self.limits.max_resolution_depth);
        let mut current = name.clone();
        loop {
            let definition = self
                .queue_definition(&current)
                .ok_or(MqObjectError::MissingReference)?;
            let MqObjectDefinition::RemoteQueue {
                name,
                remote_queue: None,
                remote_queue_manager,
                ..
            } = definition
            else {
                return Err(MqObjectError::InvalidReferenceKind);
            };
            context.enter(
                Some(definition_key(definition)),
                MqObjectIdentity {
                    kind: MqObjectKind::RemoteQueue,
                    name: name.clone(),
                },
            )?;
            current = remote_queue_manager.clone();
            match self.queue_definition(&current) {
                Some(MqObjectDefinition::RemoteQueue {
                    remote_queue: None, ..
                }) => {}
                Some(_) => return Err(MqObjectError::InvalidReferenceKind),
                None => return Ok(()),
            }
        }
    }

    fn require_transmission_queue(&self, name: &MqObjectName) -> Result<(), MqObjectError> {
        match self.queue_definition(name) {
            Some(MqObjectDefinition::LocalQueue {
                usage: MqLocalQueueUsage::Transmission,
                ..
            }) => Ok(()),
            Some(_) => Err(MqObjectError::InvalidReferenceKind),
            None => Err(MqObjectError::MissingReference),
        }
    }

    fn allocate_dynamic_name(
        &self,
        pattern: &MqDynamicQueuePattern,
    ) -> Result<(u64, MqObjectName, u64), MqObjectError> {
        let Some(prefix) = pattern.wildcard_prefix() else {
            let name =
                MqObjectName::new(&pattern.0).map_err(|_| MqObjectError::InvalidDynamicPattern)?;
            if self.queue_definition(&name).is_some() || self.instances.contains_key(&name) {
                return Err(MqObjectError::NameInUse);
            }
            let next = self
                .next_dynamic_id
                .checked_add(1)
                .ok_or(MqObjectError::ResourceExhausted)?;
            return Ok((self.next_dynamic_id, name, next));
        };
        let attempts = self
            .limits
            .max_objects
            .checked_add(self.limits.max_dynamic_instances)
            .and_then(|value| value.checked_add(1))
            .ok_or(MqObjectError::ResourceExhausted)?;
        let mut candidate_id = self.next_dynamic_id;
        for _ in 0..attempts {
            let name = MqObjectName::new(format!("{prefix}{candidate_id:016X}"))
                .map_err(|_| MqObjectError::InvalidDynamicPattern)?;
            let next = candidate_id
                .checked_add(1)
                .ok_or(MqObjectError::ResourceExhausted)?;
            if self.queue_definition(&name).is_none() && !self.instances.contains_key(&name) {
                return Ok((candidate_id, name, next));
            }
            candidate_id = next;
        }
        Err(MqObjectError::ResourceExhausted)
    }
}

struct ResolutionContext {
    visited: BTreeSet<ObjectKey>,
    path: Vec<MqObjectIdentity>,
    max_depth: usize,
}

impl ResolutionContext {
    fn new(max_depth: usize) -> Self {
        Self {
            visited: BTreeSet::new(),
            path: Vec::new(),
            max_depth,
        }
    }

    fn enter(
        &mut self,
        key: Option<ObjectKey>,
        identity: MqObjectIdentity,
    ) -> Result<(), MqObjectError> {
        if self.path.len() >= self.max_depth {
            return Err(MqObjectError::ResolutionDepthExceeded);
        }
        if key.is_some_and(|key| !self.visited.insert(key)) {
            return Err(MqObjectError::ResolutionCycle);
        }
        self.path.push(identity);
        Ok(())
    }
}

fn validate_limits(limits: MqObjectLimits) -> Result<(), MqObjectError> {
    if limits.max_objects == 0
        || limits.max_dynamic_instances == 0
        || limits.max_resolution_depth == 0
        || limits.max_persisted_bytes == 0
    {
        Err(MqObjectError::InvalidLimits)
    } else {
        Ok(())
    }
}

fn local_queue_capabilities() -> BTreeSet<MqObjectCapability> {
    capability_set(&[
        MqObjectCapability::Input,
        MqObjectCapability::Browse,
        MqObjectCapability::Output,
        MqObjectCapability::Inquire,
        MqObjectCapability::Set,
    ])
}

fn capability_set(values: &[MqObjectCapability]) -> BTreeSet<MqObjectCapability> {
    values.iter().copied().collect()
}
