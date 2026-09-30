//! Atomic task-channel mutations over the A1 owner-scoped read authority.

use super::{
    browse_read::{ReadPort, ReadReply, ReadRequest},
    scope::{self, ContainerSelector, OwnerIdentity},
    state::{self, ContainerDatatype, ContainerOwner, ContainerValue},
};
use mainframe_env_host_api::{AccessIntent, HostProblem};
use mainframe_env_store_api::{
    ProviderStateMutation, ProviderStateRecord, ProviderStateStore, ProviderStateWrite, StoreError,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const CAPACITY_NAMESPACE: &str = "cics-container-capacity-v1";
const REPLAY_NAMESPACE: &str = "cics-container-replay-v1";
const MAX_ATTEMPTS: usize = 8;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Capacity {
    schema_version: u8,
    channels: usize,
    containers: usize,
    replays: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Replay {
    schema_version: u8,
    owner_execution: String,
    owner_principal: String,
    owner_run_unit: String,
    digest: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind")]
enum Action<'a> {
    Put {
        channel: &'a str,
        name: &'a str,
        bytes: &'a [u8],
        datatype: Option<ContainerDatatype>,
        ccsid: Option<u16>,
        append: bool,
    },
    DeleteContainer {
        channel: &'a str,
        name: &'a str,
    },
    Move {
        channel: &'a str,
        name: &'a str,
        to_channel: &'a str,
        as_name: &'a str,
    },
    DeleteChannel {
        channel: &'a str,
    },
}

pub(super) struct ChannelPort<'a> {
    store: &'a dyn ProviderStateStore,
    identity: OwnerIdentity<'a>,
    authorize: &'a mut dyn FnMut(&str, &str, AccessIntent) -> Result<(), HostProblem>,
    creator_program: Option<&'a str>,
}

impl<'a> ChannelPort<'a> {
    #[cfg(test)]
    pub fn new(
        store: &'a dyn ProviderStateStore,
        identity: OwnerIdentity<'a>,
        authorize: &'a mut dyn FnMut(&str, &str, AccessIntent) -> Result<(), HostProblem>,
    ) -> Self {
        Self {
            store,
            identity,
            authorize,
            creator_program: None,
        }
    }

    pub fn new_with_program(
        store: &'a dyn ProviderStateStore,
        identity: OwnerIdentity<'a>,
        creator_program: &'a str,
        authorize: &'a mut dyn FnMut(&str, &str, AccessIntent) -> Result<(), HostProblem>,
    ) -> Self {
        Self {
            store,
            identity,
            authorize,
            creator_program: Some(creator_program),
        }
    }

    pub fn count(&mut self, channel: &str) -> Result<usize, HostProblem> {
        let mut authorize =
            |class: &str, resource: &str| (self.authorize)(class, resource, AccessIntent::Read);
        let mut reader = ReadPort::new(self.store, self.identity, &mut authorize);
        match reader.read(
            ContainerSelector::Channel(channel),
            ReadRequest::Names {
                max: state::MAX_CONTAINERS,
            },
        ) {
            Ok(ReadReply::Names(names)) => Ok(names.len()),
            Ok(_) => Err(HostProblem::InfrastructureFailure),
            Err(error) => Err(read_problem(error)),
        }
    }

    #[cfg(test)]
    pub fn get(&mut self, channel: &str, name: &str) -> Result<Vec<u8>, HostProblem> {
        Ok(self.get_value(channel, name)?.bytes)
    }

    pub fn get_value(&mut self, channel: &str, name: &str) -> Result<ContainerValue, HostProblem> {
        let mut authorize =
            |class: &str, resource: &str| (self.authorize)(class, resource, AccessIntent::Read);
        let mut reader = ReadPort::new(self.store, self.identity, &mut authorize);
        match reader.read(
            ContainerSelector::Channel(channel),
            ReadRequest::Value(name),
        ) {
            Ok(ReadReply::Value(Some(value))) => Ok(value),
            Ok(ReadReply::Value(None)) => Err(HostProblem::NotFound),
            Ok(_) => Err(HostProblem::InfrastructureFailure),
            Err(error) => Err(read_problem(error)),
        }
    }

    #[cfg(test)]
    pub fn put(
        &mut self,
        channel: &str,
        name: &str,
        bytes: &[u8],
        append: bool,
        replay: &str,
    ) -> Result<(), HostProblem> {
        self.put_value(channel, name, bytes, None, None, append, replay)
    }

    pub fn put_value(
        &mut self,
        channel: &str,
        name: &str,
        bytes: &[u8],
        datatype: Option<ContainerDatatype>,
        ccsid: Option<u16>,
        append: bool,
        replay: &str,
    ) -> Result<(), HostProblem> {
        self.mutate(
            Action::Put {
                channel,
                name,
                bytes,
                datatype,
                ccsid,
                append,
            },
            replay,
        )
    }

    pub fn delete_container(
        &mut self,
        channel: &str,
        name: &str,
        replay: &str,
    ) -> Result<(), HostProblem> {
        self.mutate(Action::DeleteContainer { channel, name }, replay)
    }

    pub fn move_to(
        &mut self,
        channel: &str,
        name: &str,
        to_channel: &str,
        as_name: &str,
        replay: &str,
    ) -> Result<(), HostProblem> {
        self.mutate(
            Action::Move {
                channel,
                name,
                to_channel,
                as_name,
            },
            replay,
        )
    }

    pub fn delete_channel(&mut self, channel: &str, replay: &str) -> Result<(), HostProblem> {
        self.mutate(Action::DeleteChannel { channel }, replay)
    }

    fn mutate(&mut self, action: Action<'_>, replay_id: &str) -> Result<(), HostProblem> {
        if replay_id.is_empty() || replay_id.len() > 256 {
            return Err(HostProblem::MissingIdempotency);
        }
        let (channel, name, to_channel, as_name) = match &action {
            Action::Put { channel, name, .. } | Action::DeleteContainer { channel, name } => {
                (*channel, Some(*name), None, None)
            }
            Action::Move {
                channel,
                name,
                to_channel,
                as_name,
            } => (*channel, Some(*name), Some(*to_channel), Some(*as_name)),
            Action::DeleteChannel { channel } => (*channel, None, None, None),
        };
        if !state::valid_name(channel, 16)
            || name.is_some_and(|name| !state::valid_name(name, 16))
            || to_channel.is_some_and(|name| !state::valid_name(name, 16))
            || as_name.is_some_and(|name| !state::valid_name(name, 16))
        {
            return Err(HostProblem::Malformed);
        }
        let source = scope::resolve(
            self.store,
            self.identity,
            ContainerSelector::Channel(channel),
        )
        .map_err(scope_problem)?;
        let intent = if matches!(action, Action::DeleteChannel { .. }) {
            AccessIntent::Alter
        } else {
            AccessIntent::Update
        };
        (self.authorize)(source.class, &source.resource, intent)?;
        let target = if let Some(to_channel) = to_channel {
            let target = scope::resolve(
                self.store,
                self.identity,
                ContainerSelector::Channel(to_channel),
            )
            .map_err(scope_problem)?;
            (self.authorize)(target.class, &target.resource, AccessIntent::Update)?;
            Some(target.owner)
        } else {
            None
        };
        let digest = hex(&Sha256::digest(
            serde_json::to_vec(&action).map_err(|_| HostProblem::InfrastructureFailure)?,
        ));
        let replay_key = replay_id.to_owned();
        for _ in 0..MAX_ATTEMPTS {
            if let Some(row) = self
                .store
                .get_provider_state(REPLAY_NAMESPACE, &replay_key)
                .map_err(store_problem)?
            {
                let receipt: Replay = serde_json::from_slice(&row.payload)
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
                if receipt.schema_version != 1 {
                    return Err(HostProblem::InfrastructureFailure);
                }
                if receipt.owner_execution != self.identity.execution
                    || receipt.owner_principal != self.identity.principal
                    || receipt.owner_run_unit != self.identity.run_unit
                {
                    return Err(HostProblem::IdempotencyConflict);
                }
                return if receipt.digest == digest {
                    Ok(())
                } else {
                    Err(HostProblem::IdempotencyConflict)
                };
            }
            let capacity_old = self
                .store
                .get_provider_state(CAPACITY_NAMESPACE, "global")
                .map_err(store_problem)?;
            let mut capacity = match &capacity_old {
                Some(row) => serde_json::from_slice::<Capacity>(&row.payload)
                    .map_err(|_| HostProblem::InfrastructureFailure)?,
                None => Capacity {
                    schema_version: 1,
                    channels: 0,
                    containers: 0,
                    replays: 0,
                },
            };
            if capacity.schema_version != 1
                || capacity.channels > 4096
                || capacity.containers > 16384
                || capacity.replays >= 16384
            {
                return Err(HostProblem::ResourceExhausted);
            }
            let source_channel = self.load_channel(&source.owner)?;
            let mut writes = Vec::new();
            match &action {
                Action::Put {
                    name,
                    bytes,
                    datatype,
                    ccsid,
                    append,
                    ..
                } => {
                    let old = self.load_container(&source.owner, name)?;
                    if old.is_some() && source_channel.is_none() {
                        return Err(HostProblem::InfrastructureFailure);
                    }
                    if old.as_ref().is_some_and(|(_, value)| value.read_only) {
                        return Err(HostProblem::Malformed);
                    }
                    let mut value = old.as_ref().map_or_else(
                        || ContainerValue {
                            datatype: datatype.unwrap_or(if ccsid.is_some() {
                                ContainerDatatype::Character
                            } else {
                                ContainerDatatype::Bit
                            }),
                            ccsid: ccsid.or(if *datatype == Some(ContainerDatatype::Character) {
                                Some(37)
                            } else {
                                None
                            }),
                            read_only: false,
                            bytes: Vec::new(),
                        },
                        |(_, value)| value.clone(),
                    );
                    if old.is_some() && datatype.is_some_and(|kind| kind != value.datatype) {
                        return Err(HostProblem::Malformed);
                    }
                    if old.is_some() && ccsid.is_some_and(|codepage| Some(codepage) != value.ccsid)
                    {
                        return Err(HostProblem::Unsupported);
                    }
                    if *append {
                        value.bytes.extend_from_slice(bytes);
                    } else {
                        value.bytes = bytes.to_vec();
                    }
                    let mut record = state::container_record(&source.owner, name, &value)?;
                    let expected = old.as_ref().map(|(row, _)| row.version);
                    record.version = next_version(expected)?;
                    if old.is_none() {
                        if self.names(&source.owner)?.len() >= state::MAX_CONTAINERS {
                            return Err(HostProblem::ResourceExhausted);
                        }
                        capacity.containers += 1;
                    }
                    writes.push(ProviderStateMutation::Put(ProviderStateWrite {
                        record,
                        expected_version: expected,
                    }));
                    self.bump_channel(
                        &source.owner,
                        source_channel.as_ref(),
                        &mut writes,
                        &mut capacity,
                    )?;
                }
                Action::DeleteContainer { name, .. } => {
                    let old_channel = source_channel.as_ref().ok_or(HostProblem::NotFound)?;
                    let (row, value) = self
                        .load_container(&source.owner, name)?
                        .ok_or(HostProblem::NotFound)?;
                    if value.read_only {
                        return Err(HostProblem::Malformed);
                    }
                    writes.push(ProviderStateMutation::Delete {
                        namespace: row.namespace,
                        key: row.key,
                        expected_version: row.version,
                    });
                    capacity.containers = capacity
                        .containers
                        .checked_sub(1)
                        .ok_or(HostProblem::InfrastructureFailure)?;
                    self.bump_channel(
                        &source.owner,
                        Some(old_channel),
                        &mut writes,
                        &mut capacity,
                    )?;
                }
                Action::DeleteChannel { .. } => {
                    let old_channel = source_channel.as_ref().ok_or(HostProblem::NotFound)?;
                    if state::channel_creator(old_channel, &source.owner)?
                        != self.creator_program.map(str::to_owned)
                    {
                        return Err(HostProblem::Unauthorized);
                    }
                    for row in self.names(&source.owner)? {
                        self.load_container(&source.owner, &row.key)?
                            .ok_or(HostProblem::UnknownOutcome)?;
                        writes.push(ProviderStateMutation::Delete {
                            namespace: row.namespace,
                            key: row.key,
                            expected_version: row.version,
                        });
                        capacity.containers = capacity
                            .containers
                            .checked_sub(1)
                            .ok_or(HostProblem::InfrastructureFailure)?;
                    }
                    writes.push(ProviderStateMutation::Delete {
                        namespace: old_channel.namespace.clone(),
                        key: old_channel.key.clone(),
                        expected_version: old_channel.version,
                    });
                    capacity.channels = capacity
                        .channels
                        .checked_sub(1)
                        .ok_or(HostProblem::InfrastructureFailure)?;
                }
                Action::Move { name, as_name, .. } => {
                    let target = target.as_ref().ok_or(HostProblem::InfrastructureFailure)?;
                    let old_channel = source_channel.as_ref().ok_or(HostProblem::NotFound)?;
                    let (old_row, value) = self
                        .load_container(&source.owner, name)?
                        .ok_or(HostProblem::NotFound)?;
                    if value.read_only {
                        return Err(HostProblem::Malformed);
                    }
                    if source.owner == *target && *name == *as_name {
                        let mut record = old_row.clone();
                        record.version = next_version(Some(old_row.version))?;
                        writes.push(ProviderStateMutation::Put(ProviderStateWrite {
                            record,
                            expected_version: Some(old_row.version),
                        }));
                        self.bump_channel(
                            &source.owner,
                            Some(old_channel),
                            &mut writes,
                            &mut capacity,
                        )?;
                    } else {
                        let target_old = self.load_container(target, as_name)?;
                        if target_old
                            .as_ref()
                            .is_some_and(|(_, value)| value.read_only)
                        {
                            return Err(HostProblem::Malformed);
                        }
                        if source.owner != *target
                            && target_old.is_none()
                            && self.names(target)?.len() >= state::MAX_CONTAINERS
                        {
                            return Err(HostProblem::ResourceExhausted);
                        }
                        let mut target_row = state::container_record(target, as_name, &value)?;
                        let expected = target_old.as_ref().map(|(row, _)| row.version);
                        target_row.version = next_version(expected)?;
                        writes.push(ProviderStateMutation::Delete {
                            namespace: old_row.namespace,
                            key: old_row.key,
                            expected_version: old_row.version,
                        });
                        writes.push(ProviderStateMutation::Put(ProviderStateWrite {
                            record: target_row,
                            expected_version: expected,
                        }));
                        if target_old.is_some() {
                            capacity.containers = capacity
                                .containers
                                .checked_sub(1)
                                .ok_or(HostProblem::InfrastructureFailure)?;
                        }
                        self.bump_channel(
                            &source.owner,
                            Some(old_channel),
                            &mut writes,
                            &mut capacity,
                        )?;
                        if source.owner != *target {
                            let target_channel = self.load_channel(target)?;
                            self.bump_channel(
                                target,
                                target_channel.as_ref(),
                                &mut writes,
                                &mut capacity,
                            )?;
                        }
                    }
                }
            }
            capacity.replays += 1;
            let capacity_version = capacity_old.as_ref().map(|row| row.version);
            writes.push(ProviderStateMutation::Put(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: CAPACITY_NAMESPACE.into(),
                    key: "global".into(),
                    version: next_version(capacity_version)?,
                    payload: serde_json::to_vec(&capacity)
                        .map_err(|_| HostProblem::InfrastructureFailure)?,
                },
                expected_version: capacity_version,
            }));
            writes.push(ProviderStateMutation::Put(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: REPLAY_NAMESPACE.into(),
                    key: replay_key.clone(),
                    version: 1,
                    payload: serde_json::to_vec(&Replay {
                        schema_version: 1,
                        owner_execution: self.identity.execution.into(),
                        owner_principal: self.identity.principal.into(),
                        owner_run_unit: self.identity.run_unit.into(),
                        digest: digest.clone(),
                    })
                    .map_err(|_| HostProblem::InfrastructureFailure)?,
                },
                expected_version: None,
            }));
            match self.store.mutate_provider_states_atomic(writes) {
                Ok(()) => return Ok(()),
                Err(StoreError::Conflict | StoreError::AlreadyExists) => continue,
                Err(_) => {
                    if let Some(row) = self
                        .store
                        .get_provider_state(REPLAY_NAMESPACE, &replay_key)
                        .map_err(|_| HostProblem::UnknownOutcome)?
                    {
                        let receipt: Replay = serde_json::from_slice(&row.payload)
                            .map_err(|_| HostProblem::UnknownOutcome)?;
                        if receipt.schema_version == 1
                            && receipt.digest == digest
                            && receipt.owner_execution == self.identity.execution
                            && receipt.owner_principal == self.identity.principal
                            && receipt.owner_run_unit == self.identity.run_unit
                        {
                            return Ok(());
                        }
                    }
                    return Err(HostProblem::UnknownOutcome);
                }
            }
        }
        Err(HostProblem::UnknownOutcome)
    }

    fn load_channel(
        &self,
        owner: &ContainerOwner,
    ) -> Result<Option<ProviderStateRecord>, HostProblem> {
        let key = state::channel_key(owner)?;
        let row = self
            .store
            .get_provider_state("cics-channel-v1", &key)
            .map_err(store_problem)?;
        if let Some(row) = &row {
            state::decode_channel(row, owner)?;
        }
        Ok(row)
    }

    fn load_container(
        &self,
        owner: &ContainerOwner,
        name: &str,
    ) -> Result<Option<(ProviderStateRecord, ContainerValue)>, HostProblem> {
        let row = self
            .store
            .get_provider_state(&state::container_namespace(owner)?, name)
            .map_err(store_problem)?;
        row.map(|row| {
            let value = state::decode_container(&row, owner)?;
            Ok((row, value))
        })
        .transpose()
    }

    fn names(&self, owner: &ContainerOwner) -> Result<Vec<ProviderStateRecord>, HostProblem> {
        let rows = self
            .store
            .list_provider_state(
                &state::container_namespace(owner)?,
                state::MAX_CONTAINERS + 1,
            )
            .map_err(store_problem)?;
        if rows.len() > state::MAX_CONTAINERS {
            return Err(HostProblem::ResourceExhausted);
        }
        for row in &rows {
            state::decode_container(row, owner)?;
        }
        Ok(rows)
    }

    fn bump_channel(
        &self,
        owner: &ContainerOwner,
        old: Option<&ProviderStateRecord>,
        writes: &mut Vec<ProviderStateMutation>,
        capacity: &mut Capacity,
    ) -> Result<(), HostProblem> {
        let creator = old
            .map(|row| state::channel_creator(row, owner))
            .transpose()?
            .flatten();
        let mut record = state::channel_record_with_program(
            owner,
            old.map_or(self.creator_program, |_| creator.as_deref()),
        )?;
        let expected = old.map(|row| row.version);
        record.version = next_version(expected)?;
        if old.is_none() {
            if capacity.channels >= 4096 {
                return Err(HostProblem::ResourceExhausted);
            }
            capacity.channels += 1;
        }
        writes.push(ProviderStateMutation::Put(ProviderStateWrite {
            record,
            expected_version: expected,
        }));
        Ok(())
    }
}

fn next_version(old: Option<u64>) -> Result<u64, HostProblem> {
    old.map_or(Some(1), |version| version.checked_add(1))
        .ok_or(HostProblem::ResourceExhausted)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn store_problem(_: StoreError) -> HostProblem {
    HostProblem::InfrastructureFailure
}
fn scope_problem(error: scope::ScopeError) -> HostProblem {
    match error {
        scope::ScopeError::Unauthorized => HostProblem::Unauthorized,
        scope::ScopeError::Bounds => HostProblem::Malformed,
        scope::ScopeError::StaleEpoch => HostProblem::NotFound,
        scope::ScopeError::Backend(problem) => problem,
    }
}
fn read_problem(error: super::ContainerReadError) -> HostProblem {
    match error {
        super::ContainerReadError::Unauthorized => HostProblem::Unauthorized,
        super::ContainerReadError::Bounds => HostProblem::Malformed,
        super::ContainerReadError::NotFound | super::ContainerReadError::StaleEpoch => {
            HostProblem::NotFound
        }
        super::ContainerReadError::Changed => HostProblem::UnknownOutcome,
        super::ContainerReadError::Backend(problem) => problem,
    }
}
