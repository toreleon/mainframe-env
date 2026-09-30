//! Stable name, existence and content reads over one owner namespace.

use super::scope::{self, ContainerSelector, OwnerIdentity, ScopeError};
use super::state::{self, ContainerOwner, ContainerValue};
use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::service::handlers) enum ContainerReadError {
    Unauthorized,
    StaleEpoch,
    Bounds,
    NotFound,
    Changed,
    Backend(HostProblem),
}

impl From<ScopeError> for ContainerReadError {
    fn from(error: ScopeError) -> Self {
        match error {
            ScopeError::Unauthorized => Self::Unauthorized,
            ScopeError::StaleEpoch => Self::StaleEpoch,
            ScopeError::Bounds => Self::Bounds,
            ScopeError::Backend(problem) => Self::Backend(problem),
        }
    }
}

#[derive(Clone, Copy)]
pub(in crate::service::handlers) enum ReadRequest<'a> {
    Names { max: usize },
    Exists(&'a str),
    Value(&'a str),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::service::handlers) enum ReadReply {
    Names(Vec<String>),
    Exists(bool),
    Value(Option<ContainerValue>),
}

pub(super) struct ReadPort<'a> {
    store: &'a dyn ProviderStateStore,
    identity: OwnerIdentity<'a>,
    authorize: &'a mut dyn FnMut(&str, &str) -> Result<(), HostProblem>,
}

impl<'a> ReadPort<'a> {
    pub fn new(
        store: &'a dyn ProviderStateStore,
        identity: OwnerIdentity<'a>,
        authorize: &'a mut dyn FnMut(&str, &str) -> Result<(), HostProblem>,
    ) -> Self {
        Self {
            store,
            identity,
            authorize,
        }
    }

    pub fn read(
        &mut self,
        selector: ContainerSelector<'_>,
        request: ReadRequest<'_>,
    ) -> Result<ReadReply, ContainerReadError> {
        match request {
            ReadRequest::Names { max } if max == 0 || max > state::MAX_CONTAINERS => {
                return Err(ContainerReadError::Bounds);
            }
            ReadRequest::Exists(name) | ReadRequest::Value(name)
                if !state::valid_name(name, 16) =>
            {
                return Err(ContainerReadError::Bounds);
            }
            _ => {}
        }
        let before = scope::resolve(self.store, self.identity, selector)?;
        (self.authorize)(before.class, &before.resource).map_err(|problem| match problem {
            HostProblem::Unauthorized => ContainerReadError::Unauthorized,
            other => ContainerReadError::Backend(other),
        })?;
        let channel_before = if matches!(&before.owner, ContainerOwner::Channel { .. }) {
            let key = state::channel_key(&before.owner).map_err(ContainerReadError::Backend)?;
            let row = self
                .store
                .get_provider_state("cics-channel-v1", &key)
                .map_err(|_| ContainerReadError::Backend(HostProblem::InfrastructureFailure))?
                .ok_or(ContainerReadError::NotFound)?;
            state::decode_channel(&row, &before.owner).map_err(ContainerReadError::Backend)?;
            Some(row)
        } else {
            None
        };
        let reply = match request {
            ReadRequest::Names { max } => ReadReply::Names(self.names(&before.owner, max)?),
            ReadRequest::Exists(name) => {
                ReadReply::Exists(self.value(&before.owner, name)?.is_some())
            }
            ReadRequest::Value(name) => ReadReply::Value(self.value(&before.owner, name)?),
        };
        let after = scope::resolve(self.store, self.identity, selector)?;
        if before != after {
            return Err(ContainerReadError::StaleEpoch);
        }
        if matches!(&before.owner, ContainerOwner::Channel { .. }) {
            let key = state::channel_key(&before.owner).map_err(ContainerReadError::Backend)?;
            let row = self
                .store
                .get_provider_state("cics-channel-v1", &key)
                .map_err(|_| ContainerReadError::Backend(HostProblem::InfrastructureFailure))?
                .ok_or(ContainerReadError::Changed)?;
            state::decode_channel(&row, &before.owner).map_err(ContainerReadError::Backend)?;
            if channel_before.as_ref() != Some(&row) {
                return Err(ContainerReadError::Changed);
            }
        }
        Ok(reply)
    }

    fn names(&self, owner: &ContainerOwner, max: usize) -> Result<Vec<String>, ContainerReadError> {
        let namespace = state::container_namespace(owner).map_err(ContainerReadError::Backend)?;
        let rows = self
            .store
            .list_provider_state(&namespace, max + 1)
            .map_err(|_| ContainerReadError::Backend(HostProblem::InfrastructureFailure))?;
        if rows.len() > max {
            return Err(ContainerReadError::Bounds);
        }
        let again = self
            .store
            .list_provider_state(&namespace, max + 1)
            .map_err(|_| ContainerReadError::Backend(HostProblem::InfrastructureFailure))?;
        if rows != again {
            return Err(ContainerReadError::Changed);
        }
        rows.iter()
            .map(|row| {
                state::decode_container(row, owner).map_err(ContainerReadError::Backend)?;
                Ok(row.key.clone())
            })
            .collect()
    }

    fn value(
        &self,
        owner: &ContainerOwner,
        name: &str,
    ) -> Result<Option<ContainerValue>, ContainerReadError> {
        let namespace = state::container_namespace(owner).map_err(ContainerReadError::Backend)?;
        let first: Option<ProviderStateRecord> = self
            .store
            .get_provider_state(&namespace, name)
            .map_err(|_| ContainerReadError::Backend(HostProblem::InfrastructureFailure))?;
        let second = self
            .store
            .get_provider_state(&namespace, name)
            .map_err(|_| ContainerReadError::Backend(HostProblem::InfrastructureFailure))?;
        if first != second {
            return Err(ContainerReadError::Changed);
        }
        first
            .map(|row| state::decode_container(&row, owner).map_err(ContainerReadError::Backend))
            .transpose()
    }
}
