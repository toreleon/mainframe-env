//! Durable definitions and device-specific effects for CIC-905 ISSUE forms.
//!
//! This record owns physical terminal/3740/3650 and LU6.1 TCTTE facilities.
//! APPC and MRO conversation ownership belongs to the shared protocol ledger.

use mainframe_env_store_api::{
    ProviderStateMutation, ProviderStateRecord, ProviderStateStore, ProviderStateWrite, StoreError,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

mod route;
pub(in crate::service) use route::finish_task;
pub(in crate::service) use route::invoke;
pub use route::prune_issue_device_receipts;

pub const ISSUE_DEVICE_NAMESPACE: &str = "cics-issue-device-v1";
const MAX_ROW_BYTES: usize = 64 * 1024;
const MAX_NAMES: usize = 64;
const MAX_PASS_BYTES: usize = 255;
const MAX_PRINT_BYTES: usize = 32_767;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum IssueDeviceKind {
    Display3270,
    Printer3270,
    Entry3740,
    Interpreter3650,
    Lu61,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IssueDeviceDefinition {
    pub terminal: String,
    pub kind: IssueDeviceKind,
    /// The physical control-unit identity required for 3270 buffer copying.
    pub control_unit: Option<String>,
    /// Printer terminals in source-defined preference order.
    pub printers: Vec<String>,
    /// Installed 3650 application names.
    pub programs: Vec<String>,
    /// Installed Communications Server application names allowed for PASS.
    pub applications: Vec<String>,
    /// CINIT X'0D' logon mode, when retained for LOGONLOGMODE.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logon_logmode: Option<String>,
    /// TYPETERM DISCREQ or RELREQ capability.
    pub disconnect_allowed: bool,
    /// Communications Server AUTH=PASS capability.
    pub pass_allowed: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IssueDeviceState {
    pub endfile: bool,
    pub endoutput: bool,
    pub eods: bool,
    pub loaded_program: Option<String>,
    pub loaded_converse: bool,
    pub pass_target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pass_owner_run_unit: Option<String>,
    pub pass_data: Vec<u8>,
    pub pass_logmode: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub pass_use_logon_mode: bool,
    pub pass_noquiesce: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub pass_delivered: bool,
    /// Target run and carrier event that consumed a committed PASS.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pass_claimed_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pass_claim_event: Option<String>,
    pub disconnected: bool,
    /// Task that owns an alternate LUTYPE6.1 TCTTE facility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lu61_owner_run_unit: Option<String>,
    /// Outbound direction-change request retained for the LU6.1 peer.
    #[serde(default, skip_serializing_if = "is_false")]
    pub lu61_signal_pending: bool,
    pub print_count: u32,
    pub last_print: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_printer: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub printer_out_of_service: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub printer_attached_run: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IssueDeviceRecord {
    schema_version: u16,
    pub version: u64,
    pub definition: IssueDeviceDefinition,
    pub state: IssueDeviceState,
}

/// Exact logon material supplied to a trusted target application claim.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IssuePassTransfer {
    pub terminal: String,
    pub application: String,
    pub logon_data: Vec<u8>,
    pub logmode: Option<String>,
    pub noquiesce: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IssueDeviceProblem {
    Malformed,
    WrongDevice,
    NotConfigured,
    Disconnected,
    Length,
    Capacity,
    StaleOwner,
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn valid_name(name: &str, maximum: usize) -> bool {
    !name.is_empty()
        && name.len() <= maximum
        && name.bytes().all(|byte| {
            byte.is_ascii_uppercase() || byte.is_ascii_digit() || b"$#@".contains(&byte)
        })
}

fn valid_names(names: &[String], maximum: usize) -> bool {
    names.len() <= MAX_NAMES
        && names.iter().all(|name| valid_name(name, maximum))
        && names.iter().collect::<BTreeSet<_>>().len() == names.len()
}

impl IssueDeviceDefinition {
    pub fn validate(&self) -> Result<(), IssueDeviceProblem> {
        if !valid_name(&self.terminal, 4)
            || self
                .control_unit
                .as_ref()
                .is_some_and(|name| !valid_name(name, 8))
            || !valid_names(&self.printers, 4)
            || !valid_names(&self.programs, 8)
            || !valid_names(&self.applications, 8)
            || self
                .logon_logmode
                .as_ref()
                .is_some_and(|name| !valid_name(name, 8))
            || matches!(
                self.kind,
                IssueDeviceKind::Display3270 | IssueDeviceKind::Printer3270
            ) && self.control_unit.is_none()
            || !matches!(
                self.kind,
                IssueDeviceKind::Display3270 | IssueDeviceKind::Interpreter3650
            ) && !self.printers.is_empty()
            || self.kind != IssueDeviceKind::Interpreter3650 && !self.programs.is_empty()
            || self.pass_allowed && self.applications.is_empty()
        {
            return Err(IssueDeviceProblem::Malformed);
        }
        Ok(())
    }
}

impl IssueDeviceRecord {
    #[cfg(test)]
    pub fn new(definition: IssueDeviceDefinition) -> Result<Self, IssueDeviceProblem> {
        let record = Self {
            schema_version: 1,
            version: 1,
            definition,
            state: IssueDeviceState::default(),
        };
        record.validate()?;
        Ok(record)
    }

    pub fn load(
        store: &dyn ProviderStateStore,
        terminal: &str,
    ) -> Result<Option<Self>, StoreError> {
        let Some(row) = store.get_provider_state(ISSUE_DEVICE_NAMESPACE, terminal)? else {
            return Ok(None);
        };
        if row.version == 0 || row.payload.len() > MAX_ROW_BYTES {
            return Err(StoreError::IncompatibleVersion);
        }
        let record: Self =
            serde_json::from_slice(&row.payload).map_err(|_| StoreError::IncompatibleVersion)?;
        if record.version != row.version
            || record.definition.terminal != terminal
            || record.validate().is_err()
            || record
                .encode()
                .map_err(|_| StoreError::IncompatibleVersion)?
                != row.payload
        {
            return Err(StoreError::IncompatibleVersion);
        }
        Ok(Some(record))
    }

    pub fn persist(
        &self,
        next: &mut Self,
        store: &dyn ProviderStateStore,
    ) -> Result<bool, StoreError> {
        if self.definition != next.definition {
            return Err(StoreError::IncompatibleVersion);
        }
        next.version = self
            .version
            .checked_add(1)
            .ok_or(StoreError::CapacityExceeded)?;
        let payload = next.encode().map_err(|_| StoreError::CapacityExceeded)?;
        match store.put_provider_state(
            ProviderStateRecord {
                namespace: ISSUE_DEVICE_NAMESPACE.into(),
                key: self.definition.terminal.clone(),
                version: next.version,
                payload,
            },
            Some(self.version),
        ) {
            Ok(()) => Ok(true),
            Err(StoreError::Conflict) => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// Prepare an exact-version state mutation for an atomic device/receipt
    /// commit. The caller owns authorization and the source response mapping.
    pub fn mutation(&self, next: &mut Self) -> Result<ProviderStateMutation, StoreError> {
        if self.definition != next.definition {
            return Err(StoreError::IncompatibleVersion);
        }
        next.version = self
            .version
            .checked_add(1)
            .ok_or(StoreError::CapacityExceeded)?;
        let payload = next.encode().map_err(|_| StoreError::CapacityExceeded)?;
        Ok(ProviderStateMutation::Put(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: ISSUE_DEVICE_NAMESPACE.into(),
                key: self.definition.terminal.clone(),
                version: next.version,
                payload,
            },
            expected_version: Some(self.version),
        }))
    }

    #[cfg(test)]
    pub fn install(&self, store: &dyn ProviderStateStore) -> Result<(), StoreError> {
        if self.version != 1 {
            return Err(StoreError::IncompatibleVersion);
        }
        let payload = self.encode().map_err(|_| StoreError::CapacityExceeded)?;
        store.put_provider_state(
            ProviderStateRecord {
                namespace: ISSUE_DEVICE_NAMESPACE.into(),
                key: self.definition.terminal.clone(),
                version: self.version,
                payload,
            },
            None,
        )
    }

    pub fn validate(&self) -> Result<(), IssueDeviceProblem> {
        self.definition.validate()?;
        if self.schema_version != 1
            || self.version == 0
            || self.state.pass_data.len() > MAX_PASS_BYTES
            || self.state.last_print.len() > MAX_PRINT_BYTES
            || self.state.loaded_program.as_ref().is_some_and(|name| {
                !self.definition.programs.contains(name)
                    || self.definition.kind != IssueDeviceKind::Interpreter3650
            })
            || self.state.loaded_converse && self.state.loaded_program.is_none()
            || self.state.pass_target.as_ref().is_some_and(|name| {
                !self.definition.pass_allowed || !self.definition.applications.contains(name)
            })
            || self.state.pass_target.is_none()
                && (!self.state.pass_data.is_empty()
                    || self.state.pass_owner_run_unit.is_some()
                    || self.state.pass_logmode.is_some()
                    || self.state.pass_use_logon_mode
                    || self.state.pass_noquiesce
                    || self.state.pass_delivered
                    || self.state.pass_claimed_by.is_some()
                    || self.state.pass_claim_event.is_some())
            || self.state.pass_claimed_by.is_some() != self.state.pass_claim_event.is_some()
            || self.state.pass_claimed_by.is_some() && !self.state.pass_delivered
            || self
                .state
                .pass_claimed_by
                .as_ref()
                .is_some_and(|run| run.is_empty() || run.len() > 128 || run.contains('\0'))
            || self.state.pass_claim_event.as_ref().is_some_and(|event| {
                event.is_empty()
                    || event.len() > 128
                    || !event.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')
                    })
            })
            || self
                .state
                .pass_owner_run_unit
                .as_ref()
                .is_some_and(|owner| owner.is_empty() || owner.len() > 128)
            || self.state.pass_delivered && !self.state.disconnected
            || self
                .state
                .pass_logmode
                .as_ref()
                .is_some_and(|name| !valid_name(name, 8))
            || self.state.pass_use_logon_mode
                && self.state.pass_logmode != self.definition.logon_logmode
            || (self.state.endfile || self.state.endoutput)
                && self.definition.kind != IssueDeviceKind::Entry3740
            || self.state.eods && self.definition.kind != IssueDeviceKind::Interpreter3650
            || self.state.print_count > 0
                && !matches!(
                    self.definition.kind,
                    IssueDeviceKind::Display3270
                        | IssueDeviceKind::Printer3270
                        | IssueDeviceKind::Interpreter3650
                )
            || self.state.last_printer.is_some()
                && !matches!(
                    self.definition.kind,
                    IssueDeviceKind::Display3270 | IssueDeviceKind::Interpreter3650
                )
            || self.state.printer_out_of_service
                && self.definition.kind != IssueDeviceKind::Printer3270
            || self.state.printer_attached_run.is_some()
                && self.definition.kind != IssueDeviceKind::Printer3270
            || self
                .state
                .printer_attached_run
                .as_ref()
                .is_some_and(|owner| owner.is_empty() || owner.len() > 128)
            || self.state.lu61_owner_run_unit.is_some()
                && self.definition.kind != IssueDeviceKind::Lu61
            || self.state.lu61_signal_pending
                && (self.definition.kind != IssueDeviceKind::Lu61
                    || self.state.disconnected
                    || self.state.lu61_owner_run_unit.is_none())
            || self
                .state
                .lu61_owner_run_unit
                .as_ref()
                .is_some_and(|owner| owner.is_empty() || owner.len() > 128 || owner.contains('\0'))
        {
            return Err(IssueDeviceProblem::Malformed);
        }
        Ok(())
    }

    fn encode(&self) -> Result<Vec<u8>, IssueDeviceProblem> {
        self.validate()?;
        let payload = serde_json::to_vec(self).map_err(|_| IssueDeviceProblem::Malformed)?;
        if payload.len() > MAX_ROW_BYTES {
            return Err(IssueDeviceProblem::Capacity);
        }
        Ok(payload)
    }

    fn active(&self) -> Result<(), IssueDeviceProblem> {
        if self.state.disconnected {
            Err(IssueDeviceProblem::Disconnected)
        } else {
            Ok(())
        }
    }

    pub fn mark_endfile(&mut self, also_endoutput: bool) -> Result<(), IssueDeviceProblem> {
        self.active()?;
        if self.definition.kind != IssueDeviceKind::Entry3740 {
            return Err(IssueDeviceProblem::WrongDevice);
        }
        self.state.endfile = true;
        self.state.endoutput |= also_endoutput;
        Ok(())
    }

    pub fn mark_endoutput(&mut self, also_endfile: bool) -> Result<(), IssueDeviceProblem> {
        self.active()?;
        if self.definition.kind != IssueDeviceKind::Entry3740 {
            return Err(IssueDeviceProblem::WrongDevice);
        }
        self.state.endoutput = true;
        self.state.endfile |= also_endfile;
        Ok(())
    }

    pub fn mark_eods(&mut self) -> Result<(), IssueDeviceProblem> {
        self.active()?;
        if self.definition.kind != IssueDeviceKind::Interpreter3650 {
            return Err(IssueDeviceProblem::WrongDevice);
        }
        self.state.eods = true;
        Ok(())
    }

    pub fn load_program(
        &mut self,
        program: &str,
        converse: bool,
    ) -> Result<(), IssueDeviceProblem> {
        self.active()?;
        if self.definition.kind != IssueDeviceKind::Interpreter3650 {
            return Err(IssueDeviceProblem::WrongDevice);
        }
        if !self.definition.programs.iter().any(|name| name == program) {
            return Err(IssueDeviceProblem::NotConfigured);
        }
        self.state.loaded_program = Some(program.into());
        self.state.loaded_converse = converse;
        Ok(())
    }

    pub fn prepare_pass(
        &mut self,
        application: &str,
        owner_run_unit: &str,
        data: &[u8],
        logmode: Option<&str>,
        use_logon_mode: bool,
        noquiesce: bool,
    ) -> Result<(), IssueDeviceProblem> {
        self.active()?;
        if self.state.pass_target.is_some()
            && self.state.pass_owner_run_unit.as_deref() != Some(owner_run_unit)
        {
            return Err(IssueDeviceProblem::StaleOwner);
        }
        if !self.definition.pass_allowed || !self.definition.disconnect_allowed {
            return Err(IssueDeviceProblem::NotConfigured);
        }
        if !self
            .definition
            .applications
            .iter()
            .any(|name| name == application)
        {
            return Err(IssueDeviceProblem::NotConfigured);
        }
        if owner_run_unit.is_empty() || owner_run_unit.len() > 128 {
            return Err(IssueDeviceProblem::StaleOwner);
        }
        if data.len() > MAX_PASS_BYTES
            || logmode.is_some_and(|name| !valid_name(name, 8))
            || logmode.is_some() && use_logon_mode
        {
            return Err(IssueDeviceProblem::Length);
        }
        let selected_logmode = if use_logon_mode {
            self.definition
                .logon_logmode
                .as_deref()
                .ok_or(IssueDeviceProblem::NotConfigured)?
        } else {
            logmode.unwrap_or("")
        };
        self.state.pass_target = Some(application.into());
        self.state.pass_owner_run_unit = Some(owner_run_unit.into());
        self.state.pass_data = data.to_vec();
        self.state.pass_logmode = (!selected_logmode.is_empty()).then(|| selected_logmode.into());
        self.state.pass_use_logon_mode = use_logon_mode;
        self.state.pass_noquiesce = noquiesce;
        self.state.pass_delivered = false;
        self.state.pass_claimed_by = None;
        self.state.pass_claim_event = None;
        Ok(())
    }

    /// Commit an accepted PASS only at the known successful task end. A
    /// repeated completion of the same owner is idempotent after restart.
    pub fn complete_pass(&mut self, owner_run_unit: &str) -> Result<bool, IssueDeviceProblem> {
        if self.state.pass_target.is_none() {
            return Ok(false);
        }
        if self.state.pass_owner_run_unit.as_deref() != Some(owner_run_unit) {
            return Err(IssueDeviceProblem::StaleOwner);
        }
        if self.state.pass_delivered {
            return Ok(false);
        }
        self.active()?;
        self.state.disconnected = true;
        self.state.pass_delivered = true;
        Ok(true)
    }

    /// Roll back an uncommitted PASS without severing its source session.
    pub fn cancel_pass(&mut self, owner_run_unit: &str) -> Result<bool, IssueDeviceProblem> {
        if self.state.pass_target.is_none() {
            return Ok(false);
        }
        if self.state.pass_owner_run_unit.as_deref() != Some(owner_run_unit)
            || self.state.pass_delivered
        {
            return Err(IssueDeviceProblem::StaleOwner);
        }
        self.state.pass_target = None;
        self.state.pass_owner_run_unit = None;
        self.state.pass_data.clear();
        self.state.pass_logmode = None;
        self.state.pass_use_logon_mode = false;
        self.state.pass_noquiesce = false;
        self.state.pass_claimed_by = None;
        self.state.pass_claim_event = None;
        Ok(true)
    }

    pub fn claim_pass(
        &mut self,
        application: &str,
        target_run_unit: &str,
        event_id: &str,
    ) -> Result<bool, IssueDeviceProblem> {
        if !self.state.pass_delivered
            || !self.state.disconnected
            || self.state.pass_target.as_deref() != Some(application)
            || target_run_unit.is_empty()
            || target_run_unit.len() > 128
            || target_run_unit.contains('\0')
            || event_id.is_empty()
            || event_id.len() > 128
            || !event_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err(IssueDeviceProblem::StaleOwner);
        }
        if self.state.pass_claimed_by.is_some() {
            return if self.state.pass_claimed_by.as_deref() == Some(target_run_unit)
                && self.state.pass_claim_event.as_deref() == Some(event_id)
            {
                Ok(false)
            } else {
                Err(IssueDeviceProblem::StaleOwner)
            };
        }
        self.state.pass_claimed_by = Some(target_run_unit.into());
        self.state.pass_claim_event = Some(event_id.into());
        Ok(true)
    }

    pub fn pass_transfer(&self) -> Result<IssuePassTransfer, IssueDeviceProblem> {
        if !self.state.pass_delivered || self.state.pass_claimed_by.is_none() {
            return Err(IssueDeviceProblem::StaleOwner);
        }
        Ok(IssuePassTransfer {
            terminal: self.definition.terminal.clone(),
            application: self
                .state
                .pass_target
                .clone()
                .ok_or(IssueDeviceProblem::Malformed)?,
            logon_data: self.state.pass_data.clone(),
            logmode: self.state.pass_logmode.clone(),
            noquiesce: self.state.pass_noquiesce,
        })
    }

    pub fn disconnect(&mut self) -> Result<(), IssueDeviceProblem> {
        self.active()?;
        if self.definition.kind == IssueDeviceKind::Lu61 && !self.definition.disconnect_allowed {
            return Err(IssueDeviceProblem::NotConfigured);
        }
        self.state.disconnected = true;
        self.state.lu61_signal_pending = false;
        Ok(())
    }

    pub fn mark_lu61_signal(&mut self, owner_run_unit: &str) -> Result<(), IssueDeviceProblem> {
        self.active()?;
        if self.definition.kind != IssueDeviceKind::Lu61 {
            return Err(IssueDeviceProblem::WrongDevice);
        }
        if self.state.lu61_owner_run_unit.as_deref() != Some(owner_run_unit) {
            return Err(IssueDeviceProblem::StaleOwner);
        }
        self.state.lu61_signal_pending = true;
        Ok(())
    }

    #[cfg(test)]
    pub fn assign_lu61_owner(&mut self, run_unit: &str) -> Result<(), IssueDeviceProblem> {
        self.active()?;
        if self.definition.kind != IssueDeviceKind::Lu61 {
            return Err(IssueDeviceProblem::WrongDevice);
        }
        if run_unit.is_empty() || run_unit.len() > 128 || run_unit.contains('\0') {
            return Err(IssueDeviceProblem::Malformed);
        }
        self.state.lu61_owner_run_unit = Some(run_unit.into());
        Ok(())
    }

    pub fn record_print(&mut self, printer: &str, bytes: &[u8]) -> Result<(), IssueDeviceProblem> {
        self.active()?;
        if !matches!(
            self.definition.kind,
            IssueDeviceKind::Display3270 | IssueDeviceKind::Interpreter3650
        ) || !self.definition.printers.iter().any(|name| name == printer)
        {
            return Err(IssueDeviceProblem::NotConfigured);
        }
        if bytes.len() > MAX_PRINT_BYTES {
            return Err(IssueDeviceProblem::Length);
        }
        self.state.print_count = self
            .state
            .print_count
            .checked_add(1)
            .ok_or(IssueDeviceProblem::Capacity)?;
        self.state.last_print = bytes.to_vec();
        self.state.last_printer = Some(printer.into());
        Ok(())
    }

    pub fn printer_available(&self) -> bool {
        self.definition.kind == IssueDeviceKind::Printer3270
            && !self.state.disconnected
            && !self.state.printer_out_of_service
            && self.state.printer_attached_run.is_none()
    }

    pub fn accept_print(&mut self, bytes: &[u8]) -> Result<(), IssueDeviceProblem> {
        if !self.printer_available() {
            return Err(IssueDeviceProblem::NotConfigured);
        }
        if bytes.len() > MAX_PRINT_BYTES {
            return Err(IssueDeviceProblem::Length);
        }
        self.state.print_count = self
            .state
            .print_count
            .checked_add(1)
            .ok_or(IssueDeviceProblem::Capacity)?;
        self.state.last_print = bytes.to_vec();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_store::{MemoryStore, SqliteStateStore};

    fn definition(terminal: &str, kind: IssueDeviceKind) -> IssueDeviceDefinition {
        IssueDeviceDefinition {
            terminal: terminal.into(),
            kind,
            control_unit: None,
            printers: vec![],
            programs: vec![],
            applications: vec![],
            logon_logmode: None,
            disconnect_allowed: true,
            pass_allowed: false,
        }
    }

    #[test]
    fn end_markers_are_device_specific_and_stale_cas_cannot_replace_them() {
        let store = MemoryStore::new(Default::default());
        let initial =
            IssueDeviceRecord::new(definition("T001", IssueDeviceKind::Entry3740)).unwrap();
        initial.install(&store).unwrap();
        let current = IssueDeviceRecord::load(&store, "T001").unwrap().unwrap();
        let mut next = current.clone();
        next.mark_endfile(true).unwrap();
        assert!(current.persist(&mut next, &store).unwrap());
        assert!(next.state.endfile && next.state.endoutput);
        let mut stale = current.clone();
        stale.mark_endoutput(false).unwrap();
        assert!(!current.persist(&mut stale, &store).unwrap());
        assert_eq!(
            IssueDeviceRecord::load(&store, "T001")
                .unwrap()
                .unwrap()
                .state,
            next.state
        );

        let mut wrong =
            IssueDeviceRecord::new(definition("T002", IssueDeviceKind::Entry3740)).unwrap();
        assert_eq!(wrong.mark_eods(), Err(IssueDeviceProblem::WrongDevice));
        assert!(!wrong.state.eods);
    }

    #[test]
    fn optional_saved_mode_fields_preserve_v1_canonical_reopen() {
        let store = MemoryStore::new(Default::default());
        let record =
            IssueDeviceRecord::new(definition("T005", IssueDeviceKind::Entry3740)).unwrap();
        let bytes = record.encode().unwrap();
        assert!(
            !bytes
                .windows(b"logon_logmode".len())
                .any(|w| w == b"logon_logmode")
        );
        assert!(
            !bytes
                .windows(b"pass_use_logon_mode".len())
                .any(|w| w == b"pass_use_logon_mode")
        );
        record.install(&store).unwrap();
        assert_eq!(IssueDeviceRecord::load(&store, "T005"), Ok(Some(record)));
    }

    #[test]
    fn sqlite_reopen_preserves_load_and_pass_with_exact_bounds() {
        let root = std::env::temp_dir().join(format!(
            "mainframe-env-cics-issue-device-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let url = format!("sqlite://{}?mode=rwc", root.join("state.db").display());
        let first = SqliteStateStore::open(&url, MAX_ROW_BYTES, 65_536).unwrap();
        let mut definition = definition("T003", IssueDeviceKind::Interpreter3650);
        definition.programs.push("PROG1".into());
        definition.applications.push("APPL1".into());
        definition.logon_logmode = Some("LOGON0".into());
        definition.pass_allowed = true;
        let initial = IssueDeviceRecord::new(definition).unwrap();
        initial.install(&first).unwrap();
        let mut next = initial.clone();
        next.load_program("PROG1", true).unwrap();
        next.mark_eods().unwrap();
        assert_eq!(
            next.prepare_pass("APPL1", "run", &[1; MAX_PASS_BYTES + 1], None, false, false),
            Err(IssueDeviceProblem::Length)
        );
        assert!(next.state.pass_target.is_none());
        next.prepare_pass(
            "APPL1",
            "run",
            &[2; MAX_PASS_BYTES],
            Some("MODE1"),
            false,
            true,
        )
        .unwrap();
        assert!(initial.persist(&mut next, &first).unwrap());
        drop(first);
        let reopened = SqliteStateStore::open(&url, MAX_ROW_BYTES, 65_536).unwrap();
        let record = IssueDeviceRecord::load(&reopened, "T003").unwrap().unwrap();
        assert_eq!(record.state.loaded_program.as_deref(), Some("PROG1"));
        assert!(record.state.loaded_converse && record.state.eods);
        assert_eq!(record.state.pass_data, vec![2; MAX_PASS_BYTES]);
        assert_eq!(record.state.pass_logmode.as_deref(), Some("MODE1"));
        assert_eq!(record.state.pass_owner_run_unit.as_deref(), Some("run"));
        let mut foreign = record.clone();
        assert_eq!(
            foreign.prepare_pass("APPL1", "other", b"OVERRIDE", None, false, false),
            Err(IssueDeviceProblem::StaleOwner)
        );
        assert_eq!(foreign.state, record.state);
        let mut stale = record.clone();
        assert_eq!(
            stale.complete_pass("other"),
            Err(IssueDeviceProblem::StaleOwner)
        );
        assert!(!stale.state.disconnected);
        assert_eq!(stale.complete_pass("run"), Ok(true));
        assert!(stale.state.disconnected && stale.state.pass_delivered);
        assert_eq!(stale.complete_pass("run"), Ok(false));
        assert_eq!(
            stale.cancel_pass("run"),
            Err(IssueDeviceProblem::StaleOwner)
        );
        let mut cancelled = record;
        assert_eq!(cancelled.cancel_pass("run"), Ok(true));
        assert!(cancelled.state.pass_target.is_none());
        assert!(!cancelled.state.disconnected);
        drop(reopened);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn printer_requires_a_configured_peer_and_keeps_bounded_image() {
        let mut definition = definition("T004", IssueDeviceKind::Display3270);
        definition.control_unit = Some("CU1".into());
        let mut device = IssueDeviceRecord::new(definition.clone()).unwrap();
        assert_eq!(
            device.record_print("P001", b"SCREEN"),
            Err(IssueDeviceProblem::NotConfigured)
        );
        definition.printers.push("P001".into());
        device = IssueDeviceRecord::new(definition).unwrap();
        device.record_print("P001", b"SCREEN").unwrap();
        assert_eq!(device.state.print_count, 1);
        assert_eq!(device.state.last_print, b"SCREEN");
        assert_eq!(device.state.last_printer.as_deref(), Some("P001"));
        assert_eq!(
            device.record_print("P001", &vec![0; MAX_PRINT_BYTES + 1]),
            Err(IssueDeviceProblem::Length)
        );
        assert_eq!(device.state.print_count, 1);
    }

    #[test]
    fn host_conversational_3650_can_print_its_3270_image() {
        let mut definition = definition("T365", IssueDeviceKind::Interpreter3650);
        definition.printers.push("P001".into());
        let mut device = IssueDeviceRecord::new(definition).unwrap();
        device.record_print("P001", b"3650 DISPLAY").unwrap();
        assert_eq!(device.state.last_printer.as_deref(), Some("P001"));
        assert_eq!(device.state.last_print, b"3650 DISPLAY");
        device.validate().unwrap();
    }

    #[test]
    fn lu61_alternate_owner_is_bounded_and_reopens_canonically() {
        let store = MemoryStore::new(Default::default());
        let initial = IssueDeviceRecord::new(definition("S001", IssueDeviceKind::Lu61)).unwrap();
        initial.install(&store).unwrap();
        let current = IssueDeviceRecord::load(&store, "S001").unwrap().unwrap();
        assert!(current.state.lu61_owner_run_unit.is_none());
        let mut owned = current.clone();
        owned.assign_lu61_owner("run-1").unwrap();
        current.persist(&mut owned, &store).unwrap();
        let reopened = IssueDeviceRecord::load(&store, "S001").unwrap().unwrap();
        assert_eq!(reopened.state.lu61_owner_run_unit.as_deref(), Some("run-1"));
        assert_eq!(
            owned.assign_lu61_owner(""),
            Err(IssueDeviceProblem::Malformed)
        );
        assert_eq!(owned.state.lu61_owner_run_unit.as_deref(), Some("run-1"));
    }

    #[test]
    fn lu61_signal_marker_reopens_without_changing_legacy_rows() {
        let store = MemoryStore::new(Default::default());
        let initial = IssueDeviceRecord::new(definition("S003", IssueDeviceKind::Lu61)).unwrap();
        assert!(
            !String::from_utf8_lossy(&initial.encode().unwrap()).contains("lu61_signal_pending")
        );
        let mut ownerless = initial.clone();
        ownerless.state.lu61_signal_pending = true;
        assert_eq!(ownerless.validate(), Err(IssueDeviceProblem::Malformed));
        initial.install(&store).unwrap();
        let mut owned = initial.clone();
        owned.assign_lu61_owner("run-3").unwrap();
        assert!(initial.persist(&mut owned, &store).unwrap());
        let current = IssueDeviceRecord::load(&store, "S003").unwrap().unwrap();
        let mut signaled = current.clone();
        assert_eq!(
            signaled.mark_lu61_signal("other"),
            Err(IssueDeviceProblem::StaleOwner)
        );
        assert!(!signaled.state.lu61_signal_pending);
        signaled.mark_lu61_signal("run-3").unwrap();
        assert!(current.persist(&mut signaled, &store).unwrap());
        let reopened = IssueDeviceRecord::load(&store, "S003").unwrap().unwrap();
        assert!(reopened.state.lu61_signal_pending);
        let mut closed = reopened.clone();
        closed.disconnect().unwrap();
        assert!(!closed.state.lu61_signal_pending);
        assert!(reopened.persist(&mut closed, &store).unwrap());
        let mut wrong =
            IssueDeviceRecord::new(definition("T004", IssueDeviceKind::Entry3740)).unwrap();
        assert_eq!(
            wrong.mark_lu61_signal("run-3"),
            Err(IssueDeviceProblem::WrongDevice)
        );
    }
}
