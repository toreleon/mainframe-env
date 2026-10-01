//! Bounded request-side MQOPEN/MQCLOSE object intent. No handler or routing authority lives here.
//!
//! A provider must resolve the lookup with its `MqObjectCatalog`, check SAF before mutation,
//! and bind/release tokens with `MqHandleRegistry`. Caller-supplied lifecycle is an expectation,
//! never proof that deletion is permitted. These symbolic values are not IBM wire integers.

use crate::{MqHconn, MqHobj, MqHsub};

/// IBM MQ 9.4 MQI baseline `ibm-mq-9.4-mqi-2026-08-31`, catalog row 0019.
pub const MQ_OBJECT_OPEN_SOURCE: MqObjectRouteSource = MqObjectRouteSource {
    row: "ibm-mq-9.4-mqi-2026-08-31:mqi-calls-unique:0019",
    topic: "SSFKSJ_9.4.0/refdev/q101870_.html",
    sha256: "b6f2f3659ca1e91fff52918d17a607bab29ef45cbd473b4a89c1d1c1c2ff8347",
};
/// IBM MQ 9.4 MQI baseline `ibm-mq-9.4-mqi-2026-08-31`, catalog row 0006.
pub const MQ_OBJECT_CLOSE_SOURCE: MqObjectRouteSource = MqObjectRouteSource {
    row: "ibm-mq-9.4-mqi-2026-08-31:mqi-calls-unique:0006",
    topic: "SSFKSJ_9.4.0/refdev/q101740_.html",
    sha256: "28003da6981b7913cb0d88f8b250d3debc14682bac929a895e7e1be765853b7b",
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqObjectRouteSource {
    pub row: &'static str,
    pub topic: &'static str,
    pub sha256: &'static str,
}

/// Contract ceilings, not claims about every MQOD version or IBM wire field.
pub const MQ_ROUTE_NAME_BYTES: usize = 48;
pub const MQ_ROUTE_DYNAMIC_SUFFIX_BYTES: usize = 16;
pub const MQ_ROUTE_TOPIC_STRING_BYTES: usize = 4_096;
pub const MQ_ROUTE_USER_ID_BYTES: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqObjectRouteError {
    InvalidName,
    InvalidPattern,
    InvalidTopicString,
    InvalidAlternateUser,
    MissingAccess,
    DuplicateAccess,
    IncompatibleAccess,
    IncompatibleModifier,
    InvalidObjectForm,
    InvalidCloseMode,
    InvalidConnection,
}

fn name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'/' | b'_' | b'%')
}

/// Canonical, case-sensitive request name. A wire MQOD adapter handles padding separately.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqRouteName(String);

impl MqRouteName {
    pub fn new(value: impl AsRef<str>) -> Result<Self, MqObjectRouteError> {
        let value = value.as_ref();
        if value.is_empty() || value.len() > MQ_ROUTE_NAME_BYTES || !value.bytes().all(name_byte) {
            return Err(MqObjectRouteError::InvalidName);
        }
        Ok(Self(value.into()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A model-queue dynamic name pattern: an exact name or one final `*`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqRouteDynamicPattern(String);

impl MqRouteDynamicPattern {
    pub fn new(value: impl AsRef<str>) -> Result<Self, MqObjectRouteError> {
        let value = value.as_ref();
        if let Some(prefix) = value.strip_suffix('*') {
            if prefix.is_empty()
                || prefix.len() + MQ_ROUTE_DYNAMIC_SUFFIX_BYTES > MQ_ROUTE_NAME_BYTES
                || !prefix.bytes().all(name_byte)
            {
                return Err(MqObjectRouteError::InvalidPattern);
            }
        } else {
            MqRouteName::new(value).map_err(|_| MqObjectRouteError::InvalidPattern)?;
        }
        Ok(Self(value.into()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn accepts(&self, name: &MqRouteName) -> bool {
        self.0.strip_suffix('*').map_or_else(
            || self.0 == name.as_str(),
            |prefix| {
                name.as_str().starts_with(prefix)
                    && name.as_str().len() == prefix.len() + MQ_ROUTE_DYNAMIC_SUFFIX_BYTES
            },
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqRouteTopicString(String);

impl MqRouteTopicString {
    pub fn new(value: impl AsRef<str>) -> Result<Self, MqObjectRouteError> {
        let value = value.as_ref();
        if value.is_empty()
            || value.len() > MQ_ROUTE_TOPIC_STRING_BYTES
            || value.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(MqObjectRouteError::InvalidTopicString);
        }
        Ok(Self(value.into()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqRouteAlternateUser(String);

impl MqRouteAlternateUser {
    pub fn new(value: impl AsRef<str>) -> Result<Self, MqObjectRouteError> {
        let value = value.as_ref();
        if value.is_empty()
            || value.len() > MQ_ROUTE_USER_ID_BYTES
            || value.bytes().any(|byte| !byte.is_ascii_graphic())
        {
            return Err(MqObjectRouteError::InvalidAlternateUser);
        }
        Ok(Self(value.into()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The lookup namespace; actual object kind and resolution come from the catalog.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqRouteObjectType {
    Queue,
    Topic,
    Process,
    QueueManager,
    Namelist,
    DistributionList,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MqRouteLookup {
    Queue {
        name: MqRouteName,
        /// A named queue manager requests remote resolution, not a claimed route.
        manager: Option<MqRouteName>,
        /// Permitted only if the catalog finds a model queue.
        dynamic_pattern: Option<MqRouteDynamicPattern>,
    },
    Topic {
        name: MqRouteName,
        object_string: Option<MqRouteTopicString>,
    },
    Process(MqRouteName),
    QueueManager,
    /// Source-recognized forms without a corresponding current catalog route.
    Namelist(MqRouteName),
    DistributionList,
}

impl MqRouteLookup {
    #[must_use]
    pub const fn object_type(&self) -> MqRouteObjectType {
        match self {
            Self::Queue { .. } => MqRouteObjectType::Queue,
            Self::Topic { .. } => MqRouteObjectType::Topic,
            Self::Process(_) => MqRouteObjectType::Process,
            Self::QueueManager => MqRouteObjectType::QueueManager,
            Self::Namelist(_) => MqRouteObjectType::Namelist,
            Self::DistributionList => MqRouteObjectType::DistributionList,
        }
    }
}

/// One symbolic MQOO access capability. Multiple distinct capabilities may be requested.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqRouteOpenAccess {
    InputAsQueueDefault,
    InputShared,
    InputExclusive,
    Browse,
    Output,
    Inquire,
    Set,
}

impl MqRouteOpenAccess {
    const fn is_input(self) -> bool {
        matches!(
            self,
            Self::InputAsQueueDefault | Self::InputShared | Self::InputExclusive
        )
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MqRouteContextOutput {
    #[default]
    None,
    PassIdentity,
    PassAll,
    SetIdentity,
    SetAll,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MqRouteContextIntent {
    pub save_all_from_input: bool,
    pub output: MqRouteContextOutput,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MqRouteOpenModifiers {
    pub cooperative_browse: bool,
    pub resolve_local_queue: bool,
    pub resolve_local_topic: bool,
    pub no_multicast: bool,
    pub alternate_user: Option<MqRouteAlternateUser>,
    pub context: MqRouteContextIntent,
}

/// A constructed request is structurally valid, but has no execution authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqObjectOpenRequest {
    connection: MqHconn,
    lookup: MqRouteLookup,
    access: Vec<MqRouteOpenAccess>,
    modifiers: MqRouteOpenModifiers,
}

impl MqObjectOpenRequest {
    pub fn new(
        connection: MqHconn,
        lookup: MqRouteLookup,
        access: &[MqRouteOpenAccess],
        modifiers: MqRouteOpenModifiers,
    ) -> Result<Self, MqObjectRouteError> {
        use MqObjectRouteError as E;
        if connection == MqHconn::Unassociated {
            return Err(E::InvalidConnection);
        }
        if access.is_empty() {
            return Err(E::MissingAccess);
        }
        if access.len() > 7 {
            return Err(E::DuplicateAccess);
        }
        for (index, item) in access.iter().enumerate() {
            if access[..index].contains(item) {
                return Err(E::DuplicateAccess);
            }
        }
        if access.iter().filter(|item| item.is_input()).count() > 1 {
            return Err(E::IncompatibleAccess);
        }
        let has = |wanted| access.contains(&wanted);
        let input = access.iter().any(|item| item.is_input());
        let output = has(MqRouteOpenAccess::Output);
        if (modifiers.cooperative_browse && !has(MqRouteOpenAccess::Browse))
            || (modifiers.context.save_all_from_input && !input)
            || (modifiers.context.output != MqRouteContextOutput::None && !output)
            || (modifiers.no_multicast && !output)
            || (modifiers.resolve_local_queue && modifiers.resolve_local_topic)
        {
            return Err(E::IncompatibleModifier);
        }
        match &lookup {
            MqRouteLookup::Queue {
                manager,
                dynamic_pattern,
                ..
            } => {
                if modifiers.resolve_local_topic || modifiers.no_multicast {
                    return Err(E::IncompatibleModifier);
                }
                if manager.is_some() && dynamic_pattern.is_some() {
                    return Err(E::InvalidObjectForm);
                }
                if manager.is_some() && (input || has(MqRouteOpenAccess::Browse)) {
                    return Err(E::IncompatibleAccess);
                }
            }
            MqRouteLookup::Topic { .. } => {
                if access != [MqRouteOpenAccess::Output]
                    || modifiers.resolve_local_queue
                    || modifiers.cooperative_browse
                    || modifiers.context.save_all_from_input
                {
                    return Err(E::IncompatibleAccess);
                }
            }
            MqRouteLookup::Process(_)
            | MqRouteLookup::QueueManager
            | MqRouteLookup::Namelist(_) => {
                if access != [MqRouteOpenAccess::Inquire] {
                    return Err(E::IncompatibleAccess);
                }
                if modifiers.resolve_local_queue
                    || modifiers.resolve_local_topic
                    || modifiers.no_multicast
                {
                    return Err(E::IncompatibleModifier);
                }
            }
            MqRouteLookup::DistributionList => {
                if !output || access.len() != 1 {
                    return Err(E::IncompatibleAccess);
                }
            }
        }
        Ok(Self {
            connection,
            lookup,
            access: access.to_vec(),
            modifiers,
        })
    }

    #[must_use]
    pub const fn connection(&self) -> MqHconn {
        self.connection
    }

    #[must_use]
    pub const fn lookup(&self) -> &MqRouteLookup {
        &self.lookup
    }

    #[must_use]
    pub fn access(&self) -> &[MqRouteOpenAccess] {
        &self.access
    }

    #[must_use]
    pub const fn modifiers(&self) -> &MqRouteOpenModifiers {
        &self.modifiers
    }

    #[must_use]
    pub fn review(&self) -> MqObjectRouteReview {
        let mut pending = vec![
            MqObjectRoutePending::CatalogResolution,
            MqObjectRoutePending::Authorization,
            MqObjectRoutePending::HandleRegistry,
            MqObjectRoutePending::WireOptionLegality,
            MqObjectRoutePending::ExecutionIntegration,
        ];
        let unsupported = match &self.lookup {
            MqRouteLookup::Namelist(_) => Some(MqObjectRouteUnsupported::NamelistCatalog),
            MqRouteLookup::DistributionList => Some(MqObjectRouteUnsupported::DistributionList),
            MqRouteLookup::Queue {
                manager,
                dynamic_pattern,
                ..
            } => {
                if manager.is_some() {
                    pending.push(MqObjectRoutePending::RemoteChannel);
                }
                if dynamic_pattern.is_some() {
                    pending.push(MqObjectRoutePending::ModelLifecycle);
                }
                None
            }
            MqRouteLookup::Topic { .. } => {
                pending.push(MqObjectRoutePending::TopicRouting);
                pending.push(MqObjectRoutePending::ObjectDescriptorVersion);
                None
            }
            _ => None,
        };
        MqObjectRouteReview {
            source: MQ_OBJECT_OPEN_SOURCE,
            disposition: unsupported.map_or(
                MqObjectRouteDisposition::Pending(pending),
                MqObjectRouteDisposition::Unsupported,
            ),
        }
    }
}

/// Result shape for a model open; the provider must derive and verify every field.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqDynamicQueueOpenResult {
    pub handle: MqHobj,
    pub model: MqRouteName,
    pub name: MqRouteName,
    pub kind: MqRouteDynamicKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqRouteDynamicKind {
    Temporary,
    Permanent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqRouteCloseMode {
    None,
    Delete,
    DeletePurge,
    KeepSubscription,
    RemoveSubscription,
    Immediate,
    Quiesce,
}

/// Expected object lifecycle. The catalog and registry must independently establish it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqRouteCloseLifecycle {
    Unknown,
    Predefined,
    TemporaryDynamic,
    PermanentDynamic,
    ManagedDestination,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqRouteCloseTarget {
    Object {
        handle: MqHobj,
        lifecycle: MqRouteCloseLifecycle,
    },
    Subscription {
        handle: MqHsub,
        durable: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqObjectCloseRequest {
    connection: MqHconn,
    target: MqRouteCloseTarget,
    mode: MqRouteCloseMode,
}

impl MqObjectCloseRequest {
    pub fn new(
        connection: MqHconn,
        target: MqRouteCloseTarget,
        mode: MqRouteCloseMode,
    ) -> Result<Self, MqObjectRouteError> {
        use MqRouteCloseMode as M;
        if connection == MqHconn::Unassociated {
            return Err(MqObjectRouteError::InvalidConnection);
        }
        match (target, mode) {
            (MqRouteCloseTarget::Object { lifecycle, .. }, M::Delete | M::DeletePurge)
                if !matches!(
                    lifecycle,
                    MqRouteCloseLifecycle::TemporaryDynamic
                        | MqRouteCloseLifecycle::PermanentDynamic
                        | MqRouteCloseLifecycle::Unknown
                ) =>
            {
                return Err(MqObjectRouteError::InvalidCloseMode);
            }
            (MqRouteCloseTarget::Object { .. }, M::KeepSubscription | M::RemoveSubscription)
            | (
                MqRouteCloseTarget::Subscription { .. },
                M::Delete | M::DeletePurge | M::Immediate | M::Quiesce,
            ) => {
                return Err(MqObjectRouteError::InvalidCloseMode);
            }
            (MqRouteCloseTarget::Subscription { durable: false, .. }, M::KeepSubscription) => {
                return Err(MqObjectRouteError::InvalidCloseMode);
            }
            _ => {}
        }
        Ok(Self {
            connection,
            target,
            mode,
        })
    }

    #[must_use]
    pub const fn connection(&self) -> MqHconn {
        self.connection
    }

    #[must_use]
    pub const fn target(&self) -> MqRouteCloseTarget {
        self.target
    }

    #[must_use]
    pub const fn mode(&self) -> MqRouteCloseMode {
        self.mode
    }

    #[must_use]
    pub fn review(&self) -> MqObjectRouteReview {
        let unsupported = if matches!(
            self.mode,
            MqRouteCloseMode::Immediate | MqRouteCloseMode::Quiesce
        ) {
            Some(MqObjectRouteUnsupported::ClientReadAhead)
        } else {
            None
        };
        let mut pending = vec![
            MqObjectRoutePending::CatalogResolution,
            MqObjectRoutePending::Authorization,
            MqObjectRoutePending::HandleRegistry,
            MqObjectRoutePending::WireOptionLegality,
            MqObjectRoutePending::ExecutionIntegration,
        ];
        if matches!(
            self.target,
            MqRouteCloseTarget::Object {
                lifecycle: MqRouteCloseLifecycle::TemporaryDynamic
                    | MqRouteCloseLifecycle::PermanentDynamic
                    | MqRouteCloseLifecycle::Unknown,
                ..
            }
        ) {
            pending.push(MqObjectRoutePending::ModelLifecycle);
        }
        MqObjectRouteReview {
            source: MQ_OBJECT_CLOSE_SOURCE,
            disposition: unsupported.map_or(
                MqObjectRouteDisposition::Pending(pending),
                MqObjectRouteDisposition::Unsupported,
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqObjectRoutePending {
    CatalogResolution,
    Authorization,
    HandleRegistry,
    ObjectDescriptorVersion,
    ModelLifecycle,
    RemoteChannel,
    TopicRouting,
    WireOptionLegality,
    ExecutionIntegration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqObjectRouteUnsupported {
    NamelistCatalog,
    DistributionList,
    ClientReadAhead,
}

/// Every valid review is either explicitly unsupported or pending implementation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MqObjectRouteDisposition {
    Unsupported(MqObjectRouteUnsupported),
    Pending(Vec<MqObjectRoutePending>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqObjectRouteReview {
    pub source: MqObjectRouteSource,
    pub disposition: MqObjectRouteDisposition,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mq_mqi_contract_by_label;

    fn name(value: &str) -> MqRouteName {
        MqRouteName::new(value).unwrap()
    }

    fn queue() -> MqRouteLookup {
        MqRouteLookup::Queue {
            name: name("APP.Q"),
            manager: None,
            dynamic_pattern: None,
        }
    }

    #[test]
    fn pins_match_generated_mqi_contracts() {
        for (label, source) in [
            ("MQOPEN", MQ_OBJECT_OPEN_SOURCE),
            ("MQCLOSE", MQ_OBJECT_CLOSE_SOURCE),
        ] {
            let contract = mq_mqi_contract_by_label(label).unwrap();
            assert_eq!(contract.official_row, source.row);
            assert_eq!(contract.topic_path, source.topic);
            assert_eq!(contract.topic_sha256, source.sha256);
        }
    }

    #[test]
    fn bounded_names_patterns_and_identity_are_case_sensitive() {
        assert_eq!(name("app.Q").as_str(), "app.Q");
        for bad in ["", " APP.Q", "APP Q", "APP\0Q", "é", &"A".repeat(49)] {
            assert_eq!(MqRouteName::new(bad), Err(MqObjectRouteError::InvalidName));
        }
        for bad in ["*", "A**", "A*B", &format!("{}*", "A".repeat(33))] {
            assert_eq!(
                MqRouteDynamicPattern::new(bad),
                Err(MqObjectRouteError::InvalidPattern)
            );
        }
        let pattern = MqRouteDynamicPattern::new("APP.*").unwrap();
        assert!(pattern.accepts(&name(&format!("APP.{}", "A".repeat(16)))));
        assert!(!pattern.accepts(&name("app.A")));
        assert_eq!(
            MqRouteTopicString::new("bad\0topic"),
            Err(MqObjectRouteError::InvalidTopicString)
        );
        assert_eq!(
            MqRouteAlternateUser::new("bad user"),
            Err(MqObjectRouteError::InvalidAlternateUser)
        );
    }

    #[test]
    fn open_rejects_incompatible_shapes_before_review() {
        use MqRouteOpenAccess as A;
        let make = |lookup, access: &[A], modifiers| {
            MqObjectOpenRequest::new(MqHconn::Default, lookup, access, modifiers)
        };
        let empty = MqRouteOpenModifiers::default();
        assert_eq!(
            make(queue(), &[], empty.clone()),
            Err(MqObjectRouteError::MissingAccess)
        );
        assert_eq!(
            make(queue(), &[A::Output, A::Output], empty.clone()),
            Err(MqObjectRouteError::DuplicateAccess)
        );
        assert_eq!(
            make(queue(), &[A::InputShared, A::InputExclusive], empty.clone()),
            Err(MqObjectRouteError::IncompatibleAccess)
        );
        assert_eq!(
            make(
                queue(),
                &[A::Output],
                MqRouteOpenModifiers {
                    cooperative_browse: true,
                    ..empty.clone()
                }
            ),
            Err(MqObjectRouteError::IncompatibleModifier)
        );
        assert_eq!(
            make(
                queue(),
                &[A::Output],
                MqRouteOpenModifiers {
                    context: MqRouteContextIntent {
                        save_all_from_input: true,
                        ..Default::default()
                    },
                    ..empty.clone()
                }
            ),
            Err(MqObjectRouteError::IncompatibleModifier)
        );
        assert_eq!(
            make(
                MqRouteLookup::Topic {
                    name: name("TOPIC"),
                    object_string: None
                },
                &[A::Browse],
                empty.clone()
            ),
            Err(MqObjectRouteError::IncompatibleAccess)
        );
        assert_eq!(
            make(
                MqRouteLookup::Queue {
                    name: name("R.Q"),
                    manager: Some(name("OTHER")),
                    dynamic_pattern: Some(MqRouteDynamicPattern::new("D.*").unwrap())
                },
                &[A::Output],
                empty
            ),
            Err(MqObjectRouteError::InvalidObjectForm)
        );
    }

    #[test]
    fn reviews_separate_unsupported_from_pending() {
        use MqRouteOpenAccess as A;
        let open = |lookup, access| {
            MqObjectOpenRequest::new(MqHconn::Default, lookup, access, Default::default())
                .unwrap()
                .review()
        };
        let local = open(queue(), &[A::InputShared, A::Browse]);
        assert!(
            matches!(local.disposition, MqObjectRouteDisposition::Pending(ref reasons) if reasons.contains(&MqObjectRoutePending::CatalogResolution))
        );
        let remote = open(
            MqRouteLookup::Queue {
                name: name("R.Q"),
                manager: Some(name("OTHER")),
                dynamic_pattern: None,
            },
            &[A::Output],
        );
        assert!(
            matches!(remote.disposition, MqObjectRouteDisposition::Pending(ref reasons) if reasons.contains(&MqObjectRoutePending::RemoteChannel))
        );
        let model = open(
            MqRouteLookup::Queue {
                name: name("MODEL"),
                manager: None,
                dynamic_pattern: Some(MqRouteDynamicPattern::new("D.*").unwrap()),
            },
            &[A::Output],
        );
        assert!(
            matches!(model.disposition, MqObjectRouteDisposition::Pending(ref reasons) if reasons.contains(&MqObjectRoutePending::ModelLifecycle))
        );
        let topic = open(
            MqRouteLookup::Topic {
                name: name("TOPIC"),
                object_string: None,
            },
            &[A::Output],
        );
        assert!(
            matches!(topic.disposition, MqObjectRouteDisposition::Pending(ref reasons) if reasons.contains(&MqObjectRoutePending::TopicRouting))
        );
        let namelist = open(MqRouteLookup::Namelist(name("LIST")), &[A::Inquire]);
        assert_eq!(
            namelist.disposition,
            MqObjectRouteDisposition::Unsupported(MqObjectRouteUnsupported::NamelistCatalog)
        );
    }

    #[test]
    fn close_rejects_wrong_lifecycle_and_retains_pending_authorities() {
        let mut registry = crate::MqHandleRegistry::new(1, 4).unwrap();
        let owner = crate::MqHandleOwner {
            environment: crate::MqHostEnvironment::ZosBatch,
            host_id: 1,
            process_id: 1,
            thread_id: 1,
            task_id: 1,
            syncpoint_epoch: 1,
        };
        let conn = registry
            .connect(owner, crate::MqHandleSharing::NonShared)
            .unwrap();
        let handle = registry.create_object(owner, conn).unwrap();
        let target = MqRouteCloseTarget::Object {
            handle,
            lifecycle: MqRouteCloseLifecycle::Predefined,
        };
        assert_eq!(
            MqObjectCloseRequest::new(conn, target, MqRouteCloseMode::Delete),
            Err(MqObjectRouteError::InvalidCloseMode)
        );
        let close = MqObjectCloseRequest::new(conn, target, MqRouteCloseMode::None).unwrap();
        assert!(
            matches!(close.review().disposition, MqObjectRouteDisposition::Pending(ref reasons) if reasons.contains(&MqObjectRoutePending::HandleRegistry))
        );
        let dynamic = MqRouteCloseTarget::Object {
            handle,
            lifecycle: MqRouteCloseLifecycle::PermanentDynamic,
        };
        let delete =
            MqObjectCloseRequest::new(conn, dynamic, MqRouteCloseMode::DeletePurge).unwrap();
        assert!(
            matches!(delete.review().disposition, MqObjectRouteDisposition::Pending(ref reasons) if reasons.contains(&MqObjectRoutePending::ModelLifecycle))
        );
        let read_ahead =
            MqObjectCloseRequest::new(conn, target, MqRouteCloseMode::Quiesce).unwrap();
        assert_eq!(
            read_ahead.review().disposition,
            MqObjectRouteDisposition::Unsupported(MqObjectRouteUnsupported::ClientReadAhead)
        );
    }
}
