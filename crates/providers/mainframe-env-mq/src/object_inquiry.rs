//! Read-only, catalog-backed named MQINQ fields. Public MQI routing remains pending.
//!
//! The pinned MQINQ tables identify fields but do not pin numeric selector values,
//! queue-subtype applicability from linked attribute pages, or numeric QType values.
//! This API returns typed catalog values, not MQLONGs or a raw CharAttrs buffer.
//! Character capacities count typed fields and their canonical name payload bytes;
//! they are private resource budgets, not IBM fixed-width CharAttrLength or CCSID.
//! Raw character layout, numeric selectors/status mapping, and MQSET remain pending.

use crate::{
    MqObjectCapability, MqObjectCatalog, MqObjectError, MqObjectKind, MqObjectLookup, MqObjectName,
    MqResolvedTarget,
};
use mainframe_env_host_api::mq_object_route::{MqObjectRouteSource, MqRouteOpenAccess};
use mainframe_env_host_api::{
    MqHandle, MqHandleKind, MqHandleOwner, MqHandleProblem, MqHandleRegistry, MqHconn,
};

pub const MQ_INQUIRY_SOURCE: MqObjectRouteSource = MqObjectRouteSource {
    row: "ibm-mq-9.4-mqi-2026-08-31:mqi-calls-unique:0016",
    topic: "SSFKSJ_9.4.0/refdev/q101840_.html",
    sha256: "03e3347bbf16d2f8e3a9061e921dbfca7a3afd0fe3bc13418ebdf47bb652ce1b",
};

/// MQINQ SelectorCount permits zero through 256, including repeated occurrences.
pub const MQ_INQUIRY_MAX_SELECTORS: usize = 256;
/// Private name-payload ceiling, derived from the existing catalog name bound.
pub const MQ_INQUIRY_MAX_CHARACTER_BYTES: usize =
    MQ_INQUIRY_MAX_SELECTORS * crate::MQ_OBJECT_NAME_BYTES;

/// Named attributes from MQINQ tables 1, 3, and 4, never IBM selector numbers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqInquiryAttribute {
    QueueName,
    QueueType,
    ProcessName,
    QueueManagerName,
    DefaultTransmissionQueueName,
    /// The linked attribute's queue-subtype rules are not pinned here.
    BaseQueueNamePending,
    /// Catalog data alone does not establish the linked usage/value rules.
    QueueUsagePending,
    /// Depth belongs to delivery state, not the object catalog.
    CurrentQueueDepthPending,
}

impl MqInquiryAttribute {
    pub fn from_symbol(symbol: &str) -> Result<Self, MqInquiryError> {
        Ok(match symbol {
            "MQCA_Q_NAME" => Self::QueueName,
            "MQIA_Q_TYPE" => Self::QueueType,
            "MQCA_PROCESS_NAME" => Self::ProcessName,
            "MQCA_Q_MGR_NAME" => Self::QueueManagerName,
            "MQCA_DEF_XMIT_Q_NAME" => Self::DefaultTransmissionQueueName,
            "MQCA_BASE_Q_NAME" => Self::BaseQueueNamePending,
            "MQIA_USAGE" => Self::QueueUsagePending,
            "MQIA_CURRENT_Q_DEPTH" => Self::CurrentQueueDepthPending,
            _ => return Err(MqInquiryError::UnknownSelector),
        })
    }

    #[must_use]
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::QueueName => "MQCA_Q_NAME",
            Self::QueueType => "MQIA_Q_TYPE",
            Self::ProcessName => "MQCA_PROCESS_NAME",
            Self::QueueManagerName => "MQCA_Q_MGR_NAME",
            Self::DefaultTransmissionQueueName => "MQCA_DEF_XMIT_Q_NAME",
            Self::BaseQueueNamePending => "MQCA_BASE_Q_NAME",
            Self::QueueUsagePending => "MQIA_USAGE",
            Self::CurrentQueueDepthPending => "MQIA_CURRENT_Q_DEPTH",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqInquirySelector {
    Named(MqInquiryAttribute),
    /// No numeric selector mapping is inferred, including apparently familiar values.
    NumericPending(i32),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MqInquiryCapacity {
    pub integer_attributes: usize,
    pub character_attributes: usize,
    /// Sum of canonical name bytes, excluding IBM padding/encoding (pending).
    pub character_bytes: usize,
}

impl MqInquiryCapacity {
    fn validate(self) -> Result<(), MqInquiryError> {
        if self.integer_attributes > MQ_INQUIRY_MAX_SELECTORS
            || self.character_attributes > MQ_INQUIRY_MAX_SELECTORS
            || self.character_bytes > MQ_INQUIRY_MAX_CHARACTER_BYTES
        {
            return Err(MqInquiryError::CapacityLimit);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqInquiryRequest {
    selectors: Vec<MqInquirySelector>,
    capacity: MqInquiryCapacity,
}

impl MqInquiryRequest {
    pub fn new(
        selectors: &[MqInquirySelector],
        capacity: MqInquiryCapacity,
    ) -> Result<Self, MqInquiryError> {
        if selectors.len() > MQ_INQUIRY_MAX_SELECTORS {
            return Err(MqInquiryError::SelectorLimit);
        }
        capacity.validate()?;
        Ok(Self {
            selectors: selectors.to_vec(),
            capacity,
        })
    }
}

/// A trusted provider's existing MQOPEN binding, not an application assertion.
/// The service owner must supply the actual Hobj lookup and granted open access
/// under its catalog/handle fence, after SAF authorization. This kernel neither
/// owns another binding registry nor substitutes handle validity for SAF.
#[derive(Clone, Copy, Debug)]
pub struct MqInquiryBinding<'a> {
    pub owner: MqHandleOwner,
    pub connection: MqHconn,
    pub handle: MqHandle,
    pub lookup: &'a MqObjectLookup,
    pub open_access: &'a [MqRouteOpenAccess],
}

/// Symbolic value occupying one integer-attribute slot. No wire integer mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqInquiryInteger {
    QueueType(MqObjectKind),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqInquiryCharacter {
    pub attribute: MqInquiryAttribute,
    /// None preserves an unset optional catalog field; no wire blank is invented.
    pub name: Option<MqObjectName>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MqInquiryResult {
    /// Relative integer-selector order, retaining every repeated occurrence.
    pub integers: Vec<MqInquiryInteger>,
    /// Relative character-selector order, retaining every repeated occurrence.
    pub characters: Vec<MqInquiryCharacter>,
    pub used: MqInquiryCapacity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqInquiryPending {
    NumericSelector,
    CharacterLayout,
    AttributeValue(MqInquiryAttribute),
    AttributeApplicability(MqInquiryAttribute),
    ModelRequiresDynamicQueue,
    ObjectKind(MqObjectKind),
    MqSet,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqInquiryError {
    Handle(MqHandleProblem),
    Object(MqObjectError),
    NotOpenForInquire,
    UnknownSelector,
    InvalidSelectorForObject(MqInquiryAttribute),
    SelectorLimit,
    CapacityLimit,
    OutputTooSmall { required: MqInquiryCapacity },
    Pending(MqInquiryPending),
}

/// Explicit dispositions for adapter forms that this bounded slice cannot execute.
pub const fn pending_character_layout() -> MqInquiryError {
    MqInquiryError::Pending(MqInquiryPending::CharacterLayout)
}

pub const fn pending_mqset() -> MqInquiryError {
    MqInquiryError::Pending(MqInquiryPending::MqSet)
}

/// Take one immutable catalog snapshot. Validate all selectors before returning
/// any output; short capacities are private errors, not simulated IBM warnings.
/// A wire adapter must implement partial-output/warning rules separately.
pub fn inquire_object(
    catalog: &MqObjectCatalog,
    registry: &MqHandleRegistry,
    binding: MqInquiryBinding<'_>,
    request: &MqInquiryRequest,
) -> Result<MqInquiryResult, MqInquiryError> {
    registry
        .validate(
            binding.owner,
            binding.connection,
            binding.handle,
            MqHandleKind::Object,
        )
        .map_err(MqInquiryError::Handle)?;
    if !binding.open_access.contains(&MqRouteOpenAccess::Inquire) {
        return Err(MqInquiryError::NotOpenForInquire);
    }
    let kind = catalog
        .kind(binding.lookup)
        .map_err(MqInquiryError::Object)?;
    match kind {
        MqObjectKind::ModelQueue => {
            return Err(MqInquiryError::Pending(
                MqInquiryPending::ModelRequiresDynamicQueue,
            ));
        }
        MqObjectKind::Topic | MqObjectKind::Subscription => {
            return Err(MqInquiryError::Pending(MqInquiryPending::ObjectKind(kind)));
        }
        _ => {}
    }
    let resolution = catalog
        .resolve(binding.lookup, MqObjectCapability::Inquire)
        .map_err(MqInquiryError::Object)?;
    let MqResolvedTarget::Definition { identity } = resolution.target else {
        return Err(MqInquiryError::Object(MqObjectError::UnsupportedCapability));
    };
    let is_queue = matches!(
        kind,
        MqObjectKind::LocalQueue | MqObjectKind::AliasQueue | MqObjectKind::RemoteQueue
    );
    let mut result = MqInquiryResult::default();
    for selector in &request.selectors {
        let attribute = match selector {
            MqInquirySelector::Named(attribute) => *attribute,
            MqInquirySelector::NumericPending(_) => {
                return Err(MqInquiryError::Pending(MqInquiryPending::NumericSelector));
            }
        };
        let name = match attribute {
            MqInquiryAttribute::QueueType if is_queue => {
                result.integers.push(MqInquiryInteger::QueueType(kind));
                result.used.integer_attributes += 1;
                continue;
            }
            MqInquiryAttribute::QueueName if is_queue => Some(identity.name.clone()),
            MqInquiryAttribute::ProcessName if kind == MqObjectKind::Process => {
                Some(identity.name.clone())
            }
            MqInquiryAttribute::QueueManagerName if kind == MqObjectKind::QueueManager => {
                Some(identity.name.clone())
            }
            MqInquiryAttribute::DefaultTransmissionQueueName
                if kind == MqObjectKind::QueueManager =>
            {
                catalog.queue_manager().default_transmission_queue.clone()
            }
            MqInquiryAttribute::ProcessName | MqInquiryAttribute::BaseQueueNamePending
                if is_queue =>
            {
                return Err(MqInquiryError::Pending(
                    MqInquiryPending::AttributeApplicability(attribute),
                ));
            }
            MqInquiryAttribute::QueueUsagePending
            | MqInquiryAttribute::CurrentQueueDepthPending
                if is_queue =>
            {
                return Err(MqInquiryError::Pending(MqInquiryPending::AttributeValue(
                    attribute,
                )));
            }
            _ => return Err(MqInquiryError::InvalidSelectorForObject(attribute)),
        };
        result.used.character_attributes += 1;
        result.used.character_bytes += name.as_ref().map_or(0, |name| name.as_str().len());
        result
            .characters
            .push(MqInquiryCharacter { attribute, name });
    }
    if result.used.integer_attributes > request.capacity.integer_attributes
        || result.used.character_attributes > request.capacity.character_attributes
        || result.used.character_bytes > request.capacity.character_bytes
    {
        return Err(MqInquiryError::OutputTooSmall {
            required: result.used,
        });
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
