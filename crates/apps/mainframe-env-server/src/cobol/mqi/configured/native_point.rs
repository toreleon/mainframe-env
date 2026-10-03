//! Original installed-frame ABI/source forwarding; no point semantic authority.
use super::*;
use mainframe_env_host_api::mq_md_value::MqMdCharacterEncoding;
use mainframe_env_host_api::mq_mqi::{MqMqiCall, MqMqiContext, MqMqiUnitOfWork};
use mainframe_env_host_api::mq_object_route::MqRouteLookup;
use mainframe_env_host_api::mq_raw_layout::{
    MqRawCharacterEncoding, MqRawNumberEncoding, MqRawStructureEncoding,
};
use mainframe_env_host_api::mq_wire_options::{MqWireBindings, MqWireQueueManagerPlatform};
use mainframe_env_host_api::{MqHconn, MqHobj};
use mainframe_env_interpreter::{
    MqMqiAbiScope, MqMqiNativePoint, MqMqiNativePointTarget, MqMqiNativeStructure,
};
use mainframe_env_mq::{
    MqTrustedBatchFrame, MqTrustedBatchPointProfile, MqTrustedBatchPointTarget,
    MqTrustedBatchProducerSource, MqTrustedBatchStructureProfile,
};
use std::sync::OnceLock;

impl ConfiguredInstalledMqHost {
    /// PRIVILEGED configured compiled-root OPEN/CLOSE and complete PUT/PUT1/GET setup,
    /// not application
    /// attestation or public readiness. Requires existing complete native rich
    /// rows and genuine eager compiled root admission. SAME TASK children share
    /// that root's preallocated volatile ABI; no cold alias restoration exists.
    /// Installs one NoContext-only source before activation. It provides neither
    /// JES/default context nor physical GMT, and changes no ProductServer default.
    #[allow(clippy::too_many_arguments)]
    pub fn open_native_points(
        store: Arc<dyn PlatformStore>,
        authorizer: Arc<dyn EnterpriseAuthorizer>,
        clock: Arc<dyn MqReplayClock>,
        descriptor: CapabilityDescriptor,
        mq_limits: MqLimits,
        host_limits: HostLimits,
        mqi_limits: MqMqiLimits,
        generation: u64,
        fence: u64,
        bounds: InstalledMqHostBounds,
    ) -> Result<Arc<Self>, HostProblem> {
        Self::open_configured(
            store,
            authorizer,
            clock,
            descriptor,
            mq_limits,
            host_limits,
            mqi_limits,
            generation,
            fence,
            bounds,
            true,
        )
    }
}

#[derive(Clone)]
pub(super) struct RootAbi {
    pub(super) context: MqMqiContext,
    pub(super) scope: Arc<MqMqiAbiScope>,
}
pub(super) fn abi_charge(host: &ConfiguredInstalledMqHost) -> Result<usize, HostProblem> {
    if !host.native_points {
        return Ok(0);
    }
    // Conservative native enum-slot storage bound, not wire framing or IBM limit.
    let bytes = host
        .mq_limits
        .max_handles
        .checked_mul(std::mem::size_of::<(MqHconn, MqHobj, i32, usize, usize)>())
        .and_then(|n| n.checked_add(std::mem::size_of::<MqMqiAbiScope>()))
        .ok_or(HostProblem::ResourceExhausted)?;
    if bytes > host.host_limits.max_state_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok(bytes)
}
pub(super) fn allocate(
    host: &ConfiguredInstalledMqHost,
    frame: &MqTrustedBatchFrame,
) -> Result<Option<RootAbi>, HostProblem> {
    if !host.native_points {
        return Ok(None);
    }
    abi_charge(host)?;
    let context = frame.context()?;
    Ok(Some(RootAbi {
        context,
        scope: Arc::new(MqMqiAbiScope::new(context, host.mq_limits.max_handles)?),
    }))
}

pub(super) struct Source {
    host: OnceLock<Weak<ConfiguredInstalledMqHost>>,
    store: Arc<dyn PlatformStore>,
    max_text: usize,
}
impl Source {
    pub(super) fn new(store: Arc<dyn PlatformStore>, max_text: usize) -> Self {
        Self {
            host: OnceLock::new(),
            store,
            max_text,
        }
    }
    pub(super) fn bind(&self, host: &Arc<ConfiguredInstalledMqHost>) -> Result<(), HostProblem> {
        self.host
            .set(Arc::downgrade(host))
            .map_err(|_| HostProblem::Unauthorized)
    }
}
impl MqTrustedBatchProducerSource for Source {
    fn encode_structure(
        &self,
        text: &str,
        characters: MqMdCharacterEncoding,
    ) -> Result<Vec<u8>, HostProblem> {
        if text.len() > self.max_text {
            return Err(HostProblem::ResourceExhausted);
        }
        match characters {
            // ASCII identity requires no translation table. Non-ASCII text is
            // unsupported, never silently transliterated or normalized.
            MqMdCharacterEncoding::AsciiCompatible if text.is_ascii() => {
                let mut bytes = Vec::new();
                bytes
                    .try_reserve_exact(text.len())
                    .map_err(|_| HostProblem::ResourceExhausted)?;
                bytes.extend_from_slice(text.as_bytes());
                Ok(bytes)
            }
            MqMdCharacterEncoding::OwnedCp037 => mainframe_env_encoding::CodePage::Cp037
                .encode(text, self.max_text)
                .map_err(|_| HostProblem::Unsupported),
            _ => Err(HostProblem::Unsupported),
        }
    }
    fn check_live(&self, original: &Invocation) -> Result<(), HostProblem> {
        let host = self
            .host
            .get()
            .and_then(Weak::upgrade)
            .ok_or(HostProblem::Unauthorized)?;
        if !host.native_points || !Arc::ptr_eq(&self.store, &host.store) {
            return Err(HostProblem::Unauthorized);
        }
        // Never wait for topology/frame/service reentry from a selected callback.
        // Only immutable/atomic fields are inspected; callbacks run after release.
        let frame = {
            let map = host
                .topology
                .try_lock()
                .map_err(|_| HostProblem::UnknownOutcome)?;
            let frame = if original.parent_execution_id.is_none() {
                match map.roots.get(&original.execution_id) {
                    Some(RootEntry::Retained {
                        frame,
                        native: Some(_),
                        ..
                    }) => frame,
                    _ => return Err(HostProblem::Unauthorized),
                }
            } else {
                match map.frames.get(&original.execution_id) {
                    Some(FrameEntry::Retained(frame)) => frame,
                    _ => return Err(HostProblem::Unauthorized),
                }
            };
            let root = match map.roots.get(&frame.native_root_execution()) {
                Some(RootEntry::Retained {
                    frame,
                    native: Some(_),
                    ..
                }) => frame,
                _ => return Err(HostProblem::Unauthorized),
            };
            let abi = frame.abi.as_ref().ok_or(HostProblem::Unsupported)?;
            let root_abi = root.abi.as_ref().ok_or(HostProblem::Unsupported)?;
            if !Arc::ptr_eq(&abi.scope, &root_abi.scope) || abi.context != root_abi.context {
                return Err(HostProblem::Unauthorized);
            }
            root.check_original(root.original())?;
            frame.check_original(original)?;
            frame.clone()
        };
        if !frame.same_control(&host.control) {
            return Err(HostProblem::Unauthorized);
        }
        let compiled = frame.compiled().ok_or(HostProblem::Unsupported)?;
        let control = host
            .control
            .observe(original)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        super::super::check_controls(original, control, 0)?;
        let current = self
            .store
            .get_execution(&original.execution_id)
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .ok_or(HostProblem::Unauthorized)?;
        if current.state != mainframe_env_store_api::ExecutionState::Running
            || current.execution_id != original.execution_id
            || current.run_unit_id != original.run_unit_id
            || current.principal != *original.principal.id()
            || current.attempt != original.attempt
            || current.selector != original.selector
            || current.artifact != original.artifact
            || current.version == 0
            || current.terminal_tick.is_some()
            || current
                .lease_expiry_tick
                .is_some_and(|t| t <= control.now_tick)
            || self
                .store
                .get_provider_state(&compiled.catalog.namespace, &compiled.catalog.key)
                .map_err(|_| HostProblem::InfrastructureFailure)?
                .as_ref()
                != Some(&compiled.catalog)
        {
            return Err(HostProblem::Unauthorized);
        }
        frame.check_original(original)
    }
    // physical_gmt/batch_context retain Unsupported; NoContext never samples them.
}

struct Structure {
    frame: Arc<ClosedFrame>,
    profile: MqTrustedBatchStructureProfile,
}
pub(super) fn capture(
    frame: Arc<ClosedFrame>,
    original: &Invocation,
    call: MqMqiCall,
    connection: MqHconn,
) -> Result<Arc<dyn MqMqiNativeStructure>, HostProblem> {
    if frame.abi.is_none()
        || !matches!(
            call,
            MqMqiCall::Open
                | MqMqiCall::Close
                | MqMqiCall::Put
                | MqMqiCall::PutOne
                | MqMqiCall::Get
        )
    {
        return Err(HostProblem::Unsupported);
    }
    let profile = frame.with_final_profile(
        original,
        |state| {
            let profile = state.facet.structure_profile(call, connection)?;
            if profile.characters() != MqMdCharacterEncoding::AsciiCompatible {
                return Err(HostProblem::Unsupported);
            }
            Ok(profile)
        },
        |state, profile| state.facet.recheck_structure_profile(profile),
    )?;
    Ok(Arc::new(Structure { frame, profile }))
}
impl MqMqiNativeStructure for Structure {
    fn encoding(&self) -> MqRawStructureEncoding {
        MqRawStructureEncoding {
            numbers: MqRawNumberEncoding::NormalBigEndian,
            characters: MqRawCharacterEncoding::AsciiCompatible,
        }
    }
    fn recheck(&self) -> Result<(), HostProblem> {
        self.frame.with_final_profile(
            self.frame.original(),
            |_| Ok(()),
            |state, _| state.facet.recheck_structure_profile(&self.profile),
        )
    }
    fn point(
        &self,
        target: &MqMqiNativePointTarget,
    ) -> Result<Arc<dyn MqMqiNativePoint>, HostProblem> {
        let target = match target {
            MqMqiNativePointTarget::Open { lookup, access } => MqTrustedBatchPointTarget::Open {
                lookup: lookup.clone(),
                access: *access,
            },
            MqMqiNativePointTarget::Object(object) => MqTrustedBatchPointTarget::Object(*object),
            MqMqiNativePointTarget::PutOne { lookup } => MqTrustedBatchPointTarget::PutOne {
                lookup: lookup.clone(),
            },
        };
        let profile = self.frame.with_final_profile(
            self.frame.original(),
            |state| state.facet.point_profile(&self.profile, target),
            |state, profile| state.facet.recheck_point_profile(profile),
        )?;
        Ok(Arc::new(Point {
            frame: self.frame.clone(),
            profile,
        }))
    }
}
struct Point {
    frame: Arc<ClosedFrame>,
    profile: MqTrustedBatchPointProfile,
}
impl MqMqiNativePoint for Point {
    fn descriptor_version(&self) -> Result<i32, HostProblem> {
        self.frame.with_final_profile(
            self.frame.original(),
            |_| Ok(self.profile.descriptor_version()),
            |state, _| state.facet.recheck_point_profile(&self.profile),
        )
    }
    fn max_message_bytes(&self) -> Result<usize, HostProblem> {
        self.frame.with_final_profile(
            self.frame.original(),
            |_| Ok(self.profile.max_message_bytes()),
            |state, _| state.facet.recheck_point_profile(&self.profile),
        )
    }
    fn recheck(&self) -> Result<(), HostProblem> {
        self.frame.with_final_profile(
            self.frame.original(),
            |_| Ok(()),
            |state, _| state.facet.recheck_point_profile(&self.profile),
        )
    }
}
impl MqWireBindings for Point {
    fn queue_defaults_are_represented(
        &self,
        c: MqHconn,
        o: Option<MqHobj>,
        q: Option<&MqRouteLookup>,
    ) -> bool {
        if self.frame.check_original(self.frame.original()).is_err() {
            return false;
        }
        let value = self
            .profile
            .wire_bindings()
            .queue_defaults_are_represented(c, o, q);
        value && self.frame.check_original(self.frame.original()).is_ok()
    }
    fn queue_manager_platform(&self) -> MqWireQueueManagerPlatform {
        self.profile.wire_bindings().queue_manager_platform()
    }
    fn admitted_unit(&self, c: MqHconn) -> Option<MqMqiUnitOfWork> {
        self.frame.check_original(self.frame.original()).ok()?;
        let value = self.profile.wire_bindings().admitted_unit(c);
        self.frame.check_original(self.frame.original()).ok()?;
        value
    }
    fn existing_cursor(&self, c: MqHconn, o: MqHobj) -> Option<u64> {
        self.frame.check_original(self.frame.original()).ok()?;
        self.profile.wire_bindings().existing_cursor(c, o)
    }
    fn milliseconds_to_ticks(&self, milliseconds: u32) -> Option<u64> {
        self.frame.check_original(self.frame.original()).ok()?;
        self.profile
            .wire_bindings()
            .milliseconds_to_ticks(milliseconds)
    }
}
