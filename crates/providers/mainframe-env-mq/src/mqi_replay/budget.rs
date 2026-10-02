//! Structural preflight only; no Value tree, product transition or status rule.
use super::*;
use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
use std::collections::BTreeSet;
use std::fmt;
use std::io::{self, Write};

pub(super) struct Sink {
    bytes: Vec<u8>,
    limit: usize,
}
impl Sink {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }
    pub(super) fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}
impl Write for Sink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self
            .bytes
            .len()
            .checked_add(bytes.len())
            .is_none_or(|n| n > self.limit)
        {
            return Err(io::Error::other("typed replay storage byte limit"));
        }
        self.bytes
            .try_reserve(bytes.len())
            .map_err(io::Error::other)?;
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct Budget {
    host: HostLimits,
    mqi: MqMqiLimits,
    nodes: usize,
    containers: usize,
    property_bytes: usize,
}
struct Walk<'a> {
    budget: &'a mut Budget,
    depth: usize,
    field: String,
}
struct Key;
impl<'de> DeserializeSeed<'de> for Key {
    type Value = String;
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<String, D::Error> {
        struct Name;
        impl Visitor<'_> for Name {
            type Value = String;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("bounded storage field name")
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<String, E> {
                if value.len() > 64 {
                    return Err(E::custom("replay key bounds"));
                }
                Ok(value.to_owned())
            }
        }
        d.deserialize_str(Name)
    }
}
impl Budget {
    fn property_bytes(&mut self, added: usize) -> Result<(), ReplayError> {
        self.property_bytes = self
            .property_bytes
            .checked_add(added)
            .ok_or(ReplayError::Bounds)?;
        if self.property_bytes
            > self
                .host
                .max_state_bytes
                .min(self.mqi.message.property_total_bytes)
        {
            return Err(ReplayError::Bounds);
        }
        Ok(())
    }
    fn sequence_limit(&self, field: &str) -> usize {
        let h = self.host;
        let m = self.mqi;
        let p = m.message;
        match field {
            "body" => h.max_record_bytes.min(p.body_bytes),
            "bytes" => h.max_record_bytes.min(m.buffer_bytes),
            "characters" => h.max_record_bytes.min(m.attribute_bytes),
            "integers" => h.max_fields.min(m.selectors),
            "items" => h.max_records.min(p.distribution_items),
            "properties" => h.max_fields.min(p.properties),
            "message_id" | "correlation_id" | "group_id" => {
                h.max_record_bytes.min(p.identifier_bytes)
            }
            "value" => h.max_record_bytes.min(p.property_value_bytes),
            "host_result_digest" => 32,
            _ => 32,
        }
    }
    fn string_limit(&self, field: &str) -> usize {
        match field {
            "name" => self
                .host
                .max_name_bytes
                .min(self.mqi.message.property_name_bytes),
            "destination" => self
                .host
                .max_name_bytes
                .min(self.mqi.message.destination_bytes),
            "format" => self.host.max_name_bytes.min(self.mqi.message.format_bytes),
            _ => 128, // fixed schema, enum tags and admitted reason symbols
        }
    }
}
impl<'de> DeserializeSeed<'de> for Walk<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        if self.depth > 24 || self.budget.nodes == 0 {
            return Err(serde::de::Error::custom("replay bounds"));
        }
        self.budget.nodes -= 1;
        d.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for Walk<'_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("bounded replay JSON")
    }
    fn visit_unit<E: serde::de::Error>(self) -> Result<(), E> {
        Ok(())
    }
    fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<(), E> {
        Ok(())
    }
    fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<(), E> {
        Err(E::custom("replay integer required"))
    }
    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<(), E> {
        if value.len() > self.budget.string_limit(&self.field) {
            return Err(E::custom("replay string bounds"));
        }
        if self.field == "name" {
            self.budget
                .property_bytes(value.len())
                .map_err(|_| E::custom("replay aggregate property bounds"))?;
        }
        Ok(())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<(), A::Error> {
        let limit = self.budget.sequence_limit(&self.field);
        let mut n = 0usize;
        loop {
            let next = a.next_element_seed(Walk {
                budget: self.budget,
                depth: self.depth + 1,
                field: String::new(),
            })?;
            if next.is_none() {
                break;
            }
            n = n
                .checked_add(1)
                .ok_or_else(|| serde::de::Error::custom("replay bounds"))?;
            if n > limit {
                return Err(serde::de::Error::custom("replay collection bounds"));
            }
            if self.field == "value" {
                self.budget
                    .property_bytes(1)
                    .map_err(|_| serde::de::Error::custom("replay aggregate property bounds"))?;
            }
        }
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<(), A::Error> {
        if self.budget.containers == 0 {
            return Err(serde::de::Error::custom("replay map bounds"));
        }
        self.budget.containers -= 1;
        let mut names = BTreeSet::new();
        while let Some(name) = a.next_key_seed(Key)? {
            if name.len() > 64 || names.len() == 16 || !names.insert(name.clone()) {
                return Err(serde::de::Error::custom("replay duplicate/key bounds"));
            }
            a.next_value_seed(Walk {
                budget: self.budget,
                depth: self.depth + 1,
                field: name,
            })?;
        }
        Ok(())
    }
}
pub(super) fn preflight(
    bytes: &[u8],
    host: HostLimits,
    mqi: MqMqiLimits,
) -> Result<(), ReplayError> {
    let containers = mqi
        .message
        .properties
        .checked_add(mqi.message.distribution_items)
        .and_then(|n| n.checked_mul(4))
        .and_then(|n| n.checked_add(32))
        .ok_or(ReplayError::Bounds)?;
    let mut budget = Budget {
        host,
        mqi,
        nodes: bytes.len(),
        containers,
        property_bytes: 0,
    };
    let mut reader = serde_json::Deserializer::from_slice(bytes);
    Walk {
        budget: &mut budget,
        depth: 0,
        field: String::new(),
    }
    .deserialize(&mut reader)
    .map_err(|_| ReplayError::Malformed)?;
    reader.end().map_err(|_| ReplayError::Malformed)
}
