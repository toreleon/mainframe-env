//! Opaque native ABI observations; no permission, aliases or registry adoption.
use super::*;
use crate::service::{PointFacts, StructureFacts};
use mainframe_env_host_api::MqHobj;
use mainframe_env_host_api::mq_md_value::MqMdCharacterEncoding;
use mainframe_env_host_api::mq_mqi::MqMqiCall;
use mainframe_env_host_api::mq_object_route::{MqRouteLookup, MqRouteOpenAccess};
use mainframe_env_host_api::mq_wire_options::{MqWireBindings, MqWireQueueManagerPlatform};

/// Exact decoded target query. Values do not attest topology, owner or SAF.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MqTrustedBatchPointTarget {
    /// Explicit OUTPUT or INPUT_SHARED on predefined normal-local OD1 only.
    Open {
        lookup: MqRouteLookup,
        access: MqRouteOpenAccess,
    },
    /// Independently resolved predefined normal-local PUT1 target.
    PutOne { lookup: MqRouteLookup },
    /// Already issued live HOBJ; PUT requires OUTPUT, GET INPUT_SHARED,
    /// and CLOSE requires predefined. No capability is inferred from a name.
    Object(MqHobj),
}
/// Source profile before raw OD/MD decoding. No Clone/Serde/public constructor.
/// This is a native observation, not a root/frame, structure or execution permit.
/// ```compile_fail
/// use mainframe_env_mq::MqTrustedBatchStructureProfile;
/// fn cloneable<T: Clone>() {}
/// cloneable::<MqTrustedBatchStructureProfile>();
/// ```
/// ```compile_fail
/// use mainframe_env_mq::MqTrustedBatchStructureProfile;
/// fn serializable<T: serde::Serialize>() {}
/// serializable::<MqTrustedBatchStructureProfile>();
/// ```
pub struct MqTrustedBatchStructureProfile {
    root: Arc<Root>,
    frame: FrameLease,
    call: MqMqiCall,
    connection: MqHconn,
    facts: StructureFacts,
}
impl MqTrustedBatchStructureProfile {
    /// Actual configured structure character profile; body CCSID/Encoding is unrelated.
    pub fn characters(&self) -> MqMdCharacterEncoding {
        self.facts.characters
    }
    /// Actual configured QM structure CCSID; no inference of z/OS default37.
    pub fn coded_char_set_id(&self) -> i32 {
        self.facts.ccsid
    }
}
/// Exact tuple-bound native point observation, never executable authority.
/// No public constructor, Clone, Serde, numeric aliases or default metadata map.
/// ```compile_fail
/// use mainframe_env_mq::MqTrustedBatchPointProfile;
/// let forged = MqTrustedBatchPointProfile {};
/// ```
/// ```compile_fail
/// use mainframe_env_mq::MqTrustedBatchPointProfile;
/// fn cloneable<T: Clone>() {}
/// cloneable::<MqTrustedBatchPointProfile>();
/// ```
/// ```compile_fail
/// use mainframe_env_mq::MqTrustedBatchPointProfile;
/// fn serializable<T: serde::Serialize>() {}
/// serializable::<MqTrustedBatchPointProfile>();
/// ```
pub struct MqTrustedBatchPointProfile {
    root: Arc<Root>,
    frame: FrameLease,
    original: Invocation,
    call: MqMqiCall,
    connection: MqHconn,
    target: MqTrustedBatchPointTarget,
    facts: PointFacts,
}
impl MqTrustedBatchPointProfile {
    /// Actual homogeneous queue descriptor version. No V2-to-V1 narrowing.
    pub fn descriptor_version(&self) -> i32 {
        self.facts.version
    }
    /// Actual structure character profile. CP037 is truthful even when the host
    /// compiler adapter only accepts ASCII-compatible/big-endian structures.
    pub fn characters(&self) -> MqMdCharacterEncoding {
        self.facts.structure.characters
    }
    /// Lesser actual configured QM and queue message-body maxima; host/kernel
    /// quotas and per-call admission remain independently mandatory.
    pub fn max_message_bytes(&self) -> usize {
        self.facts.max_message_bytes
    }
    /// Existing sole wire binding port for this exact tuple. Consumer MUST keep
    /// the documented finite call/options profile and independently admit the
    /// original effect, SAF and live handles/unit again before dispatch.
    pub fn wire_bindings(&self) -> &impl MqWireBindings {
        self
    }
}
impl MqWireBindings for MqTrustedBatchPointProfile {
    fn queue_defaults_are_represented(
        &self,
        connection: MqHconn,
        object: Option<MqHobj>,
        lookup: Option<&MqRouteLookup>,
    ) -> bool {
        if connection != self.connection {
            return false;
        }
        // q101870_350–389: cluster binding does not apply to this actual owned
        // noncluster route. 718: read-ahead ignored for nonclient applications.
        // Explicit INPUT_SHARED avoids DefInputOpen. Complete producer has no
        // properties/HMSG. Explicit PMO_SYNC_RESPONSE or the exact retained
        // synchronous queue default is admitted by the full adapter.
        // GET is ONLY the existing complete GMO1 NoWait/Remove profile, whose
        // owner refuses nonempty structured properties/headers, conversion and
        // group/segment forms before adoption. q096715_1167–1204/1515–1520 does
        // NOT establish a generic PropertyControl default. The full adapter
        // restricts options before this gate; partial GET/PUT/general defaults
        // cannot reuse these observations as per-call admission.
        let exact = match &self.target {
            MqTrustedBatchPointTarget::Open { lookup: exact, .. }
            | MqTrustedBatchPointTarget::PutOne { lookup: exact } => {
                object.is_none() && lookup == Some(exact)
            }
            MqTrustedBatchPointTarget::Object(exact) => object == Some(*exact) && lookup.is_none(),
        };
        let synchronous_default = !matches!(self.call, MqMqiCall::Put | MqMqiCall::PutOne)
            || self
                .facts
                .structure
                .catalog
                .native_attributes()
                .is_some_and(|a| {
                    a.queues.iter().any(|q| {
                        q.name == self.facts.queue
                            && q.producer_defaults.as_ref().is_some_and(|d| {
                                d.response == crate::MqNativePutResponse::Synchronous
                            })
                    })
                });
        exact
            && synchronous_default
            && self
                .root
                .runtime
                .service
                .native_point(
                    self.frame,
                    &self.original,
                    self.call,
                    self.connection,
                    &self.target,
                    None,
                )
                .is_ok_and(|fresh| fresh == self.facts)
    }
    fn queue_manager_platform(&self) -> MqWireQueueManagerPlatform {
        MqWireQueueManagerPlatform::Zos
    }
    fn admitted_unit(&self, connection: MqHconn) -> Option<MqMqiUnitOfWork> {
        if connection != self.connection {
            return None;
        }
        self.root
            .runtime
            .service
            .native_structure(self.frame, &self.original, self.call, self.connection)
            .ok()
            .filter(|fresh| *fresh == self.facts.structure)
            .map(|fresh| MqMqiUnitOfWork::Local { unit: fresh.unit })
    }
    fn existing_cursor(&self, _: MqHconn, _: MqHobj) -> Option<u64> {
        None
    }
    fn milliseconds_to_ticks(&self, _: u32) -> Option<u64> {
        None
    }
}
impl MqTrustedBatchFrame {
    /// Fresh same-service/store/frame lookup BEFORE raw OD/MD decode. Only this
    /// initial OPEN/complete GET/PUT/PUT1/CLOSE profile; no default/symbolic
    /// HCONN or body ABI. GET output ResolvedQName remains separately unrepresented.
    pub fn structure_profile(
        &self,
        call: MqMqiCall,
        connection: MqHconn,
    ) -> Result<MqTrustedBatchStructureProfile, HostProblem> {
        self.require_active()?;
        let facts = self.root.runtime.service.native_structure(
            self.frame,
            &self.original,
            call,
            connection,
        )?;
        Ok(MqTrustedBatchStructureProfile {
            root: self.root.clone(),
            frame: self.frame,
            call,
            connection,
            facts,
        })
    }
    /// Bind exact decoded lookup/retained live HOBJ to the captured profile. A
    /// different root/frame/connection/catalog/current unit refuses, without writes.
    pub fn point_profile(
        &self,
        structure: &MqTrustedBatchStructureProfile,
        target: MqTrustedBatchPointTarget,
    ) -> Result<MqTrustedBatchPointProfile, HostProblem> {
        self.require_structure_origin(structure)?;
        let facts = self.root.runtime.service.native_point(
            self.frame,
            &self.original,
            structure.call,
            structure.connection,
            &target,
            None,
        )?;
        if facts.structure != structure.facts {
            return Err(HostProblem::UnknownOutcome);
        }
        Ok(MqTrustedBatchPointProfile {
            root: self.root.clone(),
            frame: self.frame,
            original: self.original.clone(),
            call: structure.call,
            connection: structure.connection,
            target,
            facts,
        })
    }
    fn require_structure_origin(
        &self,
        structure: &MqTrustedBatchStructureProfile,
    ) -> Result<(), HostProblem> {
        self.require_active()?;
        if !Arc::ptr_eq(&self.root, &structure.root) || self.frame != structure.frame {
            return Err(HostProblem::Unauthorized);
        }
        Ok(())
    }
    /// Fresh ABI encoding preflight. Stable facts compare independently of
    /// normal physical catalog/marker/control row-version advancement. These
    /// five calls do not decide/advance the current unit; unit decisions use the
    /// owning current-unit API and require a separately captured profile.
    pub fn recheck_structure_profile(
        &self,
        structure: &MqTrustedBatchStructureProfile,
    ) -> Result<(), HostProblem> {
        self.require_structure_origin(structure)?;
        let fresh = self.root.runtime.service.native_structure(
            self.frame,
            &self.original,
            structure.call,
            structure.connection,
        )?;
        if fresh != structure.facts {
            return Err(HostProblem::UnknownOutcome);
        }
        Ok(())
    }
    /// Fresh output encoding comparison, never a returned request permit. For
    /// CLOSE this checks captured queue/profile/connection/unit without requiring
    /// the now-retired HOBJ or reviving it. Host must independently know the raw
    /// outcome; uncertainty cannot become success/retry from an equal observation.
    pub fn recheck_point_profile(
        &self,
        point: &MqTrustedBatchPointProfile,
    ) -> Result<(), HostProblem> {
        self.require_active()?;
        if !Arc::ptr_eq(&self.root, &point.root) || self.frame != point.frame {
            return Err(HostProblem::Unauthorized);
        }
        let closed = (point.call == MqMqiCall::Close).then_some(&point.facts);
        let fresh = self.root.runtime.service.native_point(
            self.frame,
            &self.original,
            point.call,
            point.connection,
            &point.target,
            closed,
        )?;
        if fresh != point.facts {
            return Err(HostProblem::UnknownOutcome);
        }
        Ok(())
    }
}
