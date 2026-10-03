//! Revocable transport of opaque native observations, never selected authority.
use super::*;
use mainframe_env_host_api::MqHobj;
use mainframe_env_host_api::mq_object_route::MqRouteLookup;
use mainframe_env_host_api::mq_raw_layout::MqRawStructureEncoding;
use mainframe_env_host_api::mq_wire_options::{MqWireBindings, MqWireQueueManagerPlatform};
use mainframe_env_interpreter::{MqMqiNativePoint, MqMqiNativePointTarget, MqMqiNativeStructure};

fn observe<T>(
    active: &AtomicBool,
    callback: impl FnOnce() -> Result<T, HostProblem>,
) -> Result<T, HostProblem> {
    if !active.load(Ordering::Acquire) {
        return Err(HostProblem::Unauthorized);
    }
    let result = match catch_unwind(AssertUnwindSafe(callback)) {
        Ok(result) => result,
        Err(_) => {
            active.store(false, Ordering::Release);
            return Err(HostProblem::UnknownOutcome);
        }
    };
    if !active.load(Ordering::Acquire) {
        return Err(HostProblem::Unauthorized);
    }
    result
}

pub(super) fn structure(
    active: Arc<AtomicBool>,
    inner: Arc<dyn MqMqiNativeStructure>,
) -> Result<Arc<dyn MqMqiNativeStructure>, HostProblem> {
    let encoding = observe(&active, || {
        let encoding = inner.encoding();
        inner.recheck()?;
        Ok(encoding)
    })?;
    Ok(Arc::new(Structure {
        active,
        inner,
        encoding,
    }))
}
struct Structure {
    active: Arc<AtomicBool>,
    inner: Arc<dyn MqMqiNativeStructure>,
    encoding: MqRawStructureEncoding,
}
impl MqMqiNativeStructure for Structure {
    fn encoding(&self) -> MqRawStructureEncoding {
        // Immutable scalar observation is not a permit. Recheck remains mandatory
        // before raw decoding and again after all observations/writeback planning.
        self.encoding
    }
    fn recheck(&self) -> Result<(), HostProblem> {
        observe(&self.active, || self.inner.recheck())
    }
    fn point(
        &self,
        target: &MqMqiNativePointTarget,
    ) -> Result<Arc<dyn MqMqiNativePoint>, HostProblem> {
        let (inner, platform) = observe(&self.active, || {
            let inner = self.inner.point(target)?;
            let platform = inner.queue_manager_platform();
            inner.recheck()?;
            Ok((inner, platform))
        })?;
        Ok(Arc::new(Point {
            active: self.active.clone(),
            inner,
            platform,
        }))
    }
}
struct Point {
    active: Arc<AtomicBool>,
    inner: Arc<dyn MqMqiNativePoint>,
    platform: MqWireQueueManagerPlatform,
}
impl MqMqiNativePoint for Point {
    fn descriptor_version(&self) -> Result<i32, HostProblem> {
        observe(&self.active, || self.inner.descriptor_version())
    }
    fn max_message_bytes(&self) -> Result<usize, HostProblem> {
        observe(&self.active, || self.inner.max_message_bytes())
    }
    fn recheck(&self) -> Result<(), HostProblem> {
        observe(&self.active, || self.inner.recheck())
    }
}
impl MqWireBindings for Point {
    fn queue_defaults_are_represented(
        &self,
        connection: MqHconn,
        object: Option<MqHobj>,
        lookup: Option<&MqRouteLookup>,
    ) -> bool {
        observe(&self.active, || {
            Ok(self
                .inner
                .queue_defaults_are_represented(connection, object, lookup))
        })
        .unwrap_or(false)
    }
    fn queue_manager_platform(&self) -> MqWireQueueManagerPlatform {
        // Scalar platform does not authorize use of a revoked point tuple.
        self.platform
    }
    fn admitted_unit(&self, connection: MqHconn) -> Option<MqMqiUnitOfWork> {
        observe(&self.active, || Ok(self.inner.admitted_unit(connection)))
            .ok()
            .flatten()
    }
    fn existing_cursor(&self, connection: MqHconn, object: MqHobj) -> Option<u64> {
        observe(&self.active, || {
            Ok(self.inner.existing_cursor(connection, object))
        })
        .ok()
        .flatten()
    }
    fn milliseconds_to_ticks(&self, milliseconds: u32) -> Option<u64> {
        observe(&self.active, || {
            Ok(self.inner.milliseconds_to_ticks(milliseconds))
        })
        .ok()
        .flatten()
    }
}
