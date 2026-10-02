//! Pure, bounded queue-manager pub/sub state. The selected host route is pending.

mod codec;
mod lifecycle;
use lifecycle::handle_kernel_problem;
pub use lifecycle::{MqMessageHandleAccess, MqRegistryAccess};

use crate::{
    MqHandleKernel, MqHandleKernelProblem, MqObjectCapability, MqObjectCatalog, MqObjectDefinition,
    MqObjectError, MqObjectLookup, MqObjectName, MqSubscriptionDestination,
};
use mainframe_env_host_api::{
    MqDeliveryOutcome, MqHandleKind, MqHandleOwner, MqHandleProblem, MqHandleRegistry, MqHconn,
    MqHobj, MqHsub, MqMessage, MqMessageLimits, MqMessageProblem,
};
use std::collections::BTreeMap;

/// The kernel's durable format is independent of live MQ handles and callback identifiers.
pub const MQ_PUBSUB_SNAPSHOT_SCHEMA: &str = "mainframe-env.mq-pubsub@1";

/// Resource ceilings are product guards, not IBM numeric limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqPubsubLimits {
    pub max_subscriptions: usize,
    pub max_callbacks: usize,
    pub max_pending_per_subscription: usize,
    pub max_staged: usize,
    pub max_retained: usize,
    pub max_snapshot_bytes: usize,
    pub message: MqMessageLimits,
}

impl Default for MqPubsubLimits {
    fn default() -> Self {
        Self {
            max_subscriptions: 128,
            max_callbacks: 128,
            max_pending_per_subscription: 64,
            max_staged: 128,
            max_retained: 128,
            max_snapshot_bytes: 16 * 1024 * 1024,
            message: MqMessageLimits {
                body_bytes: 64 * 1024,
                ..MqMessageLimits::default()
            },
        }
    }
}

impl MqPubsubLimits {
    pub fn validate(self) -> Result<(), MqPubsubError> {
        let ceiling = Self::default();
        if self.max_subscriptions == 0
            || self.max_subscriptions > ceiling.max_subscriptions
            || self.max_callbacks == 0
            || self.max_callbacks > ceiling.max_callbacks
            || self.max_pending_per_subscription == 0
            || self.max_pending_per_subscription > ceiling.max_pending_per_subscription
            || self.max_staged == 0
            || self.max_staged > ceiling.max_staged
            || self.max_retained == 0
            || self.max_retained > ceiling.max_retained
            || self.max_snapshot_bytes == 0
            || self.max_snapshot_bytes > ceiling.max_snapshot_bytes
            || self.message.body_bytes > ceiling.message.body_bytes
            || self.message.validate().is_err()
        {
            return Err(MqPubsubError::InvalidLimits);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqPubsubError {
    InvalidLimits,
    Object(MqObjectError),
    Handle(MqHandleProblem),
    Message(MqMessageProblem),
    NotAuthorized,
    AlreadyExists,
    NoSubscription,
    NotDurable,
    NoCallback,
    NoCallbacksActive,
    NoRetainedPublication,
    InvalidState,
    ResourceExhausted,
    SequenceExhausted,
    CorruptSnapshot,
    UnsupportedSchema,
}

impl From<MqObjectError> for MqPubsubError {
    fn from(value: MqObjectError) -> Self {
        Self::Object(value)
    }
}
impl From<MqHandleProblem> for MqPubsubError {
    fn from(value: MqHandleProblem) -> Self {
        Self::Handle(value)
    }
}
impl From<MqMessageProblem> for MqPubsubError {
    fn from(value: MqMessageProblem) -> Self {
        Self::Message(value)
    }
}

/// The later SAF adapter must authorize every described resource before passing Permit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqPubsubAuthorization {
    Permit,
    Deny,
}

impl MqPubsubAuthorization {
    fn require(self) -> Result<(), MqPubsubError> {
        match self {
            Self::Permit => Ok(()),
            Self::Deny => Err(MqPubsubError::NotAuthorized),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqPubsubResource {
    pub lookup: MqObjectLookup,
    pub capability: MqObjectCapability,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqSubscriptionMode {
    Create { publications_on_request: bool },
    Resume,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqSubscriptionHandles {
    pub hobj: MqHobj,
    pub hsub: MqHsub,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqCallbackControl {
    Start,
    Stop,
    Quiesce,
    Suspend,
    Resume,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqCallbackState {
    Stopped,
    Started,
    Quiescing,
    Suspended,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MqPubsubEvent {
    Publication {
        sequence: u64,
        subscription: MqObjectName,
        callback_id: u64,
        message: MqMessage,
    },
    Trigger {
        sequence: u64,
        subscription: MqObjectName,
        process: MqObjectName,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Delivery {
    sequence: u64,
    message: MqMessage,
    outcome: MqDeliveryOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Trigger {
    sequence: u64,
    outcome: MqDeliveryOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Subscription {
    name: MqObjectName,
    topic: MqObjectName,
    destination: MqSubscriptionDestination,
    durable: bool,
    on_request: bool,
    pending: Vec<Delivery>,
    trigger: Option<Trigger>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct State {
    subscriptions: BTreeMap<MqObjectName, Subscription>,
    retained: BTreeMap<MqObjectName, MqMessage>,
    next_sequence: u64,
}

#[derive(Clone, Copy, Debug)]
struct Binding {
    owner: MqHandleOwner,
    hconn: MqHconn,
    handles: MqSubscriptionHandles,
}

#[derive(Clone, Copy, Debug)]
struct Callback {
    hconn: MqHconn,
    hobj: MqHobj,
    callback_id: u64,
    suspended: bool,
}

#[derive(Clone, Copy, Debug)]
struct ConnectionControl {
    owner: MqHandleOwner,
    hconn: MqHconn,
    state: MqCallbackState,
}

#[derive(Clone, Debug)]
enum Mutation {
    Publish {
        topic: MqObjectName,
        message: MqMessage,
        retain: bool,
    },
    Request {
        subscription: MqObjectName,
        message: MqMessage,
    },
}

/// One queue-manager state fence. Asynchronous work is returned as data for a later dispatcher.
pub struct MqPubsubKernel {
    catalog: MqObjectCatalog,
    limits: MqPubsubLimits,
    handles: MqHandleKernel,
    state: State,
    bindings: Vec<(MqObjectName, Binding)>,
    callbacks: Vec<Callback>,
    controls: Vec<ConnectionControl>,
    staged: Vec<(u64, Mutation)>,
}

impl MqPubsubKernel {
    pub fn new(
        catalog: MqObjectCatalog,
        limits: MqPubsubLimits,
        epoch: u64,
        max_handles: usize,
    ) -> Result<Self, MqPubsubError> {
        limits.validate()?;
        Ok(Self {
            catalog,
            limits,
            handles: MqHandleKernel::new(epoch, max_handles, limits.message)
                .map_err(handle_kernel_problem)?,
            state: State {
                subscriptions: BTreeMap::new(),
                retained: BTreeMap::new(),
                next_sequence: 1,
            },
            bindings: Vec::new(),
            callbacks: Vec::new(),
            controls: Vec::new(),
            staged: Vec::new(),
        })
    }

    pub fn describe_publish(
        &self,
        topic: &MqObjectName,
    ) -> Result<MqPubsubResource, MqPubsubError> {
        self.catalog.resolve(
            &MqObjectLookup::Topic(topic.clone()),
            MqObjectCapability::Publish,
        )?;
        Ok(MqPubsubResource {
            lookup: MqObjectLookup::Topic(topic.clone()),
            capability: MqObjectCapability::Publish,
        })
    }

    pub fn describe_subscribe(
        &self,
        name: &MqObjectName,
    ) -> Result<Vec<MqPubsubResource>, MqPubsubError> {
        let (topic, destination, _) = self.definition(name)?;
        self.catalog.resolve(
            &MqObjectLookup::Topic(topic.clone()),
            MqObjectCapability::Subscribe,
        )?;
        let mut resources = vec![MqPubsubResource {
            lookup: MqObjectLookup::Topic(topic),
            capability: MqObjectCapability::Subscribe,
        }];
        if let MqSubscriptionDestination::Queue(queue) = destination {
            self.catalog.resolve(
                &MqObjectLookup::Queue(queue.clone()),
                MqObjectCapability::Output,
            )?;
            resources.push(MqPubsubResource {
                lookup: MqObjectLookup::Queue(queue),
                capability: MqObjectCapability::Output,
            });
        }
        Ok(resources)
    }

    pub fn describe_request(&self, name: &MqObjectName) -> Result<MqPubsubResource, MqPubsubError> {
        self.catalog.resolve(
            &MqObjectLookup::Subscription(name.clone()),
            MqObjectCapability::RequestPublications,
        )?;
        Ok(MqPubsubResource {
            lookup: MqObjectLookup::Subscription(name.clone()),
            capability: MqObjectCapability::RequestPublications,
        })
    }

    fn definition(
        &self,
        name: &MqObjectName,
    ) -> Result<(MqObjectName, MqSubscriptionDestination, bool), MqPubsubError> {
        self.catalog
            .definitions()
            .find_map(|item| match item {
                MqObjectDefinition::Subscription {
                    name: candidate,
                    topic,
                    destination,
                    durable,
                } if candidate == name => Some((topic.clone(), destination.clone(), *durable)),
                _ => None,
            })
            .ok_or(MqPubsubError::NoSubscription)
    }

    pub fn subscribe(
        &mut self,
        owner: MqHandleOwner,
        hconn: MqHconn,
        name: &MqObjectName,
        mode: MqSubscriptionMode,
        authorization: MqPubsubAuthorization,
    ) -> Result<MqSubscriptionHandles, MqPubsubError> {
        authorization.require()?;
        self.handles.registry.validate_connection(owner, hconn)?;
        self.describe_subscribe(name)?;
        let (topic, destination, durable) = self.definition(name)?;
        match mode {
            MqSubscriptionMode::Create { .. } if self.state.subscriptions.contains_key(name) => {
                return Err(MqPubsubError::AlreadyExists);
            }
            MqSubscriptionMode::Create { .. }
                if self.state.subscriptions.len() >= self.limits.max_subscriptions =>
            {
                return Err(MqPubsubError::ResourceExhausted);
            }
            MqSubscriptionMode::Resume if !durable => return Err(MqPubsubError::NotDurable),
            MqSubscriptionMode::Resume if !self.state.subscriptions.contains_key(name) => {
                return Err(MqPubsubError::NoSubscription);
            }
            _ => {}
        }
        if self.bindings.len() >= self.limits.max_subscriptions {
            return Err(MqPubsubError::ResourceExhausted);
        }
        let created = match mode {
            MqSubscriptionMode::Create {
                publications_on_request,
            } => {
                let entry = Subscription {
                    name: name.clone(),
                    topic,
                    destination,
                    durable,
                    on_request: publications_on_request,
                    pending: Vec::new(),
                    trigger: None,
                };
                let mut candidate = self.state.clone();
                candidate.subscriptions.insert(name.clone(), entry.clone());
                self.validate_state(&candidate)?;
                self.snapshot_state(&candidate)?;
                Some(entry)
            }
            MqSubscriptionMode::Resume => None,
        };
        let hsub = self.handles.registry.create_subscription(owner, hconn)?;
        let hobj = match self.handles.registry.create_object(owner, hconn) {
            Ok(value) => value,
            Err(error) => {
                self.handles.registry.release(
                    owner,
                    hconn,
                    hsub.into(),
                    MqHandleKind::Subscription,
                )?;
                return Err(error.into());
            }
        };
        let handles = MqSubscriptionHandles { hobj, hsub };
        if let Some(entry) = created {
            self.state.subscriptions.insert(name.clone(), entry);
        }
        self.bindings.push((
            name.clone(),
            Binding {
                owner,
                hconn,
                handles,
            },
        ));
        Ok(handles)
    }

    pub fn close_subscription(
        &mut self,
        owner: MqHandleOwner,
        hconn: MqHconn,
        hsub: MqHsub,
    ) -> Result<(), MqPubsubError> {
        self.handles
            .registry
            .validate(owner, hconn, hsub.into(), MqHandleKind::Subscription)?;
        let index = self
            .bindings
            .iter()
            .position(|(_, binding)| binding.hconn == hconn && binding.handles.hsub == hsub)
            .ok_or(MqPubsubError::NoSubscription)?;
        let (name, binding) = &self.bindings[index];
        self.handles.registry.validate(
            owner,
            hconn,
            binding.handles.hobj.into(),
            MqHandleKind::Object,
        )?;
        let name = name.clone();
        let hobj = binding.handles.hobj;
        self.handles
            .registry
            .release(owner, hconn, hsub.into(), MqHandleKind::Subscription)?;
        self.handles
            .registry
            .release(owner, hconn, hobj.into(), MqHandleKind::Object)?;
        self.bindings.remove(index);
        self.callbacks.retain(|callback| callback.hobj != hobj);
        if self
            .state
            .subscriptions
            .get(&name)
            .is_some_and(|subscription| !subscription.durable)
            && !self.bindings.iter().any(|(bound, _)| bound == &name)
        {
            self.state.subscriptions.remove(&name);
        }
        Ok(())
    }

    pub fn register_callback(
        &mut self,
        owner: MqHandleOwner,
        hconn: MqHconn,
        hobj: MqHobj,
        callback_id: u64,
        authorization: MqPubsubAuthorization,
    ) -> Result<(), MqPubsubError> {
        authorization.require()?;
        self.handles
            .registry
            .validate(owner, hconn, hobj.into(), MqHandleKind::Object)?;
        if !self
            .bindings
            .iter()
            .any(|(_, binding)| binding.hconn == hconn && binding.handles.hobj == hobj)
        {
            return Err(MqPubsubError::NoSubscription);
        }
        if callback_id == 0 {
            // MQCB replacement errors deregister the prior callback for this Hobj.
            self.callbacks.retain(|item| item.hobj != hobj);
            return Err(MqPubsubError::InvalidState);
        }
        if let Some(callback) = self.callbacks.iter_mut().find(|item| item.hobj == hobj) {
            callback.callback_id = callback_id;
            callback.suspended = false;
        } else {
            if self.callbacks.len() >= self.limits.max_callbacks {
                return Err(MqPubsubError::ResourceExhausted);
            }
            self.callbacks.push(Callback {
                hconn,
                hobj,
                callback_id,
                suspended: false,
            });
        }
        Ok(())
    }

    pub fn deregister_callback(
        &mut self,
        owner: MqHandleOwner,
        hconn: MqHconn,
        hobj: MqHobj,
    ) -> Result<(), MqPubsubError> {
        self.handles
            .registry
            .validate(owner, hconn, hobj.into(), MqHandleKind::Object)?;
        let index = self
            .callbacks
            .iter()
            .position(|item| item.hconn == hconn && item.hobj == hobj)
            .ok_or(MqPubsubError::NoCallback)?;
        self.callbacks.remove(index);
        Ok(())
    }

    pub fn set_callback_suspended(
        &mut self,
        owner: MqHandleOwner,
        hconn: MqHconn,
        hobj: MqHobj,
        suspended: bool,
    ) -> Result<(), MqPubsubError> {
        self.handles
            .registry
            .validate(owner, hconn, hobj.into(), MqHandleKind::Object)?;
        let callback = self
            .callbacks
            .iter_mut()
            .find(|item| item.hconn == hconn && item.hobj == hobj)
            .ok_or(MqPubsubError::NoCallback)?;
        callback.suspended = suspended;
        Ok(())
    }

    pub fn control(
        &mut self,
        owner: MqHandleOwner,
        hconn: MqHconn,
        operation: MqCallbackControl,
    ) -> Result<MqCallbackState, MqPubsubError> {
        self.handles.registry.validate_connection(owner, hconn)?;
        let index = self.controls.iter().position(|item| item.hconn == hconn);
        let old = index.map_or(MqCallbackState::Stopped, |i| self.controls[i].state);
        let next = match operation {
            MqCallbackControl::Start if old == MqCallbackState::Stopped => {
                if !self
                    .callbacks
                    .iter()
                    .any(|item| item.hconn == hconn && !item.suspended)
                {
                    return Err(MqPubsubError::NoCallbacksActive);
                }
                MqCallbackState::Started
            }
            MqCallbackControl::Start if old == MqCallbackState::Quiescing => {
                MqCallbackState::Started
            }
            MqCallbackControl::Stop => MqCallbackState::Stopped,
            MqCallbackControl::Quiesce if old == MqCallbackState::Started => {
                MqCallbackState::Quiescing
            }
            MqCallbackControl::Suspend if old == MqCallbackState::Started => {
                MqCallbackState::Suspended
            }
            MqCallbackControl::Resume if old == MqCallbackState::Suspended => {
                MqCallbackState::Started
            }
            _ => return Err(MqPubsubError::InvalidState),
        };
        if next == MqCallbackState::Stopped {
            if let Some(i) = index {
                self.controls.remove(i);
            }
            return Ok(next);
        }
        if let Some(i) = index {
            self.controls[i].state = next;
        } else {
            if self.controls.len() >= self.limits.max_callbacks {
                return Err(MqPubsubError::ResourceExhausted);
            }
            self.controls.push(ConnectionControl {
                owner,
                hconn,
                state: next,
            });
        }
        Ok(next)
    }

    #[must_use]
    pub fn callback_state(&self, hconn: MqHconn) -> MqCallbackState {
        self.controls
            .iter()
            .find(|item| item.hconn == hconn)
            .map_or(MqCallbackState::Stopped, |item| item.state)
    }

    /// Stage by caller-owned UOW id, or apply now. Staging remains invisible until commit.
    pub fn publish(
        &mut self,
        topic: &MqObjectName,
        message: MqMessage,
        retain: bool,
        syncpoint: Option<u64>,
        authorization: MqPubsubAuthorization,
    ) -> Result<(), MqPubsubError> {
        authorization.require()?;
        self.describe_publish(topic)?;
        message.validate(self.limits.message)?;
        self.submit(
            Mutation::Publish {
                topic: topic.clone(),
                message,
                retain,
            },
            syncpoint,
        )
    }

    /// The request action sends the retained publication only to the named live subscription.
    pub fn request_publication(
        &mut self,
        owner: MqHandleOwner,
        hconn: MqHconn,
        hsub: MqHsub,
        syncpoint: Option<u64>,
        authorization: MqPubsubAuthorization,
    ) -> Result<(), MqPubsubError> {
        authorization.require()?;
        self.handles
            .registry
            .validate(owner, hconn, hsub.into(), MqHandleKind::Subscription)?;
        let name = self
            .bindings
            .iter()
            .find(|(_, binding)| binding.hconn == hconn && binding.handles.hsub == hsub)
            .map(|(name, _)| name.clone())
            .ok_or(MqPubsubError::NoSubscription)?;
        self.describe_request(&name)?;
        let subscription = self
            .state
            .subscriptions
            .get(&name)
            .ok_or(MqPubsubError::NoSubscription)?;
        if !subscription.on_request {
            return Err(MqPubsubError::InvalidState);
        }
        let message = self
            .state
            .retained
            .get(&subscription.topic)
            .cloned()
            .ok_or(MqPubsubError::NoRetainedPublication)?;
        self.submit(
            Mutation::Request {
                subscription: name,
                message,
            },
            syncpoint,
        )
    }

    fn submit(&mut self, mutation: Mutation, syncpoint: Option<u64>) -> Result<(), MqPubsubError> {
        if let Some(unit) = syncpoint {
            if unit == 0 {
                return Err(MqPubsubError::InvalidState);
            }
            if self.staged.len() >= self.limits.max_staged {
                return Err(MqPubsubError::ResourceExhausted);
            }
            self.staged.push((unit, mutation));
            return Ok(());
        }
        self.apply(&[mutation])
    }

    pub fn commit(&mut self, unit: u64) -> Result<usize, MqPubsubError> {
        if unit == 0 {
            return Err(MqPubsubError::InvalidState);
        }
        let actions: Vec<_> = self
            .staged
            .iter()
            .filter(|(id, _)| *id == unit)
            .map(|(_, action)| action.clone())
            .collect();
        self.apply(&actions)?;
        self.staged.retain(|(id, _)| *id != unit);
        Ok(actions.len())
    }

    pub fn backout(&mut self, unit: u64) -> Result<usize, MqPubsubError> {
        if unit == 0 {
            return Err(MqPubsubError::InvalidState);
        }
        let before = self.staged.len();
        self.staged.retain(|(id, _)| *id != unit);
        Ok(before - self.staged.len())
    }

    fn apply(&mut self, actions: &[Mutation]) -> Result<(), MqPubsubError> {
        let mut candidate = self.state.clone();
        for action in actions {
            match action {
                Mutation::Publish {
                    topic,
                    message,
                    retain,
                } => {
                    if *retain {
                        if !candidate.retained.contains_key(topic)
                            && candidate.retained.len() >= self.limits.max_retained
                        {
                            return Err(MqPubsubError::ResourceExhausted);
                        }
                        candidate.retained.insert(topic.clone(), message.clone());
                    }
                    let names: Vec<_> = candidate
                        .subscriptions
                        .values()
                        .filter(|item| &item.topic == topic)
                        .map(|item| item.name.clone())
                        .collect();
                    for name in names {
                        self.append(&mut candidate, &name, message.clone())?;
                    }
                }
                Mutation::Request {
                    subscription,
                    message,
                } => self.append(&mut candidate, subscription, message.clone())?,
            }
        }
        self.validate_state(&candidate)?;
        self.snapshot_state(&candidate)?;
        self.state = candidate;
        Ok(())
    }

    fn append(
        &self,
        state: &mut State,
        name: &MqObjectName,
        message: MqMessage,
    ) -> Result<(), MqPubsubError> {
        let subscription = state
            .subscriptions
            .get_mut(name)
            .ok_or(MqPubsubError::NoSubscription)?;
        if subscription.pending.len() >= self.limits.max_pending_per_subscription {
            return Err(MqPubsubError::ResourceExhausted);
        }
        let sequence = state.next_sequence;
        state.next_sequence = sequence
            .checked_add(1)
            .ok_or(MqPubsubError::SequenceExhausted)?;
        if subscription.pending.is_empty()
            && subscription.trigger.is_none()
            && self.trigger_process(&subscription.destination).is_some()
        {
            subscription.trigger = Some(Trigger {
                sequence,
                outcome: MqDeliveryOutcome::Pending,
            });
        }
        subscription.pending.push(Delivery {
            sequence,
            message,
            outcome: MqDeliveryOutcome::Pending,
        });
        Ok(())
    }

    fn trigger_process(&self, destination: &MqSubscriptionDestination) -> Option<MqObjectName> {
        let MqSubscriptionDestination::Queue(queue) = destination else {
            return None;
        };
        self.catalog
            .definitions()
            .find_map(|definition| match definition {
                MqObjectDefinition::LocalQueue {
                    name,
                    trigger_process,
                    ..
                } if name == queue => trigger_process.clone(),
                _ => None,
            })
    }

    /// Returns the earliest ready event and marks it unknown until an explicit settlement.
    /// The caller must never infer that an unacknowledged dispatch did not occur.
    pub fn next_event(&mut self) -> Result<Option<MqPubsubEvent>, MqPubsubError> {
        // Drop is not guaranteed (e.g. a caller may forget an access guard).
        // Never dispatch a callback whose registry lifetime has ended.
        self.reclaim_retired_handles();
        enum Ready {
            Trigger(MqObjectName, MqObjectName),
            Publication(MqObjectName, u64),
        }
        let mut ready: Vec<(u64, u8, Ready)> = Vec::new();
        for subscription in self.state.subscriptions.values() {
            if let Some(trigger) = &subscription.trigger
                && trigger.outcome == MqDeliveryOutcome::Pending
                && let Some(process) = self.trigger_process(&subscription.destination)
            {
                ready.push((
                    trigger.sequence,
                    0,
                    Ready::Trigger(subscription.name.clone(), process),
                ));
            }
            for delivery in &subscription.pending {
                if delivery.outcome != MqDeliveryOutcome::Pending {
                    continue;
                }
                let Some(callback) = self
                    .bindings
                    .iter()
                    .filter(|(name, _)| name == &subscription.name)
                    .find_map(|(_, binding)| {
                        self.callbacks.iter().find(|callback| {
                            callback.hconn == binding.hconn
                                && callback.hobj == binding.handles.hobj
                                && !callback.suspended
                                && self.callback_state(callback.hconn) == MqCallbackState::Started
                        })
                    })
                else {
                    continue;
                };
                ready.push((
                    delivery.sequence,
                    1,
                    Ready::Publication(subscription.name.clone(), callback.callback_id),
                ));
            }
        }
        ready.sort_by_key(|(sequence, kind, _)| (*sequence, *kind));
        let Some((sequence, _, selected)) = ready.into_iter().next() else {
            return Ok(None);
        };
        let mut candidate = self.state.clone();
        let event = match selected {
            Ready::Trigger(name, process) => {
                let trigger = candidate
                    .subscriptions
                    .get_mut(&name)
                    .and_then(|item| item.trigger.as_mut())
                    .ok_or(MqPubsubError::InvalidState)?;
                trigger.outcome = MqDeliveryOutcome::UnknownOutcome;
                MqPubsubEvent::Trigger {
                    sequence,
                    subscription: name,
                    process,
                }
            }
            Ready::Publication(name, callback_id) => {
                let delivery = self
                    .state
                    .subscriptions
                    .get(&name)
                    .and_then(|item| item.pending.iter().find(|item| item.sequence == sequence))
                    .ok_or(MqPubsubError::InvalidState)?;
                let message = delivery.message.clone();
                let delivery = candidate
                    .subscriptions
                    .get_mut(&name)
                    .ok_or(MqPubsubError::InvalidState)?
                    .pending
                    .iter_mut()
                    .find(|item| item.sequence == sequence)
                    .ok_or(MqPubsubError::InvalidState)?;
                delivery.outcome = MqDeliveryOutcome::UnknownOutcome;
                MqPubsubEvent::Publication {
                    sequence,
                    subscription: name,
                    callback_id,
                    message,
                }
            }
        };
        self.validate_state(&candidate)?;
        self.snapshot_state(&candidate)?;
        self.state = candidate;
        Ok(Some(event))
    }

    pub fn settle_event(
        &mut self,
        event: &MqPubsubEvent,
        outcome: MqDeliveryOutcome,
    ) -> Result<(), MqPubsubError> {
        if matches!(outcome, MqDeliveryOutcome::Pending) {
            return Err(MqPubsubError::InvalidState);
        }
        let (name, sequence) = match event {
            MqPubsubEvent::Publication {
                sequence,
                subscription,
                ..
            }
            | MqPubsubEvent::Trigger {
                sequence,
                subscription,
                ..
            } => (subscription, *sequence),
        };
        let mut candidate = self.state.clone();
        let subscription = candidate
            .subscriptions
            .get_mut(name)
            .ok_or(MqPubsubError::NoSubscription)?;
        match event {
            MqPubsubEvent::Publication { .. } => {
                let index = subscription
                    .pending
                    .iter()
                    .position(|item| item.sequence == sequence)
                    .ok_or(MqPubsubError::InvalidState)?;
                if subscription.pending[index].outcome != MqDeliveryOutcome::UnknownOutcome {
                    return Err(MqPubsubError::InvalidState);
                }
                if outcome == MqDeliveryOutcome::Accepted {
                    subscription.pending.remove(index);
                } else {
                    subscription.pending[index].outcome = outcome;
                }
            }
            MqPubsubEvent::Trigger { .. } => {
                let trigger = subscription
                    .trigger
                    .as_mut()
                    .filter(|item| item.sequence == sequence)
                    .ok_or(MqPubsubError::InvalidState)?;
                if trigger.outcome != MqDeliveryOutcome::UnknownOutcome {
                    return Err(MqPubsubError::InvalidState);
                }
                trigger.outcome = outcome.clone();
                if outcome == MqDeliveryOutcome::Accepted {
                    subscription.trigger = None;
                }
            }
        }
        self.validate_state(&candidate)?;
        self.snapshot_state(&candidate)?;
        self.state = candidate;
        Ok(())
    }

    /// An explicit retry is allowed only after a caller records duplicate risk.
    pub fn retry_event(&mut self, event: &MqPubsubEvent) -> Result<(), MqPubsubError> {
        let (name, sequence) = match event {
            MqPubsubEvent::Publication {
                sequence,
                subscription,
                ..
            }
            | MqPubsubEvent::Trigger {
                sequence,
                subscription,
                ..
            } => (subscription, *sequence),
        };
        let mut candidate = self.state.clone();
        let subscription = candidate
            .subscriptions
            .get_mut(name)
            .ok_or(MqPubsubError::NoSubscription)?;
        let outcome = match event {
            MqPubsubEvent::Publication { .. } => {
                &mut subscription
                    .pending
                    .iter_mut()
                    .find(|item| item.sequence == sequence)
                    .ok_or(MqPubsubError::InvalidState)?
                    .outcome
            }
            MqPubsubEvent::Trigger { .. } => {
                &mut subscription
                    .trigger
                    .as_mut()
                    .filter(|item| item.sequence == sequence)
                    .ok_or(MqPubsubError::InvalidState)?
                    .outcome
            }
        };
        if *outcome != MqDeliveryOutcome::DuplicatePossible {
            return Err(MqPubsubError::InvalidState);
        }
        *outcome = MqDeliveryOutcome::Pending;
        self.validate_state(&candidate)?;
        self.snapshot_state(&candidate)?;
        self.state = candidate;
        Ok(())
    }

    #[must_use]
    pub fn pending(&self, name: &MqObjectName) -> Option<Vec<(u64, MqDeliveryOutcome)>> {
        self.state.subscriptions.get(name).map(|item| {
            item.pending
                .iter()
                .map(|delivery| (delivery.sequence, delivery.outcome.clone()))
                .collect()
        })
    }

    #[must_use]
    pub fn trigger_state(&self, name: &MqObjectName) -> Option<(u64, MqDeliveryOutcome)> {
        self.state
            .subscriptions
            .get(name)
            .and_then(|item| item.trigger.as_ref())
            .map(|item| (item.sequence, item.outcome.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MqLocalQueueUsage, MqObjectLimits, MqQueueManagerDefinition};
    use mainframe_env_host_api::{
        MqExpiry, MqHandleSharing, MqHostEnvironment, MqMessageDescriptor, MqMessageIdentifiers,
        MqMessageOrdering, MqMessageProperty, MqPersistence, MqPriority, MqPropertyType,
    };

    fn name(value: &str) -> MqObjectName {
        MqObjectName::new(value).unwrap()
    }

    fn catalog() -> MqObjectCatalog {
        MqObjectCatalog::new(
            MqQueueManagerDefinition {
                name: name("QM"),
                default_transmission_queue: None,
            },
            vec![
                MqObjectDefinition::Topic {
                    name: name("TOPIC.A"),
                },
                MqObjectDefinition::LocalQueue {
                    name: name("Q.DEST"),
                    usage: MqLocalQueueUsage::Normal,
                    trigger_process: Some(name("PROC")),
                },
                MqObjectDefinition::Process { name: name("PROC") },
                MqObjectDefinition::Subscription {
                    name: name("SUB.D"),
                    topic: name("TOPIC.A"),
                    destination: MqSubscriptionDestination::Managed,
                    durable: true,
                },
                MqObjectDefinition::Subscription {
                    name: name("SUB.N"),
                    topic: name("TOPIC.A"),
                    destination: MqSubscriptionDestination::Queue(name("Q.DEST")),
                    durable: false,
                },
            ],
            MqObjectLimits::default(),
        )
        .unwrap()
    }

    fn owner() -> MqHandleOwner {
        MqHandleOwner {
            environment: MqHostEnvironment::MqiClient,
            host_id: 1,
            process_id: 1,
            thread_id: 1,
            task_id: 1,
            syncpoint_epoch: 1,
        }
    }

    fn message(body: &[u8]) -> MqMessage {
        MqMessage {
            descriptor: MqMessageDescriptor {
                identifiers: MqMessageIdentifiers::default(),
                format: None,
                expiry: MqExpiry::Unlimited,
                persistence: MqPersistence::Persistent,
                priority: MqPriority::QueueDefault,
                ordering: MqMessageOrdering::default(),
            },
            body: body.to_vec(),
            properties: Vec::new(),
        }
    }

    fn connected(limits: MqPubsubLimits, epoch: u64) -> (MqPubsubKernel, MqHconn) {
        let mut kernel = MqPubsubKernel::new(catalog(), limits, epoch, 16).unwrap();
        let hconn = kernel
            .handles_mut()
            .connect(owner(), MqHandleSharing::NonShared)
            .unwrap();
        (kernel, hconn)
    }

    #[test]
    fn invalid_limits_fail_before_kernel_creation() {
        let limits = MqPubsubLimits {
            max_subscriptions: 0,
            ..MqPubsubLimits::default()
        };
        assert_eq!(limits.validate(), Err(MqPubsubError::InvalidLimits));
    }

    #[test]
    fn restart_rejects_malformed_snapshot() {
        let error = MqPubsubKernel::restore(b"{}", catalog(), MqPubsubLimits::default(), 2, 16);
        assert!(matches!(error, Err(MqPubsubError::CorruptSnapshot)));
    }

    #[test]
    fn generic_resources_and_authorization_precede_mutation() {
        let (mut kernel, hconn) = connected(MqPubsubLimits::default(), 1);
        assert_eq!(kernel.describe_subscribe(&name("SUB.N")).unwrap().len(), 2);
        assert_eq!(kernel.describe_subscribe(&name("SUB.D")).unwrap().len(), 1);
        let before = kernel.snapshot().unwrap();
        assert_eq!(
            kernel.subscribe(
                owner(),
                hconn,
                &name("SUB.D"),
                MqSubscriptionMode::Create {
                    publications_on_request: false,
                },
                MqPubsubAuthorization::Deny,
            ),
            Err(MqPubsubError::NotAuthorized)
        );
        assert_eq!(kernel.snapshot().unwrap(), before);
        assert_eq!(
            kernel.publish(
                &name("TOPIC.A"),
                message(b"x"),
                true,
                None,
                MqPubsubAuthorization::Deny,
            ),
            Err(MqPubsubError::NotAuthorized)
        );
        assert_eq!(kernel.snapshot().unwrap(), before);
    }

    #[test]
    fn fanout_trigger_and_callback_order_are_deterministic() {
        let (mut kernel, hconn) = connected(MqPubsubLimits::default(), 1);
        let mut handles = Vec::new();
        for (subscription, callback) in [("SUB.D", 11), ("SUB.N", 22)] {
            let pair = kernel
                .subscribe(
                    owner(),
                    hconn,
                    &name(subscription),
                    MqSubscriptionMode::Create {
                        publications_on_request: false,
                    },
                    MqPubsubAuthorization::Permit,
                )
                .unwrap();
            kernel
                .register_callback(
                    owner(),
                    hconn,
                    pair.hobj,
                    callback,
                    MqPubsubAuthorization::Permit,
                )
                .unwrap();
            handles.push(pair);
        }
        assert_eq!(
            kernel.control(owner(), hconn, MqCallbackControl::Start),
            Ok(MqCallbackState::Started)
        );
        kernel
            .publish(
                &name("TOPIC.A"),
                message(b"one"),
                false,
                None,
                MqPubsubAuthorization::Permit,
            )
            .unwrap();
        let first = kernel.next_event().unwrap().unwrap();
        assert!(matches!(
            first,
            MqPubsubEvent::Publication {
                sequence: 1,
                callback_id: 11,
                ..
            }
        ));
        assert!(matches!(
            kernel.next_event().unwrap(),
            Some(MqPubsubEvent::Trigger { sequence: 2, .. })
        ));
        let third = kernel.next_event().unwrap().unwrap();
        assert!(matches!(
            third,
            MqPubsubEvent::Publication {
                sequence: 2,
                callback_id: 22,
                ..
            }
        ));
        assert!(kernel.next_event().unwrap().is_none());
        kernel
            .settle_event(&first, MqDeliveryOutcome::Accepted)
            .unwrap();
        kernel
            .settle_event(&third, MqDeliveryOutcome::Accepted)
            .unwrap();
        assert!(kernel.pending(&name("SUB.D")).unwrap().is_empty());
        assert!(kernel.pending(&name("SUB.N")).unwrap().is_empty());
        assert_eq!(handles.len(), 2);
    }

    #[test]
    fn syncpoint_overload_is_atomic_and_backout_discards_staging() {
        let limits = MqPubsubLimits {
            max_pending_per_subscription: 1,
            ..MqPubsubLimits::default()
        };
        let (mut kernel, hconn) = connected(limits, 1);
        kernel
            .subscribe(
                owner(),
                hconn,
                &name("SUB.D"),
                MqSubscriptionMode::Create {
                    publications_on_request: false,
                },
                MqPubsubAuthorization::Permit,
            )
            .unwrap();
        let before = kernel.snapshot().unwrap();
        for body in [b"a".as_slice(), b"b".as_slice()] {
            kernel
                .publish(
                    &name("TOPIC.A"),
                    message(body),
                    false,
                    Some(7),
                    MqPubsubAuthorization::Permit,
                )
                .unwrap();
        }
        assert_eq!(kernel.commit(7), Err(MqPubsubError::ResourceExhausted));
        assert_eq!(kernel.snapshot().unwrap(), before);
        assert_eq!(kernel.backout(7), Ok(2));
        assert_eq!(kernel.commit(7), Ok(0));
        assert_eq!(kernel.snapshot().unwrap(), before);
    }

    #[test]
    fn retained_request_duplicate_and_unknown_require_explicit_resolution() {
        let (mut kernel, hconn) = connected(MqPubsubLimits::default(), 1);
        kernel
            .publish(
                &name("TOPIC.A"),
                message(b"retained"),
                true,
                None,
                MqPubsubAuthorization::Permit,
            )
            .unwrap();
        let pair = kernel
            .subscribe(
                owner(),
                hconn,
                &name("SUB.D"),
                MqSubscriptionMode::Create {
                    publications_on_request: true,
                },
                MqPubsubAuthorization::Permit,
            )
            .unwrap();
        kernel
            .request_publication(
                owner(),
                hconn,
                pair.hsub,
                None,
                MqPubsubAuthorization::Permit,
            )
            .unwrap();
        kernel
            .register_callback(owner(), hconn, pair.hobj, 42, MqPubsubAuthorization::Permit)
            .unwrap();
        kernel
            .control(owner(), hconn, MqCallbackControl::Start)
            .unwrap();
        let event = kernel.next_event().unwrap().unwrap();
        assert_eq!(
            kernel.pending(&name("SUB.D")).unwrap()[0].1,
            MqDeliveryOutcome::UnknownOutcome
        );
        assert_eq!(kernel.retry_event(&event), Err(MqPubsubError::InvalidState));
        kernel
            .settle_event(&event, MqDeliveryOutcome::DuplicatePossible)
            .unwrap();
        kernel.retry_event(&event).unwrap();
        let retry = kernel.next_event().unwrap().unwrap();
        assert!(matches!(
            retry,
            MqPubsubEvent::Publication {
                callback_id: 42,
                ..
            }
        ));
        kernel
            .settle_event(&retry, MqDeliveryOutcome::Accepted)
            .unwrap();
        assert!(kernel.pending(&name("SUB.D")).unwrap().is_empty());
    }

    #[test]
    fn restart_retains_only_durable_state_and_requires_rebind() {
        let (mut kernel, hconn) = connected(MqPubsubLimits::default(), 1);
        let mut old = None;
        for subscription in ["SUB.D", "SUB.N"] {
            let pair = kernel
                .subscribe(
                    owner(),
                    hconn,
                    &name(subscription),
                    MqSubscriptionMode::Create {
                        publications_on_request: false,
                    },
                    MqPubsubAuthorization::Permit,
                )
                .unwrap();
            if subscription == "SUB.D" {
                old = Some(pair);
            }
        }
        kernel
            .publish(
                &name("TOPIC.A"),
                message(b"persist"),
                true,
                None,
                MqPubsubAuthorization::Permit,
            )
            .unwrap();
        let snapshot = kernel.snapshot().unwrap();
        let mut restarted =
            MqPubsubKernel::restore(&snapshot, catalog(), MqPubsubLimits::default(), 2, 16)
                .unwrap();
        assert_eq!(restarted.snapshot().unwrap(), snapshot);
        assert!(restarted.pending(&name("SUB.N")).is_none());
        assert_eq!(restarted.pending(&name("SUB.D")).unwrap().len(), 1);
        assert!(restarted.next_event().unwrap().is_none());
        let new_conn = restarted
            .handles_mut()
            .connect(owner(), MqHandleSharing::NonShared)
            .unwrap();
        assert!(matches!(
            restarted.register_callback(
                owner(),
                new_conn,
                old.unwrap().hobj,
                1,
                MqPubsubAuthorization::Permit
            ),
            Err(MqPubsubError::Handle(MqHandleProblem::Stale))
        ));
        let pair = restarted
            .subscribe(
                owner(),
                new_conn,
                &name("SUB.D"),
                MqSubscriptionMode::Resume,
                MqPubsubAuthorization::Permit,
            )
            .unwrap();
        restarted
            .register_callback(
                owner(),
                new_conn,
                pair.hobj,
                2,
                MqPubsubAuthorization::Permit,
            )
            .unwrap();
        restarted
            .control(owner(), new_conn, MqCallbackControl::Start)
            .unwrap();
        assert!(matches!(
            restarted.next_event().unwrap(),
            Some(MqPubsubEvent::Publication { callback_id: 2, .. })
        ));
    }

    #[test]
    fn callback_control_and_malformed_snapshot_fail_closed() {
        let (mut kernel, hconn) = connected(MqPubsubLimits::default(), 1);
        let pair = kernel
            .subscribe(
                owner(),
                hconn,
                &name("SUB.D"),
                MqSubscriptionMode::Create {
                    publications_on_request: false,
                },
                MqPubsubAuthorization::Permit,
            )
            .unwrap();
        assert_eq!(
            kernel.control(owner(), hconn, MqCallbackControl::Start),
            Err(MqPubsubError::NoCallbacksActive)
        );
        kernel
            .register_callback(owner(), hconn, pair.hobj, 8, MqPubsubAuthorization::Permit)
            .unwrap();
        kernel
            .control(owner(), hconn, MqCallbackControl::Start)
            .unwrap();
        kernel
            .control(owner(), hconn, MqCallbackControl::Suspend)
            .unwrap();
        kernel
            .control(owner(), hconn, MqCallbackControl::Resume)
            .unwrap();
        kernel
            .control(owner(), hconn, MqCallbackControl::Quiesce)
            .unwrap();
        kernel
            .control(owner(), hconn, MqCallbackControl::Stop)
            .unwrap();
        kernel
            .deregister_callback(owner(), hconn, pair.hobj)
            .unwrap();
        assert_eq!(
            kernel.deregister_callback(owner(), hconn, pair.hobj),
            Err(MqPubsubError::NoCallback)
        );
        let snapshot = kernel.snapshot().unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(&snapshot).unwrap();
        value["schema_version"] = serde_json::json!("mainframe-env.mq-pubsub@2");
        let unsupported = serde_json::to_vec(&value).unwrap();
        assert!(matches!(
            MqPubsubKernel::restore(&unsupported, catalog(), MqPubsubLimits::default(), 2, 16),
            Err(MqPubsubError::UnsupportedSchema)
        ));
        let mut value: serde_json::Value = serde_json::from_slice(&snapshot).unwrap();
        value["callback_pointer"] = serde_json::json!(123);
        let malformed = serde_json::to_vec(&value).unwrap();
        assert!(matches!(
            MqPubsubKernel::restore(&malformed, catalog(), MqPubsubLimits::default(), 2, 16),
            Err(MqPubsubError::CorruptSnapshot)
        ));
    }

    #[test]
    fn fanout_rejects_a_full_destination_without_partial_mutation() {
        let limits = MqPubsubLimits {
            max_pending_per_subscription: 1,
            ..MqPubsubLimits::default()
        };
        let (mut kernel, hconn) = connected(limits, 1);
        for value in ["SUB.D", "SUB.N"] {
            kernel
                .subscribe(
                    owner(),
                    hconn,
                    &name(value),
                    MqSubscriptionMode::Create {
                        publications_on_request: false,
                    },
                    MqPubsubAuthorization::Permit,
                )
                .unwrap();
        }
        kernel
            .publish(
                &name("TOPIC.A"),
                message(b"a"),
                false,
                None,
                MqPubsubAuthorization::Permit,
            )
            .unwrap();
        let before = kernel.snapshot().unwrap();
        assert_eq!(
            kernel.publish(
                &name("TOPIC.A"),
                message(b"b"),
                false,
                None,
                MqPubsubAuthorization::Permit
            ),
            Err(MqPubsubError::ResourceExhausted)
        );
        assert_eq!(kernel.snapshot().unwrap(), before);
        assert_eq!(kernel.pending(&name("SUB.D")).unwrap().len(), 1);
        assert_eq!(kernel.pending(&name("SUB.N")).unwrap().len(), 1);
    }

    #[test]
    fn lifecycle_bounds_and_snapshot_corruption_are_rejected() {
        let limits = MqPubsubLimits {
            max_snapshot_bytes: 1,
            ..MqPubsubLimits::default()
        };
        let (mut tiny, hconn) = connected(limits, 1);
        let before_handles = tiny.handles_mut().active_handles();
        assert_eq!(
            tiny.subscribe(
                owner(),
                hconn,
                &name("SUB.D"),
                MqSubscriptionMode::Create {
                    publications_on_request: false
                },
                MqPubsubAuthorization::Permit,
            ),
            Err(MqPubsubError::ResourceExhausted)
        );
        assert_eq!(tiny.handles_mut().active_handles(), before_handles);

        let (mut kernel, hconn) = connected(MqPubsubLimits::default(), 1);
        let nondurable = kernel
            .subscribe(
                owner(),
                hconn,
                &name("SUB.N"),
                MqSubscriptionMode::Create {
                    publications_on_request: false,
                },
                MqPubsubAuthorization::Permit,
            )
            .unwrap();
        kernel
            .close_subscription(owner(), hconn, nondurable.hsub)
            .unwrap();
        assert!(kernel.pending(&name("SUB.N")).is_none());
        assert!(matches!(
            kernel.register_callback(
                owner(),
                hconn,
                nondurable.hobj,
                4,
                MqPubsubAuthorization::Permit
            ),
            Err(MqPubsubError::Handle(MqHandleProblem::Stale))
        ));
        kernel
            .subscribe(
                owner(),
                hconn,
                &name("SUB.D"),
                MqSubscriptionMode::Create {
                    publications_on_request: false,
                },
                MqPubsubAuthorization::Permit,
            )
            .unwrap();
        let snapshot = kernel.snapshot().unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(&snapshot).unwrap();
        value["subscriptions"][0]["durable"] = serde_json::json!(false);
        let malformed = serde_json::to_vec(&value).unwrap();
        assert!(matches!(
            MqPubsubKernel::restore(&malformed, catalog(), MqPubsubLimits::default(), 2, 16),
            Err(MqPubsubError::CorruptSnapshot)
        ));
        let mut value: serde_json::Value = serde_json::from_slice(&snapshot).unwrap();
        let duplicate = value["subscriptions"][0].clone();
        value["subscriptions"]
            .as_array_mut()
            .unwrap()
            .push(duplicate);
        let malformed = serde_json::to_vec(&value).unwrap();
        assert!(matches!(
            MqPubsubKernel::restore(&malformed, catalog(), MqPubsubLimits::default(), 2, 16),
            Err(MqPubsubError::CorruptSnapshot)
        ));
    }

    #[test]
    fn callback_replacement_error_deregisters_and_property_snapshot_is_strict() {
        let (mut kernel, hconn) = connected(MqPubsubLimits::default(), 1);
        let pair = kernel
            .subscribe(
                owner(),
                hconn,
                &name("SUB.D"),
                MqSubscriptionMode::Create {
                    publications_on_request: false,
                },
                MqPubsubAuthorization::Permit,
            )
            .unwrap();
        kernel
            .register_callback(owner(), hconn, pair.hobj, 17, MqPubsubAuthorization::Permit)
            .unwrap();
        assert_eq!(
            kernel.register_callback(owner(), hconn, pair.hobj, 0, MqPubsubAuthorization::Permit),
            Err(MqPubsubError::InvalidState)
        );
        assert_eq!(
            kernel.deregister_callback(owner(), hconn, pair.hobj),
            Err(MqPubsubError::NoCallback)
        );
        let mut publication = message(b"payload");
        publication.properties.push(MqMessageProperty {
            name: "kind".into(),
            kind: MqPropertyType::String,
            value: b"sample".to_vec(),
        });
        kernel
            .publish(
                &name("TOPIC.A"),
                publication.clone(),
                true,
                None,
                MqPubsubAuthorization::Permit,
            )
            .unwrap();
        let bytes = kernel.snapshot().unwrap();
        let restored =
            MqPubsubKernel::restore(&bytes, catalog(), MqPubsubLimits::default(), 2, 16).unwrap();
        assert_eq!(
            restored.state.retained.get(&name("TOPIC.A")),
            Some(&publication)
        );
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        value["retained"][0]["message"]["properties"][0]["kind"] = serde_json::json!(255);
        assert!(matches!(
            MqPubsubKernel::restore(
                &serde_json::to_vec(&value).unwrap(),
                catalog(),
                MqPubsubLimits::default(),
                2,
                16
            ),
            Err(MqPubsubError::CorruptSnapshot)
        ));
    }
}
