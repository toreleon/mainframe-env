use super::Db2TableDefinition;
use crate::Db2Limits;
use mainframe_env_host_api::HostProblem;
use serde::de::{self, DeserializeSeed, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use std::collections::BTreeSet;
use std::fmt;

pub fn decode_table_definitions_bounded(
    bytes: &[u8],
    limits: Db2Limits,
) -> Result<Vec<Db2TableDefinition>, HostProblem> {
    if bytes.len() > limits.max_catalog_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut budget = Budget::new(limits);
    let mut preflight = serde_json::Deserializer::from_slice(bytes);
    CatalogSeed {
        limits,
        budget: &mut budget,
    }
    .deserialize(&mut preflight)
    .map_err(|_| HostProblem::Malformed)?;
    preflight.end().map_err(|_| HostProblem::Malformed)?;
    serde_json::from_slice(bytes).map_err(|_| HostProblem::Malformed)
}

struct Budget {
    text: usize,
    items: usize,
    limits: Db2Limits,
}

impl Budget {
    fn new(limits: Db2Limits) -> Self {
        Self {
            text: 0,
            items: 0,
            limits,
        }
    }

    fn item<E: de::Error>(&mut self) -> Result<(), E> {
        self.items = self
            .items
            .checked_add(1)
            .filter(|items| *items <= self.limits.max_catalog_nested_items)
            .ok_or_else(|| E::custom("catalog nested-item limit exceeded"))?;
        Ok(())
    }

    fn text<E: de::Error>(&mut self, value: &str) -> Result<(), E> {
        if value.len() > 256 || value.chars().any(char::is_control) {
            return Err(E::custom("catalog text field is invalid"));
        }
        self.text = self
            .text
            .checked_add(value.len())
            .filter(|text| *text <= self.limits.max_catalog_text_bytes)
            .ok_or_else(|| E::custom("catalog text limit exceeded"))?;
        Ok(())
    }
}

struct CatalogSeed<'a> {
    limits: Db2Limits,
    budget: &'a mut Budget,
}

impl<'de> DeserializeSeed<'de> for CatalogSeed<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_seq(CatalogVisitor {
            limits: self.limits,
            budget: self.budget,
        })
    }
}

struct CatalogVisitor<'a> {
    limits: Db2Limits,
    budget: &'a mut Budget,
}

impl<'de> Visitor<'de> for CatalogVisitor<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a bounded Db2 table-definition array")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<(), A::Error> {
        if sequence
            .size_hint()
            .is_some_and(|count| count > self.limits.max_tables)
        {
            return Err(de::Error::custom("catalog table limit exceeded"));
        }
        let mut count = 0usize;
        let mut names = BTreeSet::new();
        while let Some(name) = sequence.next_element_seed(TableSeed {
            limits: self.limits,
            budget: self.budget,
        })? {
            count += 1;
            if count > self.limits.max_tables || !names.insert(name.to_ascii_uppercase()) {
                return Err(de::Error::custom("catalog table count or name is invalid"));
            }
        }
        if count == 0 {
            return Err(de::Error::custom("catalog contains no tables"));
        }
        Ok(())
    }
}

struct TableSeed<'a> {
    limits: Db2Limits,
    budget: &'a mut Budget,
}

impl<'de> DeserializeSeed<'de> for TableSeed<'_> {
    type Value = String;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<String, D::Error> {
        self.budget.item()?;
        deserializer.deserialize_map(TableVisitor {
            limits: self.limits,
            budget: self.budget,
        })
    }
}

struct TableVisitor<'a> {
    limits: Db2Limits,
    budget: &'a mut Budget,
}

impl<'de> Visitor<'de> for TableVisitor<'_> {
    type Value = String;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a bounded Db2 table definition")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<String, A::Error> {
        let mut fields = 0_u8;
        let mut name = None;
        while let Some(field) = map.next_key::<&str>()? {
            match field {
                "name" => {
                    mark_field(&mut fields, 0, "name")?;
                    name = Some(next_bounded_text(&mut map, self.budget)?);
                }
                "columns" => {
                    mark_field(&mut fields, 1, "columns")?;
                    map.next_value_seed(ColumnSequenceSeed {
                        limits: self.limits,
                        budget: self.budget,
                    })?;
                }
                "primary_key" => {
                    mark_field(&mut fields, 2, "primary_key")?;
                    map.next_value_seed(StringSequenceSeed {
                        maximum: self.limits.max_primary_key_columns,
                        budget: self.budget,
                    })?;
                }
                "foreign_keys" => {
                    mark_field(&mut fields, 3, "foreign_keys")?;
                    map.next_value_seed(ForeignKeySequenceSeed {
                        limits: self.limits,
                        budget: self.budget,
                    })?;
                }
                "extract" => {
                    mark_field(&mut fields, 4, "extract")?;
                    map.next_value_seed(ExtractOptionSeed {
                        limits: self.limits,
                        budget: self.budget,
                    })?;
                }
                _ => return Err(de::Error::unknown_field(field, TABLE_FIELDS)),
            }
        }
        name.ok_or_else(|| de::Error::missing_field("name"))
    }
}

const TABLE_FIELDS: &[&str] = &["name", "columns", "primary_key", "foreign_keys", "extract"];

struct ColumnSequenceSeed<'a> {
    limits: Db2Limits,
    budget: &'a mut Budget,
}

impl<'de> DeserializeSeed<'de> for ColumnSequenceSeed<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_seq(ColumnSequenceVisitor {
            limits: self.limits,
            budget: self.budget,
        })
    }
}

struct ColumnSequenceVisitor<'a> {
    limits: Db2Limits,
    budget: &'a mut Budget,
}

impl<'de> Visitor<'de> for ColumnSequenceVisitor<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("bounded Db2 columns")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<(), A::Error> {
        if sequence
            .size_hint()
            .is_some_and(|count| count > self.limits.max_columns)
        {
            return Err(de::Error::custom("catalog column limit exceeded"));
        }
        let mut count = 0usize;
        while sequence
            .next_element_seed(ColumnSeed {
                limits: self.limits,
                budget: self.budget,
            })?
            .is_some()
        {
            count += 1;
            if count > self.limits.max_columns {
                return Err(de::Error::custom("catalog column limit exceeded"));
            }
        }
        Ok(())
    }
}

struct ColumnSeed<'a> {
    limits: Db2Limits,
    budget: &'a mut Budget,
}

impl<'de> DeserializeSeed<'de> for ColumnSeed<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        self.budget.item()?;
        deserializer.deserialize_map(ColumnVisitor {
            limits: self.limits,
            budget: self.budget,
        })
    }
}

struct ColumnVisitor<'a> {
    limits: Db2Limits,
    budget: &'a mut Budget,
}

impl<'de> Visitor<'de> for ColumnVisitor<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a bounded Db2 column")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let mut fields = 0_u8;
        while let Some(field) = map.next_key::<&str>()? {
            match field {
                "name" => {
                    mark_field(&mut fields, 0, "name")?;
                    let _ = next_bounded_text(&mut map, self.budget)?;
                }
                "default_value" => {
                    mark_field(&mut fields, 1, "default_value")?;
                    map.next_value_seed(OptionalByteSequenceSeed {
                        maximum: self.limits.max_column_bytes,
                        budget: self.budget,
                    })?;
                }
                "nullable" => {
                    mark_field(&mut fields, 2, "nullable")?;
                    map.next_value::<IgnoredAny>()?;
                }
                "max_bytes" => {
                    mark_field(&mut fields, 3, "max_bytes")?;
                    map.next_value::<IgnoredAny>()?;
                }
                "result_encoding" => {
                    mark_field(&mut fields, 4, "result_encoding")?;
                    map.next_value::<IgnoredAny>()?;
                }
                _ => return Err(de::Error::custom("unknown column field")),
            }
        }
        Ok(())
    }
}

struct ForeignKeySequenceSeed<'a> {
    limits: Db2Limits,
    budget: &'a mut Budget,
}

impl<'de> DeserializeSeed<'de> for ForeignKeySequenceSeed<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_seq(ForeignKeySequenceVisitor {
            limits: self.limits,
            budget: self.budget,
        })
    }
}

struct ForeignKeySequenceVisitor<'a> {
    limits: Db2Limits,
    budget: &'a mut Budget,
}

impl<'de> Visitor<'de> for ForeignKeySequenceVisitor<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("bounded Db2 foreign keys")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<(), A::Error> {
        if sequence
            .size_hint()
            .is_some_and(|count| count > self.limits.max_foreign_keys_per_table)
        {
            return Err(de::Error::custom("catalog foreign-key limit exceeded"));
        }
        let mut count = 0usize;
        while sequence
            .next_element_seed(ForeignKeySeed {
                limits: self.limits,
                budget: self.budget,
            })?
            .is_some()
        {
            count += 1;
            if count > self.limits.max_foreign_keys_per_table {
                return Err(de::Error::custom("catalog foreign-key limit exceeded"));
            }
        }
        Ok(())
    }
}

struct ForeignKeySeed<'a> {
    limits: Db2Limits,
    budget: &'a mut Budget,
}

impl<'de> DeserializeSeed<'de> for ForeignKeySeed<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        self.budget.item()?;
        deserializer.deserialize_map(ForeignKeyVisitor {
            limits: self.limits,
            budget: self.budget,
        })
    }
}

struct ForeignKeyVisitor<'a> {
    limits: Db2Limits,
    budget: &'a mut Budget,
}

impl<'de> Visitor<'de> for ForeignKeyVisitor<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a bounded Db2 foreign key")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let mut fields = 0_u8;
        while let Some(field) = map.next_key::<&str>()? {
            match field {
                "columns" => {
                    mark_field(&mut fields, 0, "columns")?;
                    map.next_value_seed(StringSequenceSeed {
                        maximum: self.limits.max_foreign_key_columns,
                        budget: self.budget,
                    })?;
                }
                "referenced_columns" => {
                    mark_field(&mut fields, 1, "referenced_columns")?;
                    map.next_value_seed(StringSequenceSeed {
                        maximum: self.limits.max_foreign_key_columns,
                        budget: self.budget,
                    })?;
                }
                "referenced_table" => {
                    mark_field(&mut fields, 2, "referenced_table")?;
                    let _ = next_bounded_text(&mut map, self.budget)?;
                }
                "delete_restrict" => {
                    mark_field(&mut fields, 3, "delete_restrict")?;
                    map.next_value::<IgnoredAny>()?;
                }
                _ => return Err(de::Error::custom("unknown foreign-key field")),
            }
        }
        Ok(())
    }
}

struct ExtractOptionSeed<'a> {
    limits: Db2Limits,
    budget: &'a mut Budget,
}

impl<'de> DeserializeSeed<'de> for ExtractOptionSeed<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_option(ExtractOptionVisitor {
            limits: self.limits,
            budget: self.budget,
        })
    }
}

struct ExtractOptionVisitor<'a> {
    limits: Db2Limits,
    budget: &'a mut Budget,
}

impl<'de> Visitor<'de> for ExtractOptionVisitor<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a null or bounded extract layout")
    }

    fn visit_none<E: de::Error>(self) -> Result<(), E> {
        Ok(())
    }

    fn visit_unit<E: de::Error>(self) -> Result<(), E> {
        Ok(())
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        self.budget.item()?;
        deserializer.deserialize_map(ExtractVisitor {
            limits: self.limits,
            budget: self.budget,
        })
    }
}

struct ExtractVisitor<'a> {
    limits: Db2Limits,
    budget: &'a mut Budget,
}

impl<'de> Visitor<'de> for ExtractVisitor<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a bounded extract layout")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let mut fields = 0_u8;
        while let Some(field) = map.next_key::<&str>()? {
            match field {
                "fields" => {
                    mark_field(&mut fields, 0, "fields")?;
                    map.next_value_seed(ExtractFieldSequenceSeed {
                        limits: self.limits,
                        budget: self.budget,
                    })?;
                }
                "trailer" => {
                    mark_field(&mut fields, 1, "trailer")?;
                    map.next_value_seed(ByteSequenceSeed {
                        maximum: self.limits.max_column_bytes,
                        budget: self.budget,
                    })?;
                }
                _ => return Err(de::Error::custom("unknown extract field")),
            }
        }
        Ok(())
    }
}

struct ExtractFieldSequenceSeed<'a> {
    limits: Db2Limits,
    budget: &'a mut Budget,
}

impl<'de> DeserializeSeed<'de> for ExtractFieldSequenceSeed<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_seq(ExtractFieldSequenceVisitor {
            limits: self.limits,
            budget: self.budget,
        })
    }
}

struct ExtractFieldSequenceVisitor<'a> {
    limits: Db2Limits,
    budget: &'a mut Budget,
}

impl<'de> Visitor<'de> for ExtractFieldSequenceVisitor<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("bounded extract fields")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<(), A::Error> {
        if sequence
            .size_hint()
            .is_some_and(|count| count > self.limits.max_extract_fields)
        {
            return Err(de::Error::custom("extract-field limit exceeded"));
        }
        let mut count = 0usize;
        while sequence
            .next_element_seed(ExtractFieldSeed {
                budget: self.budget,
            })?
            .is_some()
        {
            count += 1;
            if count > self.limits.max_extract_fields {
                return Err(de::Error::custom("extract-field limit exceeded"));
            }
        }
        Ok(())
    }
}

struct ExtractFieldSeed<'a> {
    budget: &'a mut Budget,
}

impl<'de> DeserializeSeed<'de> for ExtractFieldSeed<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        self.budget.item()?;
        deserializer.deserialize_map(ExtractFieldVisitor {
            budget: self.budget,
        })
    }
}

struct ExtractFieldVisitor<'a> {
    budget: &'a mut Budget,
}

impl<'de> Visitor<'de> for ExtractFieldVisitor<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a bounded extract field")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let mut fields = 0_u8;
        while let Some(field) = map.next_key::<&str>()? {
            match field {
                "column" => {
                    mark_field(&mut fields, 0, "column")?;
                    let _ = next_bounded_text(&mut map, self.budget)?;
                }
                "width" => {
                    mark_field(&mut fields, 1, "width")?;
                    map.next_value::<IgnoredAny>()?;
                }
                _ => return Err(de::Error::custom("unknown extract-field member")),
            }
        }
        Ok(())
    }
}

struct StringSequenceSeed<'a> {
    maximum: usize,
    budget: &'a mut Budget,
}

impl<'de> DeserializeSeed<'de> for StringSequenceSeed<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_seq(StringSequenceVisitor {
            maximum: self.maximum,
            budget: self.budget,
        })
    }
}

struct StringSequenceVisitor<'a> {
    maximum: usize,
    budget: &'a mut Budget,
}

impl<'de> Visitor<'de> for StringSequenceVisitor<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a bounded string array")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<(), A::Error> {
        if sequence
            .size_hint()
            .is_some_and(|count| count > self.maximum)
        {
            return Err(de::Error::custom("string-array limit exceeded"));
        }
        let mut count = 0usize;
        while let Some(value) = sequence.next_element::<&str>()? {
            count += 1;
            if count > self.maximum {
                return Err(de::Error::custom("string-array limit exceeded"));
            }
            self.budget.item()?;
            self.budget.text(value)?;
        }
        Ok(())
    }
}

struct OptionalByteSequenceSeed<'a> {
    maximum: usize,
    budget: &'a mut Budget,
}

impl<'de> DeserializeSeed<'de> for OptionalByteSequenceSeed<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_option(OptionalByteSequenceVisitor {
            maximum: self.maximum,
            budget: self.budget,
        })
    }
}

struct OptionalByteSequenceVisitor<'a> {
    maximum: usize,
    budget: &'a mut Budget,
}

impl<'de> Visitor<'de> for OptionalByteSequenceVisitor<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("null or a bounded byte array")
    }

    fn visit_none<E: de::Error>(self) -> Result<(), E> {
        Ok(())
    }

    fn visit_unit<E: de::Error>(self) -> Result<(), E> {
        Ok(())
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        ByteSequenceSeed {
            maximum: self.maximum,
            budget: self.budget,
        }
        .deserialize(deserializer)
    }
}

struct ByteSequenceSeed<'a> {
    maximum: usize,
    budget: &'a mut Budget,
}

impl<'de> DeserializeSeed<'de> for ByteSequenceSeed<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_seq(ByteSequenceVisitor {
            maximum: self.maximum,
            budget: self.budget,
        })
    }
}

struct ByteSequenceVisitor<'a> {
    maximum: usize,
    budget: &'a mut Budget,
}

impl<'de> Visitor<'de> for ByteSequenceVisitor<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a bounded byte array")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<(), A::Error> {
        if sequence
            .size_hint()
            .is_some_and(|count| count > self.maximum)
        {
            return Err(de::Error::custom("byte-array limit exceeded"));
        }
        let mut count = 0usize;
        while sequence.next_element::<u8>()?.is_some() {
            count += 1;
            if count > self.maximum {
                return Err(de::Error::custom("byte-array limit exceeded"));
            }
            self.budget.item()?;
        }
        Ok(())
    }
}

fn next_bounded_text<'de, A: MapAccess<'de>>(
    map: &mut A,
    budget: &mut Budget,
) -> Result<String, A::Error> {
    let value = map.next_value::<&str>()?;
    budget.item()?;
    budget.text(value)?;
    Ok(value.to_string())
}

fn mark_field<E: de::Error>(fields: &mut u8, bit: u8, name: &'static str) -> Result<(), E> {
    let mask = 1_u8 << bit;
    if *fields & mask != 0 {
        return Err(E::duplicate_field(name));
    }
    *fields |= mask;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Db2ColumnDefinition, Db2ExtractField, Db2ExtractLayout, Db2ForeignKeyDefinition,
        Db2ResultEncoding,
    };

    fn table(name: &str) -> Db2TableDefinition {
        Db2TableDefinition {
            name: name.into(),
            columns: vec![Db2ColumnDefinition {
                name: "ID".into(),
                nullable: false,
                max_bytes: 8,
                result_encoding: Db2ResultEncoding::Raw,
                default_value: None,
            }],
            primary_key: vec!["ID".into()],
            foreign_keys: vec![Db2ForeignKeyDefinition {
                columns: vec!["ID".into()],
                referenced_table: name.into(),
                referenced_columns: vec!["ID".into()],
                delete_restrict: true,
            }],
            extract: Some(Db2ExtractLayout {
                fields: vec![Db2ExtractField {
                    column: "ID".into(),
                    width: 8,
                }],
                trailer: b"END".to_vec(),
            }),
        }
    }

    fn rejected(tables: &[Db2TableDefinition], limits: Db2Limits) {
        let bytes = serde_json::to_vec(tables).unwrap();
        assert!(decode_table_definitions_bounded(&bytes, limits).is_err());
    }

    #[test]
    fn hostile_catalog_counts_nesting_duplicates_and_aggregates_fail_preflight() {
        let one = table("APP.ONE");
        assert_eq!(
            decode_table_definitions_bounded(
                &serde_json::to_vec(std::slice::from_ref(&one)).unwrap(),
                Db2Limits::default(),
            )
            .unwrap(),
            vec![one.clone()]
        );

        rejected(
            &[one.clone(), table("APP.TWO")],
            Db2Limits {
                max_tables: 1,
                ..Db2Limits::default()
            },
        );
        rejected(&[one.clone(), table("app.one")], Db2Limits::default());

        let mut nested = one.clone();
        nested.columns.push(nested.columns[0].clone());
        rejected(
            &[nested],
            Db2Limits {
                max_columns: 1,
                ..Db2Limits::default()
            },
        );
        rejected(
            std::slice::from_ref(&one),
            Db2Limits {
                max_primary_key_columns: 0,
                ..Db2Limits::default()
            },
        );
        rejected(
            std::slice::from_ref(&one),
            Db2Limits {
                max_foreign_keys_per_table: 0,
                ..Db2Limits::default()
            },
        );
        rejected(
            std::slice::from_ref(&one),
            Db2Limits {
                max_foreign_key_columns: 0,
                ..Db2Limits::default()
            },
        );
        rejected(
            std::slice::from_ref(&one),
            Db2Limits {
                max_extract_fields: 0,
                ..Db2Limits::default()
            },
        );
        rejected(
            std::slice::from_ref(&one),
            Db2Limits {
                max_catalog_text_bytes: 1,
                ..Db2Limits::default()
            },
        );
        rejected(
            &[one],
            Db2Limits {
                max_catalog_nested_items: 1,
                ..Db2Limits::default()
            },
        );
    }

    #[test]
    fn duplicate_members_and_single_oversized_text_fail_streaming_preflight() {
        let encoded = serde_json::to_string(&[table("APP.ONE")]).unwrap();
        let duplicate_table_name = encoded.replacen(
            "\"name\":\"APP.ONE\"",
            "\"name\":\"APP.ONE\",\"name\":\"APP.TWO\"",
            1,
        );
        assert!(
            decode_table_definitions_bounded(duplicate_table_name.as_bytes(), Db2Limits::default())
                .is_err()
        );
        let duplicate_foreign_key_columns = encoded.replacen(
            "\"columns\":[\"ID\"],\"referenced_table\"",
            "\"columns\":[\"ID\"],\"columns\":[\"ID\"],\"referenced_table\"",
            1,
        );
        assert!(
            decode_table_definitions_bounded(
                duplicate_foreign_key_columns.as_bytes(),
                Db2Limits::default()
            )
            .is_err()
        );
        assert!(
            decode_table_definitions_bounded(
                &serde_json::to_vec(&[table(&"X".repeat(257))]).unwrap(),
                Db2Limits::default()
            )
            .is_err()
        );
    }
}
