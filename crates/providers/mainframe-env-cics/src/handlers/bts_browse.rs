//! Bounded task-owned BTS browse cursor state.
//!
//! The caller supplies a stable, ordered snapshot of names from the shared BTS
//! lifecycle authority. Each advance checks the live lifecycle epoch before
//! changing position. The store adapter persists the whole book by CAS.

use super::{CicsService, Run};
use mainframe_env_host_api::HostProblem;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod store;
pub use store::{BrowseEffect, BrowseOutcome, BrowseOwner, BtsBrowseStore};
mod query;
pub use query::{activity_children_snapshot, activity_flat_snapshot, process_snapshot};
mod route;
pub(in crate::service) use route::invoke;

pub(super) fn release_task(service: &CicsService, run: &Run) -> Result<(), HostProblem> {
    BtsBrowseStore::new(service.store.as_ref()).clear_existing(&owner(run)?, true)
}

pub(super) fn rollback_task(service: &CicsService, run: &Run) -> Result<(), HostProblem> {
    BtsBrowseStore::new(service.store.as_ref()).clear_existing(&owner(run)?, false)
}

fn owner(run: &Run) -> Result<BrowseOwner, HostProblem> {
    BrowseOwner::new(
        run.invocation.run_unit_id.as_str(),
        run.invocation.execution_id.as_str(),
        run.invocation.principal.id().as_str(),
    )
}

pub const MAX_CURSORS: usize = 32;
pub const MAX_CURSOR_ITEMS: usize = 256;

fn token_error() -> HostProblem {
    HostProblem::Condition {
        name: "TOKENERR".into(),
        response: 112,
        response2: 3,
    }
}

fn wrong_kind() -> HostProblem {
    HostProblem::Condition {
        name: "ILLOGIC".into(),
        response: 21,
        response2: 1,
    }
}

fn end_of_browse() -> HostProblem {
    HostProblem::Condition {
        name: "END".into(),
        response: 83,
        response2: 2,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum BrowseKind {
    Activity,
    Container,
    Event,
    Process,
    Timer,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowseScope {
    pub kind: BrowseKind,
    pub resource_class: String,
    pub resource_name: String,
    pub epoch: u64,
    pub process_type: Option<String>,
    pub process_name: Option<String>,
    #[serde(default)]
    pub activity_id: Option<String>,
}

impl BrowseScope {
    pub fn new(
        kind: BrowseKind,
        resource_class: &str,
        resource_name: &str,
        epoch: u64,
    ) -> Result<Self, HostProblem> {
        if resource_class.is_empty()
            || resource_class.len() > 64
            || resource_name.is_empty()
            || resource_name.len() > 128
            || epoch == 0
        {
            return Err(HostProblem::Malformed);
        }
        Ok(Self {
            kind,
            resource_class: resource_class.into(),
            resource_name: resource_name.into(),
            epoch,
            process_type: None,
            process_name: None,
            activity_id: None,
        })
    }

    pub fn with_process_type(mut self, process_type: &str) -> Result<Self, HostProblem> {
        super::bts_lifecycle::validate_name(process_type, 8, true)?;
        self.process_type = Some(process_type.into());
        Ok(self)
    }

    pub fn with_process(
        mut self,
        process_type: &str,
        process_name: &str,
    ) -> Result<Self, HostProblem> {
        self = self.with_process_type(process_type)?;
        super::bts_lifecycle::validate_name(process_name, 36, true)?;
        self.process_name = Some(process_name.into());
        Ok(self)
    }

    pub fn with_activity(mut self, activity_id: &str) -> Result<Self, HostProblem> {
        super::bts_lifecycle::validate_activity_id(activity_id)?;
        self.activity_id = Some(activity_id.into());
        Ok(self)
    }

    fn validate(&self) -> Result<(), HostProblem> {
        Self::new(
            self.kind,
            &self.resource_class,
            &self.resource_name,
            self.epoch,
        )?;
        if self
            .process_type
            .as_deref()
            .is_some_and(|name| super::bts_lifecycle::validate_name(name, 8, true).is_err())
            || self
                .process_name
                .as_deref()
                .is_some_and(|name| super::bts_lifecycle::validate_name(name, 36, true).is_err())
            || self.process_name.is_some() && self.process_type.is_none()
            || self
                .activity_id
                .as_deref()
                .is_some_and(|id| super::bts_lifecycle::validate_activity_id(id).is_err())
        {
            return Err(HostProblem::Malformed);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowseItem {
    pub name: String,
    pub activity_id: Option<String>,
    pub level: u16,
    pub resource_epoch: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_metadata: Option<BrowseEventMetadata>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowseEventMetadata {
    pub event_type: i32,
    pub fire_status: i32,
    pub composite: Option<String>,
    pub predicate: Option<i32>,
    pub timer: Option<String>,
}

impl BrowseItem {
    pub fn container(name: &str) -> Result<Self, HostProblem> {
        if !super::bts_container::valid_task_channel_name(name) {
            return Err(HostProblem::Malformed);
        }
        Ok(Self {
            name: name.into(),
            activity_id: None,
            level: 0,
            resource_epoch: 0,
            event_metadata: None,
        })
    }

    fn valid_for(&self, kind: BrowseKind) -> bool {
        if kind == BrowseKind::Container {
            self.activity_id.is_none()
                && self.event_metadata.is_none()
                && self.level == 0
                && Self::container(&self.name).is_ok()
        } else {
            Self::new(&self.name, self.activity_id.as_deref(), self.level).is_ok()
                && (self.activity_id.is_some()
                    == matches!(kind, BrowseKind::Activity | BrowseKind::Process))
                && (self.event_metadata.is_none() || kind == BrowseKind::Event)
        }
    }

    pub fn new(name: &str, activity_id: Option<&str>, level: u16) -> Result<Self, HostProblem> {
        if super::bts_lifecycle::validate_name(name, 36, true).is_err()
            || activity_id.is_some_and(|id| super::bts_lifecycle::validate_activity_id(id).is_err())
            || usize::from(level) >= MAX_CURSOR_ITEMS
        {
            return Err(HostProblem::Malformed);
        }
        Ok(Self {
            name: name.into(),
            activity_id: activity_id.map(str::to_owned),
            level,
            resource_epoch: 0,
            event_metadata: None,
        })
    }

    pub fn with_epoch(mut self, epoch: u64) -> Result<Self, HostProblem> {
        if epoch == 0 {
            return Err(HostProblem::Malformed);
        }
        self.resource_epoch = epoch;
        Ok(self)
    }

    pub fn with_event_metadata(mut self, metadata: BrowseEventMetadata) -> Self {
        self.event_metadata = Some(metadata);
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BrowseCursor {
    scope: BrowseScope,
    items: Vec<BrowseItem>,
    position: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowseBook {
    next_token: u32,
    cursors: BTreeMap<u32, BrowseCursor>,
}

impl Default for BrowseBook {
    fn default() -> Self {
        Self {
            next_token: 1,
            cursors: BTreeMap::new(),
        }
    }
}

impl BrowseBook {
    pub fn start(
        &mut self,
        scope: BrowseScope,
        items: Vec<BrowseItem>,
    ) -> Result<u32, HostProblem> {
        if self.cursors.len() >= MAX_CURSORS || items.len() > MAX_CURSOR_ITEMS {
            return Err(HostProblem::ResourceExhausted);
        }
        if items
            .iter()
            .any(|item| !item.valid_for(scope.kind) || item.resource_epoch == 0)
        {
            return Err(HostProblem::Malformed);
        }
        scope.validate()?;
        let token = self.next_token;
        let next_token = token.checked_add(1).ok_or(HostProblem::ResourceExhausted)?;
        self.cursors.insert(
            token,
            BrowseCursor {
                scope,
                items,
                position: 0,
            },
        );
        self.next_token = next_token;
        Ok(token)
    }

    pub fn peek(&self, token: u32, kind: BrowseKind) -> Result<BrowseItem, HostProblem> {
        let cursor = self.cursors.get(&token).ok_or_else(token_error)?;
        if cursor.scope.kind != kind {
            return Err(wrong_kind());
        }
        cursor
            .items
            .get(cursor.position)
            .cloned()
            .ok_or_else(end_of_browse)
    }

    pub fn remaining(&self, token: u32, kind: BrowseKind) -> Result<Vec<BrowseItem>, HostProblem> {
        let cursor = self.cursors.get(&token).ok_or_else(token_error)?;
        if cursor.scope.kind != kind {
            return Err(wrong_kind());
        }
        Ok(cursor.items[cursor.position..].to_vec())
    }

    pub fn scope(&self, token: u32, kind: BrowseKind) -> Result<BrowseScope, HostProblem> {
        let cursor = self.cursors.get(&token).ok_or_else(token_error)?;
        if cursor.scope.kind != kind {
            return Err(wrong_kind());
        }
        Ok(cursor.scope.clone())
    }

    pub fn next(
        &mut self,
        token: u32,
        kind: BrowseKind,
        live_epoch: u64,
        expected: &BrowseItem,
    ) -> Result<BrowseItem, HostProblem> {
        self.next_skipping(token, kind, live_epoch, expected, 0)
    }

    pub fn next_skipping(
        &mut self,
        token: u32,
        kind: BrowseKind,
        live_epoch: u64,
        expected: &BrowseItem,
        skipped: usize,
    ) -> Result<BrowseItem, HostProblem> {
        let cursor = self.cursors.get_mut(&token).ok_or_else(token_error)?;
        if cursor.scope.kind != kind {
            return Err(wrong_kind());
        }
        if skipped > MAX_CURSOR_ITEMS || skipped != 0 && kind != BrowseKind::Container {
            return Err(token_error());
        }
        let index = cursor
            .position
            .checked_add(skipped)
            .ok_or_else(token_error)?;
        let item = cursor.items.get(index).ok_or_else(end_of_browse)?;
        let epoch = if matches!(
            kind,
            BrowseKind::Process | BrowseKind::Event | BrowseKind::Timer
        ) {
            item.resource_epoch
        } else {
            cursor.scope.epoch
        };
        if epoch != live_epoch || item != expected {
            return Err(token_error());
        }
        cursor.position = index + 1;
        Ok(item.clone())
    }

    pub fn end(&mut self, token: u32, kind: BrowseKind) -> Result<(), HostProblem> {
        let cursor = self.cursors.get(&token).ok_or_else(token_error)?;
        if cursor.scope.kind != kind {
            return Err(wrong_kind());
        }
        self.cursors.remove(&token);
        Ok(())
    }

    pub fn clear(&mut self) {
        self.cursors.clear();
    }

    pub fn validate(&self) -> Result<(), HostProblem> {
        if self.next_token == 0 || self.cursors.len() > MAX_CURSORS {
            return Err(HostProblem::InfrastructureFailure);
        }
        for (token, cursor) in &self.cursors {
            if *token == 0
                || *token >= self.next_token
                || cursor.items.len() > MAX_CURSOR_ITEMS
                || cursor.position > cursor.items.len()
                || cursor.scope.validate().is_err()
                || cursor
                    .items
                    .iter()
                    .any(|item| !item.valid_for(cursor.scope.kind) || item.resource_epoch == 0)
            {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BrowseBook, BrowseEffect, BrowseItem, BrowseKind, BrowseOutcome, BrowseOwner, BrowseScope,
        BtsBrowseStore, MAX_CURSOR_ITEMS, MAX_CURSORS,
    };
    use super::{
        activity_children_snapshot, activity_flat_snapshot, process_snapshot, token_error,
    };
    use crate::service::handlers::bts_lifecycle::{BtsLifecycleStore, BtsProcess};
    use mainframe_env_host_api::HostProblem;
    use mainframe_env_store::{MemoryStore, SqliteStateStore};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_SQLITE: AtomicU64 = AtomicU64::new(1);

    fn process_item(name: &str) -> BrowseItem {
        BrowseItem::new(name, Some(&"A".repeat(52)), 0)
            .unwrap()
            .with_epoch(1)
            .unwrap()
    }

    #[test]
    fn bts_browse_bounds_and_epoch_fence() {
        let mut book = BrowseBook::default();
        let scope = BrowseScope::new(BrowseKind::Process, "TYPE", "TYPE", 1).unwrap();
        let items = (0..MAX_CURSOR_ITEMS)
            .map(|index| process_item(&format!("P{index:03}")))
            .collect();
        let token = book.start(scope.clone(), items).unwrap();
        let expected = book.peek(token, BrowseKind::Process).unwrap();
        assert!(book.next(token, BrowseKind::Process, 2, &expected).is_err());
        assert_eq!(
            book.next(token, BrowseKind::Process, 1, &expected)
                .unwrap()
                .name,
            "P000"
        );
        let expected = book.peek(token, BrowseKind::Process).unwrap();
        assert_eq!(
            book.next(token, BrowseKind::Process, 1, &expected)
                .unwrap()
                .name,
            "P001"
        );
        assert!(book.next(token, BrowseKind::Process, 1, &expected).is_err());
        for _ in 1..MAX_CURSORS {
            book.start(scope.clone(), vec![process_item("P")]).unwrap();
        }
        assert!(book.start(scope.clone(), vec![process_item("P")]).is_err());
        book.end(token, BrowseKind::Process).unwrap();
        assert!(book.start(scope, vec![process_item("P"); 257]).is_err());
        book.validate().unwrap();
    }

    #[test]
    fn bts_browse_event_timer_cursor_kinds_and_state_epoch() {
        let mut book = BrowseBook::default();
        for kind in [BrowseKind::Event, BrowseKind::Timer] {
            let scope = BrowseScope::new(kind, "BTSEVENT", "CICS.BTS.A.BROWSE", 7).unwrap();
            let item = BrowseItem::new("READY", None, 0)
                .unwrap()
                .with_epoch(3)
                .unwrap();
            let token = book.start(scope, vec![item.clone()]).unwrap();
            assert_eq!(book.peek(token, kind), Ok(item.clone()));
            assert_eq!(book.next(token, kind, 4, &item), Err(token_error()));
            assert_eq!(book.next(token, kind, 3, &item), Ok(item));
            assert!(book.peek(token, kind).is_err());
            book.end(token, kind).unwrap();
        }
    }

    #[test]
    fn bts_browse_container_skips_deleted_snapshot_names_atomically() {
        let mut book = BrowseBook::default();
        let scope = BrowseScope::new(BrowseKind::Container, "BTSLIFE", "PROCESS", 3).unwrap();
        let items = ["ALPHA", "BETA", "name.with/slash"]
            .iter()
            .map(|name| BrowseItem::container(name).unwrap().with_epoch(3).unwrap())
            .collect();
        let token = book.start(scope, items).unwrap();
        let remaining = book.remaining(token, BrowseKind::Container).unwrap();
        assert_eq!(remaining.len(), 3);
        assert_eq!(
            book.next_skipping(token, BrowseKind::Container, 2, &remaining[1], 1),
            Err(token_error())
        );
        assert_eq!(
            book.next_skipping(token, BrowseKind::Container, 3, &remaining[1], 1),
            Ok(remaining[1].clone())
        );
        assert_eq!(
            book.peek(token, BrowseKind::Container),
            Ok(remaining[2].clone())
        );
        assert_eq!(
            book.next_skipping(token, BrowseKind::Container, 3, &remaining[2], 0),
            Ok(remaining[2].clone())
        );
        assert_eq!(
            book.peek(token, BrowseKind::Container),
            Err(super::end_of_browse())
        );
        book.end(token, BrowseKind::Container).unwrap();
        assert_eq!(book.peek(token, BrowseKind::Container), Err(token_error()));
    }

    #[test]
    fn bts_browse_durable_exact_replay_and_close() {
        let store = MemoryStore::new(Default::default());
        let browse = BtsBrowseStore::new(&store);
        let owner = BrowseOwner::new("RUN", "EXEC", "USER").unwrap();
        let start = BrowseEffect::Start {
            scope: BrowseScope::new(BrowseKind::Process, "TYPE", "TYPE", 1).unwrap(),
            items: vec![process_item("PROC")],
        };
        let first = browse.apply(&owner, "E1", [1; 32], &start).unwrap();
        assert_eq!(
            browse.replay(&owner, "E1", [1; 32]).unwrap(),
            Some((
                first.clone(),
                BrowseScope::new(BrowseKind::Process, "TYPE", "TYPE", 1).unwrap()
            ))
        );
        assert_eq!(
            browse.apply(&owner, "E1", [1; 32], &start),
            Ok(first.clone())
        );
        assert_eq!(
            browse.apply(&owner, "E1", [2; 32], &start),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(
            browse.peek(
                &BrowseOwner::new("RUN", "OTHER", "USER").unwrap(),
                1,
                BrowseKind::Process
            ),
            Err(HostProblem::Unauthorized)
        );
        let BrowseOutcome::Token(token) = first else {
            panic!("expected token")
        };
        let expected = browse.peek(&owner, token, BrowseKind::Process).unwrap();
        assert_eq!(
            browse.apply(
                &owner,
                "E2",
                [3; 32],
                &BrowseEffect::Next {
                    token,
                    kind: BrowseKind::Process,
                    live_epoch: 2,
                    expected: expected.clone(),
                }
            ),
            Err(token_error())
        );
        assert!(matches!(
            browse.apply(
                &owner,
                "E2",
                [3; 32],
                &BrowseEffect::Next {
                    token,
                    kind: BrowseKind::Process,
                    live_epoch: 1,
                    expected,
                }
            ),
            Ok(BrowseOutcome::Item(_))
        ));
        browse.clear_existing(&owner, false).unwrap();
        assert_eq!(
            browse.peek(&owner, token, BrowseKind::Process),
            Err(token_error())
        );
        assert_eq!(
            browse.apply(&owner, "E1", [1; 32], &start),
            Err(HostProblem::NotFound)
        );
        assert!(matches!(
            browse.apply(&owner, "E4", [6; 32], &start),
            Ok(BrowseOutcome::Token(_))
        ));
        assert_eq!(
            browse.apply(&owner, "END", [4; 32], &BrowseEffect::Close),
            Ok(BrowseOutcome::Closed)
        );
        assert_eq!(
            browse.apply(&owner, "E3", [5; 32], &start),
            Err(HostProblem::NotFound)
        );
    }

    #[test]
    fn bts_browse_lifecycle_snapshots_are_ordered_and_bounded() {
        let store = MemoryStore::new(Default::default());
        let lifecycle = BtsLifecycleStore::new(&store);
        for (name, uow) in [("ZED", "UOW-Z"), ("ALPHA", "UOW-A")] {
            let root = BtsLifecycleStore::root_id("TYPE", name, uow).unwrap();
            let process =
                BtsProcess::new("TYPE", name, &root, "MAIN", "BTS1", "USER", uow).unwrap();
            lifecycle
                .define_process(process, uow, "EXEC", "USER")
                .unwrap();
            lifecycle.finish_uow(uow, "EXEC", "USER", true).unwrap();
        }
        let processes = lifecycle.list_processes("TYPE", "UOW").unwrap();
        assert_eq!(
            process_snapshot(&processes)
                .unwrap()
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>(),
            vec!["ALPHA", "ZED"]
        );
        let root = &processes[0].root_id;
        assert_eq!(
            activity_flat_snapshot(&processes[0], "UOW").unwrap()[0].level,
            0
        );
        assert!(
            activity_children_snapshot(&processes[0], root, "UOW")
                .unwrap()
                .is_empty()
        );
        assert_eq!(lifecycle.list_processes("TYPE", "OTHER").unwrap().len(), 2);
    }

    #[test]
    fn bts_browse_cursor_survives_sqlite_restart() {
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-bts-browse-{}-{}",
            std::process::id(),
            NEXT_SQLITE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let url = format!("sqlite://{}?mode=rwc", directory.join("state.db").display());
        let owner = BrowseOwner::new("RUN", "EXEC", "USER").unwrap();
        let effect = BrowseEffect::Start {
            scope: BrowseScope::new(BrowseKind::Process, "BTSREPO", "TYPE", 1).unwrap(),
            items: vec![process_item("PROC")],
        };
        let token = {
            let sqlite = SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
            let browse = BtsBrowseStore::new(&sqlite);
            let BrowseOutcome::Token(token) =
                browse.apply(&owner, "START", [1; 32], &effect).unwrap()
            else {
                panic!("expected token")
            };
            token
        };
        {
            let sqlite = SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
            let browse = BtsBrowseStore::new(&sqlite);
            assert_eq!(
                browse.apply(&owner, "START", [1; 32], &effect),
                Ok(BrowseOutcome::Token(token))
            );
            let expected = browse.peek(&owner, token, BrowseKind::Process).unwrap();
            assert_eq!(
                browse.apply(
                    &owner,
                    "NEXT",
                    [2; 32],
                    &BrowseEffect::Next {
                        token,
                        kind: BrowseKind::Process,
                        live_epoch: 1,
                        expected: expected.clone(),
                    }
                ),
                Ok(BrowseOutcome::Item(expected))
            );
            assert_eq!(
                browse.apply(&owner, "CLOSE", [3; 32], &BrowseEffect::Close),
                Ok(BrowseOutcome::Closed)
            );
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn bts_browse_event_metadata_replays_identically_after_sqlite_restart() {
        use super::BrowseEventMetadata;
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-bts-event-metadata-{}-{}",
            std::process::id(),
            NEXT_SQLITE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let url = format!("sqlite://{}?mode=rwc", directory.join("state.db").display());
        let owner = BrowseOwner::new("RUN", "EXEC", "USER").unwrap();
        let item = BrowseItem::new("READY", None, 0)
            .unwrap()
            .with_epoch(3)
            .unwrap()
            .with_event_metadata(BrowseEventMetadata {
                event_type: 226,
                fire_status: 1000,
                composite: None,
                predicate: None,
                timer: None,
            });
        let returned = {
            let sqlite = SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
            let browse = BtsBrowseStore::new(&sqlite);
            let start = BrowseEffect::Start {
                scope: BrowseScope::new(BrowseKind::Event, "BTSEVENT", "CICS.BTS.A.BROWSE", 7)
                    .unwrap(),
                items: vec![item.clone()],
            };
            let BrowseOutcome::Token(token) =
                browse.apply(&owner, "START", [1; 32], &start).unwrap()
            else {
                panic!("expected token")
            };
            let next = BrowseEffect::Next {
                token,
                kind: BrowseKind::Event,
                live_epoch: 3,
                expected: item.clone(),
            };
            browse.apply(&owner, "NEXT", [2; 32], &next).unwrap()
        };
        {
            let sqlite = SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
            let browse = BtsBrowseStore::new(&sqlite);
            assert_eq!(
                browse.replay(&owner, "NEXT", [2; 32]).unwrap().unwrap().0,
                returned
            );
            let BrowseOutcome::Item(replayed) = returned else {
                panic!("expected item")
            };
            assert_eq!(replayed.event_metadata.unwrap().event_type, 226);
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}
