//! Strict storage@3 observations in the sole MQI codec. No live handles/state.
use super::*;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Descriptor {
    struc_id: [u8; 4],
    version: i32,
    options: i32,
    support: i32,
    context: i32,
    copy_options: i32,
}
impl Descriptor {
    fn capture(v: &MqPropertyDescriptor) -> Self {
        Self {
            struc_id: v.struc_id,
            version: v.version,
            options: v.options,
            support: v.support,
            context: v.context,
            copy_options: v.copy_options,
        }
    }
    fn restore(self) -> MqPropertyDescriptor {
        MqPropertyDescriptor {
            struc_id: self.struc_id,
            version: self.version,
            options: self.options,
            support: self.support,
            context: self.context,
            copy_options: self.copy_options,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub(super) enum Observation {
    Set {
        descriptor: Descriptor,
    },
    Inquired {
        descriptor: Descriptor,
        property_type: String,
        returned_encoding: i32,
        #[serde(deserialize_with = "super::shape::required_option")]
        returned_ccsid: Option<i32>,
        returned_name: Vec<u8>,
        name_length: i32,
        name_ccsid: i32,
        data_length: i32,
        copied_value: Vec<u8>,
    },
    PropertyDeleted {},
    HandleDeleted {},
    Absent {},
}
impl Observation {
    pub(super) fn capture(v: &MqPropertyObservation) -> Self {
        match v {
            MqPropertyObservation::Set(pd) => Self::Set {
                descriptor: Descriptor::capture(pd),
            },
            MqPropertyObservation::Inquired(v) => Self::Inquired {
                descriptor: Descriptor::capture(&v.descriptor),
                property_type: kind(v.kind).into(),
                returned_encoding: v.returned_encoding,
                returned_ccsid: v.returned_ccsid,
                returned_name: v.returned_name.clone(),
                name_length: v.name_length,
                name_ccsid: v.name_ccsid,
                data_length: v.data_length,
                copied_value: v.copied_value.clone(),
            },
            MqPropertyObservation::PropertyDeleted => Self::PropertyDeleted {},
            MqPropertyObservation::HandleDeleted => Self::HandleDeleted {},
            MqPropertyObservation::Absent => Self::Absent {},
        }
    }
    pub(super) fn restore(self) -> Result<MqPropertyObservation, ReplayError> {
        Ok(match self {
            Self::Set { descriptor } => MqPropertyObservation::Set(descriptor.restore()),
            Self::Inquired {
                descriptor,
                property_type,
                returned_encoding,
                returned_ccsid,
                returned_name,
                name_length,
                name_ccsid,
                data_length,
                copied_value,
            } => MqPropertyObservation::Inquired(MqPropertyInquiryObservation {
                descriptor: descriptor.restore(),
                kind: parse_kind(&property_type)?,
                returned_encoding,
                returned_ccsid,
                returned_name,
                name_length,
                name_ccsid,
                data_length,
                copied_value,
            }),
            Self::PropertyDeleted {} => MqPropertyObservation::PropertyDeleted,
            Self::HandleDeleted {} => MqPropertyObservation::HandleDeleted,
            Self::Absent {} => MqPropertyObservation::Absent,
        })
    }
}
fn kind(value: MqPropertyType) -> &'static str {
    match value {
        MqPropertyType::Null => "Null",
        MqPropertyType::Boolean => "Boolean",
        MqPropertyType::ByteString => "ByteString",
        MqPropertyType::Int8 => "Int8",
        MqPropertyType::Int16 => "Int16",
        MqPropertyType::Int32 => "Int32",
        MqPropertyType::Int64 => "Int64",
        MqPropertyType::Float32 => "Float32",
        MqPropertyType::Float64 => "Float64",
        MqPropertyType::String => "String",
    }
}
fn parse_kind(value: &str) -> Result<MqPropertyType, ReplayError> {
    match value {
        "Null" => Ok(MqPropertyType::Null),
        "ByteString" => Ok(MqPropertyType::ByteString),
        "Int8" => Ok(MqPropertyType::Int8),
        "Int16" => Ok(MqPropertyType::Int16),
        "Int32" => Ok(MqPropertyType::Int32),
        "Int64" => Ok(MqPropertyType::Int64),
        "String" => Ok(MqPropertyType::String),
        _ => Err(ReplayError::Malformed),
    }
}
