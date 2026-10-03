//! Structural @2 allocation preflight; the sole typed catalog decoder owns facts.
use super::*;
use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
use std::fmt;

struct Walk<'a> {
    limits: MqObjectLimits,
    field: &'a str,
    depth: usize,
    producer: bool,
}
impl<'de> DeserializeSeed<'de> for Walk<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        if self.depth > 12 {
            return Err(serde::de::Error::custom("catalog depth"));
        }
        d.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for Walk<'_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("bounded catalog structure")
    }
    fn visit_unit<E: serde::de::Error>(self) -> Result<(), E> {
        if self.field == "producer_defaults" {
            return Err(E::custom("catalog defaults must be explicit"));
        }
        Ok(())
    }
    fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<(), E> {
        Ok(())
    }
    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<(), E> {
        if value.len() > 128 {
            return Err(E::custom("catalog string bound"));
        }
        Ok(())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<(), A::Error> {
        let limit = match self.field {
            "objects" | "queues" => self.limits.max_objects,
            "model_instances" => self.limits.max_dynamic_instances,
            _ => return Err(serde::de::Error::custom("catalog unexpected sequence")),
        };
        let mut count = 0;
        while a
            .next_element_seed(Walk {
                limits: self.limits,
                field: self.field,
                depth: self.depth + 1,
                producer: self.producer,
            })?
            .is_some()
        {
            count += 1;
            if count > limit {
                return Err(serde::de::Error::custom("catalog collection bound"));
            }
        }
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<(), A::Error> {
        let mut keys = BTreeSet::new();
        while let Some(key) = a.next_key::<String>()? {
            if key.len() > 64 || keys.len() >= 16 || !keys.insert(key.clone()) {
                return Err(serde::de::Error::custom("catalog duplicate/key bound"));
            }
            a.next_value_seed(Walk {
                limits: self.limits,
                field: &key,
                depth: self.depth + 1,
                producer: self.producer,
            })?;
        }
        // Required nullable fields are a structural @2 rule. @1's historical
        // parser/defaults are untouched; no product transition is derived here.
        if !self.producer && self.field == "queues" && keys.contains("producer_defaults") {
            return Err(serde::de::Error::custom("catalog@2 defaults field"));
        }
        let required: &[&str] = match self.field {
            "queue_manager" => &["default_transmission_queue"],
            "model_instances" => &["trigger_process"],
            "objects" if keys.contains("usage") || keys.contains("definition_type") => {
                &["trigger_process"]
            }
            "objects" if keys.contains("remote_queue_manager") => {
                &["remote_queue", "transmission_queue"]
            }
            _ => &[],
        };
        if required.iter().any(|key| !keys.contains(*key)) {
            return Err(serde::de::Error::custom("catalog required nullable field"));
        }
        Ok(())
    }
}
pub(super) fn check(
    bytes: &[u8],
    limits: MqObjectLimits,
    producer: bool,
) -> Result<(), MqObjectError> {
    let mut d = serde_json::Deserializer::from_slice(bytes);
    Walk {
        limits,
        field: "",
        depth: 0,
        producer,
    }
    .deserialize(&mut d)
    .map_err(|_| MqObjectError::CorruptSnapshot)?;
    d.end().map_err(|_| MqObjectError::CorruptSnapshot)
}
