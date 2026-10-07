//! Independent bounded reference simulation for the frozen 0.6 dataset surface.
//!
//! This module deliberately owns its types and transition rules. It consumes
//! frozen catalog/fixture data, but it does not import or call product dataset,
//! AMS, host, or provider-store code and it never produces licensed evidence.

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const OFFICIAL_CATALOG: &str =
    include_str!("../../../../conformance/subsystems/coverage/catalogs/dataset-vsam-ams.json");
const ORGANIZATION_FIXTURES: &str =
    include_str!("../../../../conformance/subsystems/dataset/fixtures/dataset-organizations.json");
const AMS_FIXTURES: &str =
    include_str!("../../../../conformance/subsystems/dataset/fixtures/ams-commands.json");

const MAX_DATASETS: usize = 64;
const MAX_RECORDS: usize = 256;
const MAX_RECORD_BYTES: usize = 4_096;
const MAX_TOTAL_BYTES: usize = 1024 * 1024;
const MAX_ALIASES: usize = 64;
const MAX_GENERATIONS: usize = 32;
const MAX_NAME_BYTES: usize = 44;

const OPTION_CAPABILITIES: &[(&str, &str)] = &[
    ("physical-disk", "physical-volumes"),
    ("tape", "tape"),
    ("acs-exit", "sms-acs"),
    ("encryption", "encryption"),
    ("compression", "compression"),
    ("migration", "migration-recall"),
    ("recall", "migration-recall"),
    ("installation-defined", "unknown"),
    ("timing", "unknown"),
    ("undocumented-internal", "unknown"),
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatasetReferenceSimulationReport {
    pub official_rows: usize,
    pub organization_rows: usize,
    pub command_rows: usize,
    pub property_cases: usize,
    pub observation_perturbations_rejected: usize,
    pub differential_credit: usize,
    pub digest: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Organization {
    KeySequenced,
    EntrySequenced,
    Relative,
    VariableRelative,
    Linear,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Condition {
    Normal,
    Duplicate,
    NotFound,
    Length,
    InvalidAccess,
    Capability,
    InjectedFailure,
}

impl Condition {
    const fn cc(self) -> u8 {
        match self {
            Self::Normal => 0,
            Self::NotFound => 8,
            Self::Duplicate | Self::Length | Self::InvalidAccess | Self::Capability => 12,
            Self::InjectedFailure => 16,
        }
    }

    const fn tag(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Duplicate => "DUPREC",
            Self::NotFound => "NOTFND",
            Self::Length => "LENGERR",
            Self::InvalidAccess => "INVREQ",
            Self::Capability => "CAPABILITY",
            Self::InjectedFailure => "UNKNOWN",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Reply {
    condition: Condition,
    records: Vec<Vec<u8>>,
    identities: Vec<Vec<u8>>,
}

impl Reply {
    fn normal(records: Vec<Vec<u8>>, identities: Vec<Vec<u8>>) -> Self {
        Self {
            condition: Condition::Normal,
            records,
            identities,
        }
    }

    fn condition(condition: Condition) -> Self {
        Self {
            condition,
            records: Vec::new(),
            identities: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Definition {
    record_length: usize,
    key_offset: usize,
    key_length: usize,
    fixed: bool,
    block_size: usize,
    buffer_count: usize,
    primary: u64,
    secondary: u64,
    data_class: String,
    management_class: String,
    storage_class: String,
    volumes: Vec<String>,
    lifecycle: Lifecycle,
    backup_generation: u64,
}

impl Definition {
    fn for_organization(organization: Organization) -> Self {
        Self {
            record_length: if organization == Organization::Linear {
                1_024
            } else {
                8
            },
            key_offset: 0,
            key_length: if organization == Organization::KeySequenced {
                2
            } else {
                0
            },
            fixed: organization == Organization::Relative,
            block_size: 0,
            buffer_count: 5,
            primary: 1,
            secondary: 0,
            data_class: "STANDARD".into(),
            management_class: "ACTIVE".into(),
            storage_class: "ABSTRACT".into(),
            volumes: vec!["MENV00".into()],
            lifecycle: Lifecycle::Cataloged,
            backup_generation: 0,
        }
    }

    fn validate(&self) -> Result<(), String> {
        if self.record_length == 0
            || self.record_length > MAX_RECORD_BYTES
            || self.key_offset.saturating_add(self.key_length) > self.record_length
            || self.buffer_count == 0
            || self.primary == 0
            || self.volumes.is_empty()
            || self.volumes.iter().any(|volume| volume.is_empty())
        {
            Err("invalid reference definition".into())
        } else {
            Ok(())
        }
    }

    fn canonical(&self, output: &mut String) {
        output.push_str(&format!(
            "L={};K={},{};F={};B={},{};S={},{};C={},{},{};V={};Y={:?};G={};",
            self.record_length,
            self.key_offset,
            self.key_length,
            u8::from(self.fixed),
            self.block_size,
            self.buffer_count,
            self.primary,
            self.secondary,
            self.data_class,
            self.management_class,
            self.storage_class,
            self.volumes.join(","),
            self.lifecycle,
            self.backup_generation,
        ));
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Lifecycle {
    Cataloged,
    Open,
    Closed,
    RecoveryRequired,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ReferenceDataset {
    organization: Organization,
    definition: Definition,
    records: Vec<Vec<u8>>,
    relative: BTreeMap<u64, Vec<u8>>,
    linear: Vec<u8>,
    version: u64,
}

impl ReferenceDataset {
    fn new(organization: Organization) -> Result<Self, String> {
        let definition = Definition::for_organization(organization);
        definition.validate()?;
        Ok(Self {
            organization,
            definition,
            records: Vec::new(),
            relative: BTreeMap::new(),
            linear: Vec::new(),
            version: 1,
        })
    }

    fn validate_record(&self, record: &[u8]) -> Result<(), Condition> {
        if record.len() > MAX_RECORD_BYTES
            || (self.definition.fixed && record.len() != self.definition.record_length)
            || (!self.definition.fixed
                && self.organization != Organization::Linear
                && record.len() > self.definition.record_length)
        {
            Err(Condition::Length)
        } else {
            Ok(())
        }
    }

    fn key(&self, record: &[u8]) -> Result<Vec<u8>, Condition> {
        if self.organization != Organization::KeySequenced {
            return Err(Condition::InvalidAccess);
        }
        let end = self
            .definition
            .key_offset
            .checked_add(self.definition.key_length)
            .ok_or(Condition::Length)?;
        record
            .get(self.definition.key_offset..end)
            .map(<[u8]>::to_vec)
            .ok_or(Condition::Length)
    }

    fn insert(&mut self, record: Vec<u8>) -> Reply {
        if let Err(condition) = self.validate_record(&record) {
            return Reply::condition(condition);
        }
        match self.organization {
            Organization::KeySequenced => {
                let Ok(key) = self.key(&record) else {
                    return Reply::condition(Condition::Length);
                };
                if self
                    .records
                    .iter()
                    .any(|candidate| self.key(candidate).ok().as_ref() == Some(&key))
                {
                    return Reply::condition(Condition::Duplicate);
                }
                self.records.push(record);
                let key_offset = self.definition.key_offset;
                let key_length = self.definition.key_length;
                self.records.sort_by(|left, right| {
                    left[key_offset..key_offset + key_length]
                        .cmp(&right[key_offset..key_offset + key_length])
                });
            }
            Organization::EntrySequenced => self.records.push(record),
            _ => return Reply::condition(Condition::InvalidAccess),
        }
        self.version = self.version.saturating_add(1);
        Reply::normal(Vec::new(), Vec::new())
    }

    fn read_key(&self, key: &[u8]) -> Reply {
        if self.organization != Organization::KeySequenced {
            return Reply::condition(Condition::InvalidAccess);
        }
        self.records
            .iter()
            .find(|record| self.key(record).ok().as_deref() == Some(key))
            .map_or_else(
                || Reply::condition(Condition::NotFound),
                |record| Reply::normal(vec![record.clone()], vec![key.to_vec()]),
            )
    }

    fn read_generic(&self, prefix: &[u8]) -> Reply {
        if self.organization != Organization::KeySequenced || prefix.is_empty() {
            return Reply::condition(Condition::InvalidAccess);
        }
        let selected = self
            .records
            .iter()
            .filter_map(|record| {
                let key = self.key(record).ok()?;
                key.starts_with(prefix).then(|| (record.clone(), key))
            })
            .collect::<Vec<_>>();
        Reply::normal(
            selected.iter().map(|(record, _)| record.clone()).collect(),
            selected.into_iter().map(|(_, key)| key).collect(),
        )
    }

    fn read_rba(&self, rba: u64, max: usize) -> Reply {
        match self.organization {
            Organization::EntrySequenced => {
                let mut current = 0u64;
                for record in &self.records {
                    if current == rba {
                        return Reply::normal(
                            vec![record.clone()],
                            vec![rba.to_be_bytes().to_vec()],
                        );
                    }
                    current = current.saturating_add(record.len() as u64);
                }
                Reply::condition(Condition::NotFound)
            }
            Organization::Linear => usize::try_from(rba)
                .ok()
                .and_then(|start| self.linear.get(start..))
                .map_or_else(
                    || Reply::condition(Condition::NotFound),
                    |remaining| {
                        Reply::normal(vec![remaining[..remaining.len().min(max)].to_vec()], vec![])
                    },
                ),
            _ => Reply::condition(Condition::InvalidAccess),
        }
    }

    fn write_relative(&mut self, rrn: u64, record: Vec<u8>) -> Reply {
        if !matches!(
            self.organization,
            Organization::Relative | Organization::VariableRelative
        ) || rrn == 0
        {
            return Reply::condition(Condition::InvalidAccess);
        }
        if let Err(condition) = self.validate_record(&record) {
            return Reply::condition(condition);
        }
        self.relative.insert(rrn, record);
        self.version = self.version.saturating_add(1);
        Reply::normal(Vec::new(), vec![rrn.to_be_bytes().to_vec()])
    }

    fn read_relative(&self, rrn: u64) -> Reply {
        if !matches!(
            self.organization,
            Organization::Relative | Organization::VariableRelative
        ) {
            return Reply::condition(Condition::InvalidAccess);
        }
        self.relative.get(&rrn).map_or_else(
            || Reply::condition(Condition::NotFound),
            |record| Reply::normal(vec![record.clone()], vec![rrn.to_be_bytes().to_vec()]),
        )
    }

    fn write_linear(&mut self, rba: u64, data: &[u8]) -> Reply {
        if self.organization != Organization::Linear {
            return Reply::condition(Condition::InvalidAccess);
        }
        let Ok(start) = usize::try_from(rba) else {
            return Reply::condition(Condition::Length);
        };
        if start > self.linear.len()
            || start.saturating_add(data.len()) > MAX_TOTAL_BYTES
            || data.len() > MAX_RECORD_BYTES
        {
            return Reply::condition(if start > self.linear.len() {
                Condition::NotFound
            } else {
                Condition::Length
            });
        }
        if start == self.linear.len() {
            self.linear.extend_from_slice(data);
        } else {
            let end = start.saturating_add(data.len());
            if end > self.linear.len() {
                self.linear.resize(end, 0);
            }
            self.linear[start..end].copy_from_slice(data);
        }
        self.version = self.version.saturating_add(1);
        Reply::normal(Vec::new(), Vec::new())
    }

    fn sequential(&self, reverse: bool) -> Reply {
        let mut selected = match self.organization {
            Organization::KeySequenced | Organization::EntrySequenced => self.records.clone(),
            Organization::Relative | Organization::VariableRelative => {
                self.relative.values().cloned().collect()
            }
            Organization::Linear => return Reply::condition(Condition::InvalidAccess),
        };
        if reverse {
            selected.reverse();
        }
        Reply::normal(selected, Vec::new())
    }

    fn canonical(&self, name: &str, output: &mut String) {
        output.push_str(&format!(
            "D={name};O={:?};R={};",
            self.organization, self.version
        ));
        self.definition.canonical(output);
        for record in &self.records {
            push_bytes(output, record);
        }
        for (rrn, record) in &self.relative {
            output.push_str(&format!("N={rrn};"));
            push_bytes(output, record);
        }
        output.push_str("X=");
        push_bytes(output, &self.linear);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AlternateIndex {
    base: String,
    offset: usize,
    length: usize,
    upgrade: bool,
    identities: BTreeMap<Vec<u8>, Vec<Vec<u8>>>,
    version: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct GenerationGroup {
    limit: usize,
    scratch: bool,
    next: u64,
    active: Vec<String>,
    retired: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Registers {
    maxcc: u8,
    lastcc: u8,
}

impl Registers {
    fn apply(&mut self, condition: Condition) {
        self.lastcc = condition.cc();
        self.maxcc = self.maxcc.max(self.lastcc);
    }

    fn set_last(&mut self, value: u8) {
        self.lastcc = value;
        self.maxcc = self.maxcc.max(value);
    }

    fn set_max(&mut self, value: u8) {
        self.maxcc = value;
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ReferenceState {
    datasets: BTreeMap<String, ReferenceDataset>,
    aliases: BTreeMap<String, String>,
    indexes: BTreeMap<String, AlternateIndex>,
    paths: BTreeMap<String, String>,
    catalogs: BTreeMap<String, bool>,
    groups: BTreeMap<String, GenerationGroup>,
    backups: BTreeMap<String, ReferenceDataset>,
    registers: Registers,
    recovery_unknown: bool,
}

impl ReferenceState {
    fn new() -> Self {
        Self {
            datasets: BTreeMap::new(),
            aliases: BTreeMap::new(),
            indexes: BTreeMap::new(),
            paths: BTreeMap::new(),
            catalogs: BTreeMap::new(),
            groups: BTreeMap::new(),
            backups: BTreeMap::new(),
            registers: Registers {
                maxcc: 0,
                lastcc: 0,
            },
            recovery_unknown: false,
        }
    }

    fn define(&mut self, name: &str, organization: Organization) -> Condition {
        if !valid_name(name)
            || self.datasets.len() >= MAX_DATASETS
            || self.datasets.contains_key(name)
            || self.aliases.contains_key(name)
        {
            return Condition::Duplicate;
        }
        match ReferenceDataset::new(organization) {
            Ok(dataset) => {
                self.datasets.insert(name.into(), dataset);
                Condition::Normal
            }
            Err(_) => Condition::Length,
        }
    }

    fn delete(&mut self, name: &str) -> Condition {
        let resolved = self.resolve(name).unwrap_or_else(|| name.to_string());
        if self.datasets.remove(&resolved).is_none() {
            return Condition::NotFound;
        }
        self.indexes.retain(|_, index| index.base != resolved);
        self.paths
            .retain(|_, index| self.indexes.contains_key(index));
        self.aliases.retain(|_, target| target != &resolved);
        Condition::Normal
    }

    fn define_alias(&mut self, alias: &str, target: &str) -> Condition {
        if self.aliases.len() >= MAX_ALIASES
            || !valid_name(alias)
            || self.resolve(target).is_none()
            || self.datasets.contains_key(alias)
            || self.aliases.contains_key(alias)
        {
            return Condition::Duplicate;
        }
        self.aliases.insert(alias.into(), target.into());
        Condition::Normal
    }

    fn retarget_alias(&mut self, alias: &str, target: &str) -> Condition {
        if !self.aliases.contains_key(alias) || self.resolve(target).is_none() {
            return Condition::NotFound;
        }
        self.aliases.insert(alias.into(), target.into());
        Condition::Normal
    }

    fn resolve(&self, name: &str) -> Option<String> {
        let mut current = name.to_string();
        let mut seen = BTreeSet::new();
        for _ in 0..=MAX_ALIASES {
            if self.datasets.contains_key(&current) || self.catalogs.contains_key(&current) {
                return Some(current);
            }
            if !seen.insert(current.clone()) {
                return None;
            }
            current = self.aliases.get(&current)?.clone();
        }
        None
    }

    fn define_index(
        &mut self,
        index: &str,
        base: &str,
        offset: usize,
        length: usize,
        upgrade: bool,
    ) -> Condition {
        let Some(dataset) = self.datasets.get(base) else {
            return Condition::NotFound;
        };
        if dataset.organization != Organization::KeySequenced
            || length == 0
            || offset.saturating_add(length) > dataset.definition.record_length
            || self.indexes.contains_key(index)
        {
            return Condition::InvalidAccess;
        }
        let mut definition = AlternateIndex {
            base: base.into(),
            offset,
            length,
            upgrade,
            identities: BTreeMap::new(),
            version: 1,
        };
        if rebuild_index(dataset, &mut definition).is_err() {
            return Condition::Duplicate;
        }
        self.indexes.insert(index.into(), definition);
        Condition::Normal
    }

    fn define_path(&mut self, path: &str, index: &str) -> Condition {
        if self.paths.contains_key(path) || !self.indexes.contains_key(index) {
            Condition::NotFound
        } else {
            self.paths.insert(path.into(), index.into());
            Condition::Normal
        }
    }

    fn insert_base(&mut self, base: &str, record: Vec<u8>) -> Condition {
        let Some(current) = self.datasets.get(base).cloned() else {
            return Condition::NotFound;
        };
        let mut next = current.clone();
        let reply = next.insert(record);
        if reply.condition != Condition::Normal {
            return reply.condition;
        }
        let mut updated = Vec::new();
        for (name, index) in self
            .indexes
            .iter()
            .filter(|(_, index)| index.base == base && index.upgrade)
        {
            let mut next_index = index.clone();
            if rebuild_index(&next, &mut next_index).is_err() {
                return Condition::Duplicate;
            }
            next_index.version = next_index.version.saturating_add(1);
            updated.push((name.clone(), next_index));
        }
        self.datasets.insert(base.into(), next);
        for (name, index) in updated {
            self.indexes.insert(name, index);
        }
        Condition::Normal
    }

    fn build_index(&mut self, index: &str) -> Condition {
        let Some(mut next) = self.indexes.get(index).cloned() else {
            return Condition::NotFound;
        };
        let Some(base) = self.datasets.get(&next.base) else {
            return Condition::NotFound;
        };
        if rebuild_index(base, &mut next).is_err() {
            return Condition::Duplicate;
        }
        next.version = next.version.saturating_add(1);
        self.indexes.insert(index.into(), next);
        Condition::Normal
    }

    fn define_group(&mut self, base: &str, limit: usize, scratch: bool) -> Condition {
        if limit == 0 || limit > MAX_GENERATIONS || self.groups.contains_key(base) {
            return Condition::InvalidAccess;
        }
        self.groups.insert(
            base.into(),
            GenerationGroup {
                limit,
                scratch,
                next: 1,
                active: Vec::new(),
                retired: Vec::new(),
            },
        );
        Condition::Normal
    }

    fn create_generation(&mut self, base: &str, records: Vec<Vec<u8>>) -> Condition {
        let Some(mut group) = self.groups.get(base).cloned() else {
            return Condition::NotFound;
        };
        let name = format!("{base}.G{:04}V00", group.next);
        group.next = group.next.saturating_add(1);
        let Ok(mut dataset) = ReferenceDataset::new(Organization::EntrySequenced) else {
            return Condition::Length;
        };
        for record in records {
            if dataset.insert(record).condition != Condition::Normal {
                return Condition::Length;
            }
        }
        self.datasets.insert(name.clone(), dataset);
        group.active.push(name);
        while group.active.len() > group.limit {
            let retired = group.active.remove(0);
            if group.scratch {
                self.datasets.remove(&retired);
            } else {
                group.retired.push(retired);
            }
        }
        self.groups.insert(base.into(), group);
        Condition::Normal
    }

    fn repro(&mut self, source: &str, target: &str) -> Condition {
        let Some(source) = self.datasets.get(source).cloned() else {
            return Condition::NotFound;
        };
        let Some(target_dataset) = self.datasets.get_mut(target) else {
            return Condition::NotFound;
        };
        if source.organization != target_dataset.organization {
            return Condition::InvalidAccess;
        }
        target_dataset.records = source.records;
        target_dataset.relative = source.relative;
        target_dataset.linear = source.linear;
        target_dataset.version = target_dataset.version.saturating_add(1);
        Condition::Normal
    }

    fn export(&mut self, name: &str) -> Condition {
        let Some(mut snapshot) = self.datasets.get(name).cloned() else {
            return Condition::NotFound;
        };
        snapshot.definition.backup_generation =
            snapshot.definition.backup_generation.saturating_add(1);
        self.backups.insert(name.into(), snapshot);
        Condition::Normal
    }

    fn import(&mut self, source: &str, target: &str) -> Condition {
        let Some(snapshot) = self.backups.get(source).cloned() else {
            return Condition::NotFound;
        };
        self.datasets.insert(target.into(), snapshot);
        Condition::Normal
    }

    fn checkpoint(&self) -> Checkpoint {
        Checkpoint {
            state: Box::new(self.clone()),
            digest: digest_text(&self.canonical()),
        }
    }

    fn restore(checkpoint: Checkpoint) -> Result<Self, String> {
        if checkpoint.digest != digest_text(&checkpoint.state.canonical()) {
            Err("reference checkpoint integrity mismatch".into())
        } else {
            Ok(*checkpoint.state)
        }
    }

    fn atomic(
        &mut self,
        inject_failure: bool,
        transition: impl FnOnce(&mut Self) -> Condition,
    ) -> Condition {
        let mut next = self.clone();
        let condition = transition(&mut next);
        if inject_failure {
            Condition::InjectedFailure
        } else if condition == Condition::Normal {
            *self = next;
            condition
        } else {
            condition
        }
    }

    fn canonical(&self) -> String {
        let mut output = String::new();
        for (name, dataset) in &self.datasets {
            dataset.canonical(name, &mut output);
        }
        for (alias, target) in &self.aliases {
            output.push_str(&format!("A={alias}>{target};"));
        }
        for (name, index) in &self.indexes {
            output.push_str(&format!(
                "I={name}>{}:{}:{}:{}:{};",
                index.base,
                index.offset,
                index.length,
                u8::from(index.upgrade),
                index.version,
            ));
            for (alternate, primaries) in &index.identities {
                push_bytes(&mut output, alternate);
                for primary in primaries {
                    push_bytes(&mut output, primary);
                }
            }
        }
        for (path, index) in &self.paths {
            output.push_str(&format!("P={path}>{index};"));
        }
        for (name, connected) in &self.catalogs {
            output.push_str(&format!("C={name}:{};", u8::from(*connected)));
        }
        for (base, group) in &self.groups {
            output.push_str(&format!(
                "G={base}:{}:{}:{}:{}:{};",
                group.limit,
                u8::from(group.scratch),
                group.next,
                group.active.join(","),
                group.retired.join(","),
            ));
        }
        output.push_str(&format!(
            "CC={},{};U={};",
            self.registers.maxcc,
            self.registers.lastcc,
            u8::from(self.recovery_unknown),
        ));
        output
    }

    fn validate(&self) -> Result<(), String> {
        if self.datasets.len() > MAX_DATASETS
            || self.aliases.len() > MAX_ALIASES
            || self
                .datasets
                .values()
                .any(|dataset| dataset.records.len() + dataset.relative.len() > MAX_RECORDS)
            || self
                .datasets
                .values()
                .map(|dataset| {
                    dataset.records.iter().map(Vec::len).sum::<usize>()
                        + dataset.relative.values().map(Vec::len).sum::<usize>()
                        + dataset.linear.len()
                })
                .sum::<usize>()
                > MAX_TOTAL_BYTES
        {
            return Err("reference model bounds exceeded".into());
        }
        for (alias, target) in &self.aliases {
            if alias == target || self.resolve(alias).is_none() {
                return Err("reference alias graph is invalid".into());
            }
        }
        for index in self.indexes.values() {
            let base = self
                .datasets
                .get(&index.base)
                .ok_or_else(|| "reference AIX has no base".to_string())?;
            let mut rebuilt = index.clone();
            rebuild_index(base, &mut rebuilt)?;
            if index.upgrade && rebuilt.identities != index.identities {
                return Err("reference AIX drifted from base".into());
            }
        }
        if self
            .paths
            .values()
            .any(|index| !self.indexes.contains_key(index))
        {
            return Err("reference path has no AIX".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Checkpoint {
    state: Box<ReferenceState>,
    digest: String,
}

fn rebuild_index(base: &ReferenceDataset, index: &mut AlternateIndex) -> Result<(), String> {
    let mut identities = BTreeMap::<Vec<u8>, Vec<Vec<u8>>>::new();
    for record in &base.records {
        let end = index.offset.saturating_add(index.length);
        let alternate = record
            .get(index.offset..end)
            .ok_or_else(|| "reference alternate key is out of range".to_string())?
            .to_vec();
        let primary = base
            .key(record)
            .map_err(|_| "reference base key is invalid".to_string())?;
        identities.entry(alternate).or_default().push(primary);
    }
    index.identities = identities;
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CommandAction {
    Allocate,
    Alter,
    Capability,
    BuildIndex,
    Collect,
    DefineAlias,
    DefineIndex,
    DefineCluster,
    DefineGdg,
    DefineNonVsam,
    DefinePath,
    DefineCatalog,
    Delete,
    Diagnose,
    Examine,
    Export,
    Disconnect,
    Import,
    Connect,
    ListCatalog,
    ListData,
    Print,
    Repro,
    Recover,
    Shcds,
    Verify,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CommandSpec {
    id: &'static str,
    label: &'static str,
    action: CommandAction,
    capability: Option<&'static str>,
}

const COMMANDS: &[CommandSpec] = &[
    command("allocate", "ALLOCATE", CommandAction::Allocate),
    command("alter", "ALTER", CommandAction::Alter),
    capability("alter-libraryentry", "ALTER LIBRARYENTRY", "tape"),
    capability("alter-volumeentry", "ALTER VOLUMEENTRY", "tape"),
    command("bldindex", "BLDINDEX", CommandAction::BuildIndex),
    capability("create-libraryentry", "CREATE LIBRARYENTRY", "tape"),
    capability("create-volumeentry", "CREATE VOLUMEENTRY", "tape"),
    command("dcollect", "DCOLLECT", CommandAction::Collect),
    command("define-alias", "DEFINE ALIAS", CommandAction::DefineAlias),
    command(
        "define-alternateindex",
        "DEFINE ALTERNATEINDEX",
        CommandAction::DefineIndex,
    ),
    command(
        "define-cluster",
        "DEFINE CLUSTER",
        CommandAction::DefineCluster,
    ),
    command(
        "define-generationdatagroup",
        "DEFINE GENERATIONDATAGROUP",
        CommandAction::DefineGdg,
    ),
    command(
        "define-nonvsam",
        "DEFINE NONVSAM",
        CommandAction::DefineNonVsam,
    ),
    capability("define-pagespace", "DEFINE PAGESPACE", "paging"),
    command("define-path", "DEFINE PATH", CommandAction::DefinePath),
    command(
        "define-usercatalog",
        "DEFINE USERCATALOG",
        CommandAction::DefineCatalog,
    ),
    command("delete", "DELETE", CommandAction::Delete),
    command("diagnose", "DIAGNOSE", CommandAction::Diagnose),
    command("examine", "EXAMINE", CommandAction::Examine),
    command("export", "EXPORT", CommandAction::Export),
    command(
        "export-disconnect",
        "EXPORT DISCONNECT",
        CommandAction::Disconnect,
    ),
    command("import", "IMPORT", CommandAction::Import),
    command("import-connect", "IMPORT CONNECT", CommandAction::Connect),
    command("listcat", "LISTCAT", CommandAction::ListCatalog),
    command("listdata", "LISTDATA", CommandAction::ListData),
    command("print", "PRINT", CommandAction::Print),
    command("repro", "REPRO", CommandAction::Repro),
    command("recover", "RECOVER", CommandAction::Recover),
    capability("setcache", "SETCACHE", "storage-controller-cache"),
    command("shcds", "SHCDS", CommandAction::Shcds),
    command("verify", "VERIFY", CommandAction::Verify),
];

const fn command(id: &'static str, label: &'static str, action: CommandAction) -> CommandSpec {
    CommandSpec {
        id,
        label,
        action,
        capability: None,
    }
}

const fn capability(
    id: &'static str,
    label: &'static str,
    capability: &'static str,
) -> CommandSpec {
    CommandSpec {
        id,
        label,
        action: CommandAction::Capability,
        capability: Some(capability),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CommandObservation {
    id: String,
    condition: Condition,
    maxcc: u8,
    lastcc: u8,
    state_digest: String,
}

fn exercise_command(spec: CommandSpec) -> Result<CommandObservation, String> {
    let mut state = seeded_state()?;
    let condition = match spec.action {
        CommandAction::Capability => Condition::Capability,
        CommandAction::Allocate => state.define("USER.ALLOC", Organization::EntrySequenced),
        CommandAction::Alter => {
            let Some(dataset) = state.datasets.get_mut("USER.BASE") else {
                return Err("reference ALTER setup is missing".into());
            };
            dataset.definition.lifecycle = Lifecycle::Open;
            dataset.definition.buffer_count = 7;
            dataset.version = dataset.version.saturating_add(1);
            Condition::Normal
        }
        CommandAction::BuildIndex => {
            let _ = state.define_index("USER.AIX", "USER.BASE", 2, 2, true);
            state.build_index("USER.AIX")
        }
        CommandAction::Collect | CommandAction::ListCatalog => {
            if state.datasets.keys().is_sorted() {
                Condition::Normal
            } else {
                Condition::InvalidAccess
            }
        }
        CommandAction::DefineAlias => state.define_alias("USER.ALIAS", "USER.BASE"),
        CommandAction::DefineIndex => state.define_index("USER.AIX", "USER.BASE", 2, 2, true),
        CommandAction::DefineCluster => state.define("USER.CLUSTER", Organization::KeySequenced),
        CommandAction::DefineGdg => state.define_group("USER.GDG", 2, true),
        CommandAction::DefineNonVsam => state.define("USER.NONVSAM", Organization::EntrySequenced),
        CommandAction::DefinePath => {
            let _ = state.define_index("USER.AIX", "USER.BASE", 2, 2, true);
            state.define_path("USER.PATH", "USER.AIX")
        }
        CommandAction::DefineCatalog => {
            state.catalogs.insert("USER.CAT".into(), true);
            Condition::Normal
        }
        CommandAction::Delete => state.delete("USER.BASE"),
        CommandAction::Diagnose | CommandAction::Examine => {
            if state.validate().is_ok() {
                Condition::Normal
            } else {
                Condition::InvalidAccess
            }
        }
        CommandAction::Export => state.export("USER.BASE"),
        CommandAction::Disconnect => {
            state.catalogs.insert("USER.CAT".into(), false);
            Condition::Normal
        }
        CommandAction::Import => {
            let _ = state.export("USER.BASE");
            state.import("USER.BASE", "USER.IMPORT")
        }
        CommandAction::Connect => {
            state.catalogs.insert("USER.CAT".into(), true);
            Condition::Normal
        }
        CommandAction::ListData | CommandAction::Print => state
            .datasets
            .get("USER.BASE")
            .map_or(Condition::NotFound, |_| Condition::Normal),
        CommandAction::Repro => {
            let _ = state.define("USER.COPY", Organization::KeySequenced);
            state.repro("USER.BASE", "USER.COPY")
        }
        CommandAction::Recover => {
            let _ = state.export("USER.BASE");
            let condition = state.import("USER.BASE", "USER.RECOVER");
            if let Some(dataset) = state.datasets.get_mut("USER.RECOVER") {
                dataset.definition.lifecycle = Lifecycle::Closed;
            }
            condition
        }
        CommandAction::Shcds => {
            state.recovery_unknown = true;
            state.recovery_unknown = false;
            Condition::Normal
        }
        CommandAction::Verify => {
            if let Some(dataset) = state.datasets.get_mut("USER.BASE") {
                dataset.definition.lifecycle = Lifecycle::RecoveryRequired;
                dataset.definition.lifecycle = Lifecycle::Closed;
                Condition::Normal
            } else {
                Condition::NotFound
            }
        }
    };
    state.registers.apply(condition);
    state.validate()?;
    Ok(CommandObservation {
        id: spec.id.into(),
        condition,
        maxcc: state.registers.maxcc,
        lastcc: state.registers.lastcc,
        state_digest: digest_text(&state.canonical()),
    })
}

fn seeded_state() -> Result<ReferenceState, String> {
    let mut state = ReferenceState::new();
    if state.define("USER.BASE", Organization::KeySequenced) != Condition::Normal {
        return Err("reference seed definition failed".into());
    }
    for record in [b"AA11".to_vec(), b"BB22".to_vec()] {
        if state.insert_base("USER.BASE", record) != Condition::Normal {
            return Err("reference seed insert failed".into());
        }
    }
    state.catalogs.insert("MASTER.CAT".into(), true);
    state.validate()?;
    Ok(state)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExpectationSource {
    IndependentReference,
    ProductionObservation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ExpectedObservation {
    source: ExpectationSource,
    condition: Condition,
    maxcc: u8,
    lastcc: u8,
    state_digest: String,
    records: Vec<Vec<u8>>,
}

fn verify_observation(expected: &ExpectedObservation, actual: &ExpectedObservation) -> bool {
    expected.source == ExpectationSource::IndependentReference
        && actual.source == ExpectationSource::IndependentReference
        && expected.condition == actual.condition
        && expected.maxcc == actual.maxcc
        && expected.lastcc == actual.lastcc
        && expected.state_digest == actual.state_digest
        && expected.records == actual.records
}

pub fn run_dataset_reference_simulation() -> Result<DatasetReferenceSimulationReport, String> {
    let official = parse_json(OFFICIAL_CATALOG, "official dataset catalog")?;
    let units = official["units"]
        .as_array()
        .ok_or_else(|| "official dataset catalog units are missing".to_string())?;
    let official_rows = units.iter().try_fold(0usize, |total, unit| {
        total
            .checked_add(
                unit["rows"]
                    .as_array()
                    .ok_or_else(|| "official dataset catalog rows are missing".to_string())?
                    .len(),
            )
            .ok_or_else(|| "official row count overflow".to_string())
    })?;
    if official_rows != 36 {
        return Err("official dataset denominator is not 36".into());
    }

    let organization_fixture = parse_json(ORGANIZATION_FIXTURES, "organization fixtures")?;
    let organization_cases = organization_fixture["cases"]
        .as_array()
        .ok_or_else(|| "organization fixture cases are missing".to_string())?;
    if organization_cases.len() != 10 {
        return Err("organization fixture denominator is not 10".into());
    }
    let fixture_organizations = organization_cases
        .iter()
        .filter_map(|case| case["organization"].as_str())
        .collect::<BTreeSet<_>>();
    if fixture_organizations != BTreeSet::from(["esds", "ksds", "lds", "rrds", "vrrds"]) {
        return Err("organization fixtures do not cover the frozen five".into());
    }

    let ams_fixture = parse_json(AMS_FIXTURES, "AMS fixtures")?;
    let ams_cases = ams_fixture["cases"]
        .as_array()
        .ok_or_else(|| "AMS fixture cases are missing".to_string())?;
    if ams_cases.len() != COMMANDS.len() {
        return Err("AMS fixture and reference command counts differ".into());
    }
    for (fixture, spec) in ams_cases.iter().zip(COMMANDS) {
        if fixture["command_id"].as_str() != Some(spec.id)
            || !fixture["expected"]
                .as_str()
                .is_some_and(|expected| expected.contains(&format!("label={}", spec.label)))
            || fixture["expected"]
                .as_str()
                .and_then(|expected| expected.rsplit_once("capability="))
                .map(|(_, capability)| capability)
                != Some(spec.capability.unwrap_or("none"))
        {
            return Err(format!(
                "AMS fixture drifted from reference table at {}",
                spec.id
            ));
        }
    }

    let mut transcript = String::new();
    for organization in [
        Organization::KeySequenced,
        Organization::EntrySequenced,
        Organization::Relative,
        Organization::VariableRelative,
        Organization::Linear,
    ] {
        let observation = exercise_organization(organization)?;
        transcript.push_str(&observation);
    }
    for spec in COMMANDS {
        let observation = exercise_command(*spec)?;
        let expected = if spec.capability.is_some() { 12 } else { 0 };
        if observation.condition.cc() != expected
            || observation.lastcc != expected
            || observation.maxcc != expected
        {
            return Err(format!("reference command {} has wrong CC", spec.id));
        }
        transcript.push_str(&format!(
            "CMD={}:{}:{}:{};",
            observation.id,
            observation.condition.tag(),
            observation.maxcc,
            observation.state_digest,
        ));
    }

    let properties = run_properties()?;
    for property in &properties {
        transcript.push_str(property);
    }
    let observation_perturbations_rejected = reject_observation_perturbations()?;
    if observation_perturbations_rejected != 8 {
        return Err(
            "reference comparator did not reject all eight observation perturbations".into(),
        );
    }
    transcript.push_str(&format!(
        "OBSERVATION_PERTURBATIONS={observation_perturbations_rejected};DIFFERENTIAL=0;"
    ));
    Ok(DatasetReferenceSimulationReport {
        official_rows,
        organization_rows: 5,
        command_rows: COMMANDS.len(),
        property_cases: properties.len(),
        observation_perturbations_rejected,
        differential_credit: 0,
        digest: digest_text(&transcript),
    })
}

fn exercise_organization(organization: Organization) -> Result<String, String> {
    let mut dataset = ReferenceDataset::new(organization)?;
    let reply = match organization {
        Organization::KeySequenced => {
            for record in [b"BB22".to_vec(), b"AA11".to_vec()] {
                if dataset.insert(record).condition != Condition::Normal {
                    return Err("reference KSDS insert failed".into());
                }
            }
            let generic = dataset.read_generic(b"A");
            if dataset.read_key(b"AA").records != [b"AA11".to_vec()]
                || generic.records != [b"AA11".to_vec()]
                || dataset.sequential(false).records != [b"AA11".to_vec(), b"BB22".to_vec()]
            {
                return Err("reference KSDS ordering/access mismatch".into());
            }
            generic
        }
        Organization::EntrySequenced => {
            for record in [b"AA".to_vec(), b"BBB".to_vec()] {
                if dataset.insert(record).condition != Condition::Normal {
                    return Err("reference ESDS append failed".into());
                }
            }
            if dataset.read_rba(2, 8).records != [b"BBB".to_vec()]
                || dataset.read_rba(1, 8).condition != Condition::NotFound
            {
                return Err("reference ESDS RBA mismatch".into());
            }
            dataset.sequential(false)
        }
        Organization::Relative => {
            dataset.definition.record_length = 4;
            if dataset.write_relative(2, b"R002".to_vec()).condition != Condition::Normal
                || dataset.read_relative(2).records != [b"R002".to_vec()]
            {
                return Err("reference RRDS mismatch".into());
            }
            dataset.read_relative(2)
        }
        Organization::VariableRelative => {
            dataset.definition.record_length = 8;
            if dataset.write_relative(4, b"VR4".to_vec()).condition != Condition::Normal
                || dataset.write_relative(5, vec![b'X'; 9]).condition != Condition::Length
            {
                return Err("reference VRRDS mismatch".into());
            }
            dataset.read_relative(4)
        }
        Organization::Linear => {
            if dataset.write_linear(0, b"HELLO").condition != Condition::Normal
                || dataset.write_linear(8, b"X").condition != Condition::NotFound
                || dataset.read_rba(1, 3).records != [b"ELL".to_vec()]
            {
                return Err("reference LDS mismatch".into());
            }
            dataset.read_rba(0, 5)
        }
    };
    Ok(format!(
        "ORG={organization:?}:{}:{};",
        reply.condition.tag(),
        digest_records(&reply.records),
    ))
}

fn run_properties() -> Result<Vec<String>, String> {
    let mut observations = Vec::new();

    let mut lifecycle = ReferenceState::new();
    if lifecycle.define("USER.LIFE", Organization::EntrySequenced) != Condition::Normal
        || lifecycle
            .datasets
            .get_mut("USER.LIFE")
            .ok_or_else(|| "lifecycle dataset is missing".to_string())?
            .insert(b"DATA".to_vec())
            .condition
            != Condition::Normal
        || lifecycle.delete("USER.LIFE") != Condition::Normal
        || lifecycle.datasets.contains_key("USER.LIFE")
    {
        return Err("define/use/delete property failed".into());
    }
    observations.push("P=define-use-delete;".into());

    let mut repro = seeded_state()?;
    let _ = repro.define("USER.COPY", Organization::KeySequenced);
    if repro.repro("USER.BASE", "USER.COPY") != Condition::Normal
        || repro.datasets["USER.BASE"].records != repro.datasets["USER.COPY"].records
    {
        return Err("REPRO property failed".into());
    }
    observations.push("P=repro-exact;".into());

    let before_snapshot = repro.datasets["USER.BASE"].clone();
    if repro.export("USER.BASE") != Condition::Normal
        || repro.import("USER.BASE", "USER.RESTO") != Condition::Normal
        || repro.datasets["USER.RESTO"].records != before_snapshot.records
        || repro.datasets["USER.RESTO"].organization != before_snapshot.organization
    {
        return Err("snapshot round-trip property failed".into());
    }
    observations.push("P=snapshot-round-trip;".into());

    let mut aix = seeded_state()?;
    if aix.define_index("USER.AIX", "USER.BASE", 2, 2, true) != Condition::Normal
        || aix.insert_base("USER.BASE", b"CC33".to_vec()) != Condition::Normal
        || aix.validate().is_err()
        || !aix.indexes["USER.AIX"]
            .identities
            .contains_key(b"33".as_slice())
    {
        return Err("base/AIX property failed".into());
    }
    observations.push("P=base-aix-consistent;".into());

    let mut gdg = ReferenceState::new();
    let _ = gdg.define_group("USER.GDG", 2, true);
    for generation in 1..=3u8 {
        if gdg.create_generation("USER.GDG", vec![vec![generation]]) != Condition::Normal {
            return Err("GDG creation property failed".into());
        }
    }
    let group = &gdg.groups["USER.GDG"];
    if group.next != 4 || group.active.len() != 2 || gdg.datasets.len() != 2 {
        return Err("GDG monotonic retention property failed".into());
    }
    observations.push("P=gdg-monotonic-retention;".into());

    let mut aliases = seeded_state()?;
    let _ = aliases.define("USER.OTHER", Organization::KeySequenced);
    if aliases.define_alias("USER.ALIAS", "USER.BASE") != Condition::Normal
        || aliases.retarget_alias("USER.ALIAS", "USER.OTHER") != Condition::Normal
        || aliases.resolve("USER.ALIAS").as_deref() != Some("USER.OTHER")
    {
        return Err("alias retarget property failed".into());
    }
    observations.push("P=alias-retarget;".into());

    let mut atomic = seeded_state()?;
    let before = atomic.canonical();
    let failure = atomic.atomic(true, |state| {
        state.define("USER.PARTIAL", Organization::EntrySequenced)
    });
    if failure != Condition::InjectedFailure || atomic.canonical() != before {
        return Err("failure atomicity property failed".into());
    }
    observations.push("P=failure-atomic;".into());

    let restart = seeded_state()?;
    let checkpoint = restart.checkpoint();
    let mut corrupted = checkpoint.clone();
    corrupted.digest.push_str("-corrupt");
    if ReferenceState::restore(corrupted).is_ok() {
        return Err("corrupted restart checkpoint was accepted".into());
    }
    let restored = ReferenceState::restore(checkpoint)?;
    if restored.canonical() != restart.canonical() || restored.validate().is_err() {
        return Err("restart equivalence property failed".into());
    }
    observations.push("P=restart-equivalent;".into());

    let mut backup = seeded_state()?;
    let before = backup.datasets["USER.BASE"].clone();
    let _ = backup.export("USER.BASE");
    let _ = backup.delete("USER.BASE");
    if backup.import("USER.BASE", "USER.BASE") != Condition::Normal
        || backup.datasets["USER.BASE"].records != before.records
    {
        return Err("backup/restore equivalence property failed".into());
    }
    observations.push("P=backup-restore-equivalent;".into());

    let mut registers = Registers {
        maxcc: 0,
        lastcc: 0,
    };
    registers.apply(Condition::NotFound);
    registers.set_last(0);
    registers.set_max(4);
    registers.apply(Condition::Capability);
    if registers.lastcc != 12 || registers.maxcc != 12 {
        return Err("MAXCC/LASTCC property failed".into());
    }
    observations.push("P=maxcc-lastcc;".into());

    let mut metadata = ReferenceDataset::new(Organization::KeySequenced)?;
    metadata.definition.block_size = 4_096;
    metadata.definition.buffer_count = 8;
    metadata.definition.primary = 2;
    metadata.definition.secondary = 1;
    metadata.definition.data_class = "BULK".into();
    metadata.definition.management_class = "DAILY".into();
    metadata.definition.storage_class = "FAST".into();
    metadata.definition.volumes = vec!["VOLA".into(), "VOLB".into()];
    metadata.definition.validate()?;
    let mut canonical = String::new();
    metadata.definition.canonical(&mut canonical);
    for expected in ["BULK", "DAILY", "FAST", "VOLA,VOLB", "S=2,1"] {
        if !canonical.contains(expected) {
            return Err("metadata observability property failed".into());
        }
    }
    observations.push("P=metadata-observable;".into());

    for (option, capability) in OPTION_CAPABILITIES {
        if option.is_empty() || capability.is_empty() || Condition::Capability.cc() != 12 {
            return Err("capability/unknown boundary property failed".into());
        }
    }
    observations.push("P=physical-installation-unknown-boundaries;".into());

    let records = [b"CC33".to_vec(), b"AA11".to_vec(), b"BB22".to_vec()];
    let mut ordering_digest = None;
    for order in [
        [0usize, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let mut dataset = ReferenceDataset::new(Organization::KeySequenced)?;
        for position in order {
            if dataset.insert(records[position].clone()).condition != Condition::Normal {
                return Err("bounded ordering permutation insert failed".into());
            }
        }
        let selected = dataset.sequential(false).records;
        if selected != [b"AA11".to_vec(), b"BB22".to_vec(), b"CC33".to_vec()] {
            return Err("bounded ordering permutation changed KSDS order".into());
        }
        let digest = digest_records(&selected);
        if ordering_digest
            .as_ref()
            .is_some_and(|expected| expected != &digest)
        {
            return Err("bounded ordering permutation changed KSDS identity".into());
        }
        ordering_digest = Some(digest);
    }
    observations.push("P=bounded-ordering-permutations;".into());
    Ok(observations)
}

fn reject_observation_perturbations() -> Result<usize, String> {
    let state = seeded_state()?;
    let expected = ExpectedObservation {
        source: ExpectationSource::IndependentReference,
        condition: Condition::NotFound,
        maxcc: 8,
        lastcc: 8,
        state_digest: digest_text(&state.canonical()),
        records: vec![b"AA11".to_vec(), b"BB22".to_vec()],
    };
    let perturbations = [
        (
            "generic-success",
            ExpectedObservation {
                condition: Condition::Normal,
                maxcc: 0,
                lastcc: 0,
                records: Vec::new(),
                ..expected.clone()
            },
        ),
        (
            "ignored-attributes",
            ExpectedObservation {
                state_digest: digest_text("ignored-attributes"),
                ..expected.clone()
            },
        ),
        (
            "wrong-organization-access",
            ExpectedObservation {
                condition: Condition::Normal,
                ..expected.clone()
            },
        ),
        (
            "reordered-record-outcomes",
            ExpectedObservation {
                records: vec![b"BB22".to_vec(), b"AA11".to_vec()],
                ..expected.clone()
            },
        ),
        (
            "missing-maxcc-lastcc",
            ExpectedObservation {
                maxcc: 0,
                lastcc: 0,
                ..expected.clone()
            },
        ),
        (
            "base-aix-drift",
            ExpectedObservation {
                state_digest: digest_text("base-aix-drift"),
                ..expected.clone()
            },
        ),
        (
            "partial-failure-publication",
            ExpectedObservation {
                state_digest: digest_text("partial-failure-publication"),
                ..expected.clone()
            },
        ),
        (
            "production-observation-reuse",
            ExpectedObservation {
                source: ExpectationSource::ProductionObservation,
                ..expected.clone()
            },
        ),
    ];
    if perturbations
        .iter()
        .map(|(name, _)| *name)
        .collect::<BTreeSet<_>>()
        != BTreeSet::from([
            "base-aix-drift",
            "generic-success",
            "ignored-attributes",
            "missing-maxcc-lastcc",
            "partial-failure-publication",
            "production-observation-reuse",
            "reordered-record-outcomes",
            "wrong-organization-access",
        ])
    {
        return Err("reference observation-perturbation identities drifted".into());
    }
    let rejected = perturbations
        .iter()
        .filter(|(_, perturbation)| !verify_observation(&expected, perturbation))
        .count();
    Ok(rejected)
}

fn parse_json(source: &str, label: &str) -> Result<Value, String> {
    serde_json::from_str(source).map_err(|error| format!("{label}: {error}"))
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_BYTES
        && name.split('.').all(|qualifier| {
            !qualifier.is_empty()
                && qualifier.len() <= 8
                && qualifier.bytes().all(|byte| {
                    byte.is_ascii_uppercase()
                        || byte.is_ascii_digit()
                        || matches!(byte, b'@' | b'#' | b'$')
                })
        })
}

fn push_bytes(output: &mut String, bytes: &[u8]) {
    output.push_str(&format!("{}:", bytes.len()));
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output.push(';');
}

fn digest_records(records: &[Vec<u8>]) -> String {
    let mut digest = Sha256::new();
    for record in records {
        digest.update((record.len() as u64).to_be_bytes());
        digest.update(record);
    }
    format!("sha256:{:x}", digest.finalize())
}

fn digest_text(text: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(text.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_simulation_covers_five_organizations_and_all_31_commands() {
        let report = run_dataset_reference_simulation().unwrap();
        assert_eq!(report.official_rows, 36);
        assert_eq!(report.organization_rows, 5);
        assert_eq!(report.command_rows, 31);
        assert_eq!(report.property_cases, 13);
        assert_eq!(report.observation_perturbations_rejected, 8);
        assert_eq!(report.differential_credit, 0);
        assert!(report.digest.starts_with("sha256:"));
    }

    #[test]
    fn reference_capability_boundaries_never_become_modeled_success() {
        for spec in COMMANDS.iter().filter(|spec| spec.capability.is_some()) {
            let observation = exercise_command(*spec).unwrap();
            assert_eq!(observation.condition, Condition::Capability);
            assert_eq!(observation.lastcc, 12);
            assert_eq!(observation.maxcc, 12);
        }
    }

    #[test]
    fn reference_properties_and_perturbations_are_exact_and_repeatable() {
        assert_eq!(run_properties().unwrap().len(), 13);
        assert_eq!(reject_observation_perturbations().unwrap(), 8);
        assert_eq!(
            run_dataset_reference_simulation().unwrap(),
            run_dataset_reference_simulation().unwrap()
        );
    }
}
