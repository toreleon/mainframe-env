//! Closed configured source facet. Rust setup is trusted integration, not JES proof.
use super::*;
use mainframe_env_host_api::mq_md_value::{MqMdCharacterEncoding, MqMdFields};
use mainframe_env_store_api::PlatformStore;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Checked physical Gregorian UTC observation. Seconds60 is explicitly refused;
/// the adapter must choose a genuine available non-leap instant, never normalize.
pub struct ProducerGmt {
    date: [u8; 8],
    time: [u8; 8],
}
impl ProducerGmt {
    /// Check one exact physical UTC observation without normalizing its calendar
    /// or leap seconds. This value does not attest where the host obtained it.
    pub fn new(
        year: u16,
        month: u8,
        day: u8,
        hour: u8,
        minute: u8,
        second: u8,
        hundredths: u8,
    ) -> Result<Self, HostProblem> {
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => 0,
        };
        if year == 0
            || year > 9999
            || day == 0
            || day > days
            || hour > 23
            || minute > 59
            || second > 59
            || hundredths > 99
        {
            return Err(HostProblem::Malformed);
        }
        Ok(Self {
            date: format!("{year:04}{month:02}{day:02}")
                .into_bytes()
                .try_into()
                .map_err(|_| HostProblem::Malformed)?,
            time: format!("{hour:02}{minute:02}{second:02}{hundredths:02}")
                .into_bytes()
                .try_into()
                .map_err(|_| HostProblem::Malformed)?,
        })
    }
}
/// Trusted batch observation, not constructed from Invocation binding/name text.
pub struct ProducerBatchContext {
    job: String,
    user: Option<String>,
    accounting: Option<[u8; 32]>,
}
impl ProducerBatchContext {
    /// Check the finite batch observation shape; the configured trusted host
    /// port must independently attest its provenance and optional observations.
    pub fn new(
        job: String,
        user: Option<String>,
        accounting: Option<[u8; 32]>,
    ) -> Result<Self, HostProblem> {
        let token = |s: &str, max| {
            !s.is_empty()
                && s.len() <= max
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'$' | b'#' | b'@'))
        };
        if !token(&job, 8) || user.as_ref().is_some_and(|s| !token(s, 12)) {
            return Err(HostProblem::Malformed);
        }
        Ok(Self {
            job,
            user,
            accounting,
        })
    }
}
/// Host-owned synchronous bounded port. Observation errors are NOT absence.
/// Adapter must derive context from independently admitted batch provenance.
/// Calls must not block/reenter any service or perform publication/cleanup.
pub trait ProducerSource: Send + Sync {
    /// Trusted host adapter uses the existing owned character encoder for this
    /// declared structure profile. No context/time sample or mutation here.
    fn encode_structure(
        &self,
        text: &str,
        characters: MqMdCharacterEncoding,
    ) -> Result<Vec<u8>, HostProblem>;
    /// Independently verify this exact original host frame remains live. Caller
    /// binding equality, principal text and matching copied store rows are not proof.
    fn check_live(&self, invocation: &Invocation) -> Result<(), HostProblem>;
    /// Real physical UTC/GMT only. NoContext-only adapters refuse by default;
    /// logical ticks, fabricated timestamps or silently normalized leap seconds
    /// cannot substitute for a host clock observation.
    fn physical_gmt(&self, _: &Invocation) -> Result<ProducerGmt, HostProblem> {
        Err(HostProblem::Unsupported)
    }
    /// Real independently admitted batch context. Absence of optional user or
    /// accounting differs from a source error. NoContext-only adapters refuse.
    fn batch_context(&self, _: &Invocation) -> Result<ProducerBatchContext, HostProblem> {
        Err(HostProblem::Unsupported)
    }
}
pub(in crate::service) struct ProducerSources {
    store: Arc<dyn PlatformStore>,
    source: Arc<dyn ProducerSource>,
}
impl MqService {
    /// Only unique, inactive Rust-owned setup can attach provenance ports. This
    /// private precondition is NOT fulfilled by caller JSON or matching rows.
    /// Real installed/JES construction remains a manager-owned integration seam.
    pub(crate) fn configure_producer_sources(
        service: &mut Arc<Self>,
        store: &Arc<dyn PlatformStore>,
        source: Arc<dyn ProducerSource>,
    ) -> Result<(), HostProblem> {
        let service = Arc::get_mut(service).ok_or(HostProblem::Unsupported)?;
        let physical = service
            .selected_store
            .as_ref()
            .ok_or(HostProblem::Unsupported)?;
        if !Arc::ptr_eq(store, physical) || service.producer_sources.is_some() {
            return Err(HostProblem::Unauthorized);
        }
        {
            let guard = service.lock_selected()?;
            let rich_state::StoredAuthority::Rich(s) = &*guard else {
                return Err(HostProblem::Unsupported);
            };
            if s.runtime.is_some() {
                return Err(HostProblem::Unsupported);
            }
            if s.catalog.native_attributes().is_none() {
                return Err(HostProblem::Unsupported);
            }
        }
        service.producer_sources = Some(ProducerSources {
            store: store.clone(),
            source,
        });
        Ok(())
    }
}
struct Sampling<'a>(&'a AtomicBool);
impl Drop for Sampling<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}
fn call<T>(
    service: &MqService,
    f: impl FnOnce(&dyn ProducerSource) -> Result<T, HostProblem>,
) -> Result<T, HostProblem> {
    let configured = service
        .producer_sources
        .as_ref()
        .ok_or(HostProblem::Unsupported)?;
    if !Arc::ptr_eq(
        &configured.store,
        service
            .selected_store
            .as_ref()
            .ok_or(HostProblem::Unsupported)?,
    ) {
        return Err(HostProblem::Unauthorized);
    }
    if service.producer_sampling.swap(true, Ordering::SeqCst) {
        return Err(HostProblem::Unsupported);
    }
    let _sampling = Sampling(&service.producer_sampling);
    catch_unwind(AssertUnwindSafe(|| f(&*configured.source)))
        .map_err(|_| HostProblem::ProviderFailure)?
}
pub(super) fn check_live(service: &MqService, invocation: &Invocation) -> Result<(), HostProblem> {
    call(service, |s| s.check_live(invocation))
}
pub(super) fn recheck(
    service: &MqService,
    frame: FrameLease,
    invocation: &Invocation,
    admitted: &crate::mqi_admission::MqMqiAdmitted<'_>,
    directory: &MqLifecycleDirectory,
    now: u64,
) -> Result<(), HostProblem> {
    if !matches!(
        admitted.envelope.request,
        MqMqiRequest::FullPut { .. }
            | MqMqiRequest::FullPutOne { .. }
            | MqMqiRequest::QualifiedFullGet(_)
    ) {
        return Ok(());
    }
    admitted.recheck_controls(now)?;
    directory.owner_for(frame, invocation, now)?;
    call(service, |s| s.check_live(invocation))
}
pub(super) fn fixed<const N: usize>(
    service: &MqService,
    text: &str,
    characters: MqMdCharacterEncoding,
) -> Result<[u8; N], HostProblem> {
    if !text.is_ascii() || text.len() > N {
        return Err(HostProblem::Malformed);
    }
    let blank = match characters {
        MqMdCharacterEncoding::AsciiCompatible => b' ',
        MqMdCharacterEncoding::OwnedCp037 => 0x40,
    };
    let bytes = if text.is_empty() {
        Vec::new()
    } else {
        call(service, |s| s.encode_structure(text, characters))?
    };
    if bytes.len() != text.len()
        || (characters == MqMdCharacterEncoding::AsciiCompatible && bytes != text.as_bytes())
    {
        return Err(HostProblem::Malformed);
    }
    let mut out = [blank; N];
    out[..bytes.len()].copy_from_slice(&bytes);
    Ok(out)
}
pub(super) fn context(
    service: &MqService,
    invocation: &Invocation,
    context: &MqMqiMessageContext,
    characters: MqMdCharacterEncoding,
    fields: &mut MqMdFields,
) -> Result<(), HostProblem> {
    fields.user_identifier = fixed(service, "", characters)?;
    fields.accounting_token = [0; 32];
    fields.appl_identity_data = fixed(service, "", characters)?;
    fields.put_appl_type = 0;
    fields.put_appl_name = fixed(service, "", characters)?;
    fields.put_date = fixed(service, "", characters)?;
    fields.put_time = fixed(service, "", characters)?;
    fields.appl_origin_data = fixed(service, "", characters)?;
    if *context == MqMqiMessageContext::NoContext {
        return Ok(());
    }
    if *context != MqMqiMessageContext::Default {
        return Err(HostProblem::Unsupported);
    }
    let (gmt, batch) = call(service, |s| {
        s.check_live(invocation)?;
        let gmt = s.physical_gmt(invocation)?;
        let batch = s.batch_context(invocation)?;
        s.check_live(invocation)?;
        Ok((gmt, batch))
    })?;
    fields.user_identifier = fixed(service, batch.user.as_deref().unwrap_or(""), characters)?;
    fields.accounting_token = batch.accounting.unwrap_or([0; 32]);
    // Retained q090310_19–27 MQAT_MVS/MQAT_ZOS=2. Actual batch provenance,
    // not application numeric fields or MQAT_DEFAULT, supplies this context.
    fields.put_appl_type = 2;
    fields.put_appl_name = fixed(service, &batch.job, characters)?;
    fields.put_date = fixed(
        service,
        std::str::from_utf8(&gmt.date).map_err(|_| HostProblem::Malformed)?,
        characters,
    )?;
    fields.put_time = fixed(
        service,
        std::str::from_utf8(&gmt.time).map_err(|_| HostProblem::Malformed)?,
        characters,
    )?;
    Ok(())
}
