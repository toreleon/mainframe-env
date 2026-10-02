//! Allocation-free values preflight before typed snapshot/row allocation.
use super::*;
use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
use std::fmt;

pub(super) fn check(
    bytes: &[u8],
    limits: MqDeliveryLimits,
    message: MqMessageLimits,
) -> Result<(), MqDeliveryError> {
    if bytes.len() > limits.snapshot_bytes {
        return Err(MqDeliveryError::ResourceExhausted);
    }
    let mut d = serde_json::Deserializer::from_slice(bytes);
    Seed {
        limits,
        message,
        depth: 0,
        count: limits.snapshot_bytes,
    }
    .deserialize(&mut d)
    .map_err(|_| MqDeliveryError::CorruptSnapshot)?;
    d.end().map_err(|_| MqDeliveryError::CorruptSnapshot)
}
#[derive(Clone, Copy)]
struct Seed {
    limits: MqDeliveryLimits,
    message: MqMessageLimits,
    depth: usize,
    count: usize,
}
impl Seed {
    fn child(self, key: &str) -> Self {
        let count = match key {
            "queues" => self.limits.queues,
            "messages" => self.limits.depth_per_queue,
            "pending" | "operations" => self.limits.pending_operations,
            "finalized" => self.limits.finalized_units,
            "cursors" => self.limits.cursors,
            "body" => self.message.body_bytes,
            "properties" => self.message.properties,
            "md" => 2048,
            "value" => self.message.property_value_bytes.max(64),
            _ => self.limits.snapshot_bytes,
        };
        Self {
            depth: self.depth + 1,
            count,
            ..self
        }
    }
}
impl<'de> DeserializeSeed<'de> for Seed {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        if self.depth > 24 {
            return Err(serde::de::Error::custom("depth"));
        }
        d.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for Seed {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("bounded strict JSON")
    }
    fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<(), E> {
        Ok(())
    }
    fn visit_unit<E: serde::de::Error>(self) -> Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<(), E> {
        Ok(())
    }
    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<(), E> {
        if v.len()
            > self
                .message
                .property_name_bytes
                .max(self.message.format_bytes)
                .max(128)
        {
            return Err(E::custom("string bound"));
        }
        Ok(())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<(), A::Error> {
        let mut count = 0usize;
        while a
            .next_element_seed(Self {
                depth: self.depth + 1,
                ..self
            })?
            .is_some()
        {
            count += 1;
            if count > self.count {
                return Err(serde::de::Error::custom("count"));
            }
        }
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<(), A::Error> {
        let mut keys = BTreeSet::new();
        while let Some(key) = a.next_key_seed(KeySeed)? {
            if key.len() > 64 || keys.len() >= 64 || !keys.insert(key.clone()) {
                return Err(serde::de::Error::custom("duplicate/key bound"));
            }
            a.next_value_seed(self.child(&key))?;
        }
        Ok(())
    }
}

struct KeySeed;
impl<'de> DeserializeSeed<'de> for KeySeed {
    type Value = String;
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<String, D::Error> {
        struct KeyVisitor;
        impl Visitor<'_> for KeyVisitor {
            type Value = String;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("bounded key")
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<String, E> {
                if v.len() > 64 {
                    return Err(E::custom("key bound"));
                }
                Ok(v.to_owned())
            }
        }
        d.deserialize_str(KeyVisitor)
    }
}
