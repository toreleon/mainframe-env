use crate::Db2Limits;
use mainframe_env_host_api::{Db2HostVariable, HostProblem};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

mod bounded_decode;
pub use bounded_decode::decode_table_definitions_bounded;

pub const DB2_APPLICATION_CATALOG_CONTRACT: &str = "mainframe-env.db2-application-catalog@1";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Db2ResultEncoding {
    Raw,
    Varchar,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Db2ColumnDefinition {
    pub name: String,
    pub nullable: bool,
    pub max_bytes: usize,
    pub result_encoding: Db2ResultEncoding,
    pub default_value: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Db2ForeignKeyDefinition {
    pub columns: Vec<String>,
    pub referenced_table: String,
    pub referenced_columns: Vec<String>,
    pub delete_restrict: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Db2ExtractField {
    pub column: String,
    pub width: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Db2ExtractLayout {
    pub fields: Vec<Db2ExtractField>,
    pub trailer: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Db2TableDefinition {
    pub name: String,
    pub columns: Vec<Db2ColumnDefinition>,
    pub primary_key: Vec<String>,
    pub foreign_keys: Vec<Db2ForeignKeyDefinition>,
    pub extract: Option<Db2ExtractLayout>,
}

impl Db2TableDefinition {
    pub(crate) fn normalized_name(&self) -> String {
        self.name.to_ascii_uppercase()
    }

    pub(crate) fn column_index(&self, name: &str) -> Option<usize> {
        let name = normalize_identifier(name);
        self.columns
            .iter()
            .position(|column| normalize_identifier(&column.name) == name)
    }

    pub(crate) fn primary_key_indices(&self) -> Result<Vec<usize>, HostProblem> {
        self.primary_key
            .iter()
            .map(|name| self.column_index(name).ok_or(HostProblem::Malformed))
            .collect()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2SeedRow {
    pub table: String,
    pub values: BTreeMap<String, Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2CatalogGeneration {
    pub application: String,
    pub generation: u64,
    pub identity: String,
    pub tables: Vec<Db2TableDefinition>,
    pub rows: Vec<Db2SeedRow>,
}

impl Db2CatalogGeneration {
    pub(crate) fn validate(&self, limits: Db2Limits) -> Result<(), HostProblem> {
        validate_name(&self.application)?;
        validate_sha256(&self.identity)?;
        if self.generation == 0 || self.tables.is_empty() || self.tables.len() > limits.max_tables {
            return Err(HostProblem::Malformed);
        }
        let mut names = BTreeSet::new();
        let mut definitions = BTreeMap::new();
        for table in &self.tables {
            validate_name(&table.name)?;
            let name = table.normalized_name();
            if !names.insert(name.clone())
                || table.columns.is_empty()
                || table.columns.len() > limits.max_columns
                || table.primary_key.is_empty()
                || table.primary_key.len() > limits.max_primary_key_columns
                || table.foreign_keys.len() > limits.max_foreign_keys_per_table
            {
                return Err(HostProblem::Malformed);
            }
            let mut columns = BTreeSet::new();
            for column in &table.columns {
                validate_name(&column.name)?;
                if column.max_bytes == 0
                    || column.max_bytes > limits.max_column_bytes
                    || column
                        .default_value
                        .as_ref()
                        .is_some_and(|value| value.len() > column.max_bytes)
                    || !columns.insert(normalize_identifier(&column.name))
                {
                    return Err(HostProblem::Malformed);
                }
            }
            if table
                .primary_key
                .iter()
                .any(|column| !columns.contains(&normalize_identifier(column)))
                || table.extract.as_ref().is_some_and(|layout| {
                    layout.fields.is_empty()
                        || layout.fields.len() > limits.max_extract_fields
                        || layout.trailer.len() > limits.max_column_bytes
                        || layout.fields.iter().any(|field| {
                            field.width == 0
                                || field.width > limits.max_column_bytes
                                || !columns.contains(&normalize_identifier(&field.column))
                        })
                })
            {
                return Err(HostProblem::Malformed);
            }
            definitions.insert(name, table);
        }
        for table in &self.tables {
            for foreign_key in &table.foreign_keys {
                let target = definitions
                    .get(&foreign_key.referenced_table.to_ascii_uppercase())
                    .ok_or(HostProblem::Malformed)?;
                if foreign_key.columns.is_empty()
                    || foreign_key.columns.len() > limits.max_foreign_key_columns
                    || foreign_key.columns.len() != foreign_key.referenced_columns.len()
                    || foreign_key
                        .columns
                        .iter()
                        .any(|column| table.column_index(column).is_none())
                    || foreign_key
                        .referenced_columns
                        .iter()
                        .any(|column| target.column_index(column).is_none())
                {
                    return Err(HostProblem::Malformed);
                }
            }
        }
        let mut rows_per_table = BTreeMap::<String, usize>::new();
        for row in &self.rows {
            let table_name = row.table.to_ascii_uppercase();
            let table = definitions.get(&table_name).ok_or(HostProblem::Malformed)?;
            let count = rows_per_table.entry(table_name).or_default();
            *count += 1;
            if *count > limits.max_rows_per_table || row.values.len() > limits.max_columns {
                return Err(HostProblem::ResourceExhausted);
            }
            for column in &table.columns {
                let value = value_for_column(&row.values, &column.name);
                if value.is_none() && !column.nullable && column.default_value.is_none()
                    || value.is_some_and(|value| value.len() > column.max_bytes)
                {
                    return Err(HostProblem::Malformed);
                }
            }
            if row
                .values
                .keys()
                .any(|column| table.column_index(column).is_none())
            {
                return Err(HostProblem::Malformed);
            }
        }
        Ok(())
    }
}

pub(crate) fn normalize_identifier(value: &str) -> String {
    value
        .bytes()
        .filter(|byte| byte.is_ascii_alphanumeric())
        .map(|byte| byte.to_ascii_uppercase() as char)
        .collect()
}

pub(crate) fn value_for_column<'a, T>(
    values: &'a BTreeMap<String, T>,
    column: &str,
) -> Option<&'a T> {
    let column = normalize_identifier(column);
    values
        .iter()
        .find(|(name, _)| normalize_identifier(name) == column)
        .map(|(_, value)| value)
}

pub(crate) fn input_for_column<'a>(
    inputs: &'a BTreeMap<String, Db2HostVariable>,
    column: &str,
) -> Option<&'a Db2HostVariable> {
    let column = normalize_identifier(column);
    inputs
        .iter()
        .filter(|(name, _)| normalize_identifier(name).ends_with(&column))
        .min_by_key(|(name, _)| name.len())
        .map(|(_, value)| value)
}

fn validate_name(value: &str) -> Result<(), HostProblem> {
    if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn validate_sha256(value: &str) -> Result<(), HostProblem> {
    if value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        Ok(())
    } else {
        Err(HostProblem::Malformed)
    }
}
