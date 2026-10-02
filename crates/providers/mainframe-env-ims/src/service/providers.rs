//! Existing IMS provider factory, shared by database, SSA and recovery routes.
use super::*;

struct ImsProvider {
    service: Arc<ImsService>,
    descriptor: CapabilityDescriptor,
}

impl HostProvider for ImsProvider {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }

    fn invoke(&self, invocation: &Invocation, effect: EffectRequest) -> EffectResult {
        let sequence = effect.sequence;
        let resolution_tick = effect.deadline_tick.max(invocation.deadline_tick);
        let outcome = match effect.request {
            HostRequest::ImsNavigation(request) => self
                .service
                .execute_operands_at(
                    invocation,
                    &request.request,
                    resolution_tick,
                    Some(&request),
                )
                .map(HostResult::Ims),
            HostRequest::Ims(request) => self
                .service
                .execute_at(invocation, &request, resolution_tick)
                .map(HostResult::Ims),
            _ => Err(HostProblem::Malformed),
        };
        EffectResult { sequence, outcome }
    }
}

pub fn ims_providers(
    service: Arc<ImsService>,
    limits: InvocationLimits,
) -> Vec<Arc<dyn HostProvider>> {
    ["host.ims.read", "host.ims.write"]
        .into_iter()
        .map(|capability| {
            Arc::new(ImsProvider {
                service: service.clone(),
                descriptor: CapabilityDescriptor {
                    capability: CapabilityId::new(capability, limits)
                        .expect("static IMS capability"),
                    provider_id: "mainframe-env-ims".into(),
                    generation: "1".into(),
                    request_schema: "mainframe-env.ims-request@1".into(),
                    result_schema: "mainframe-env.ims-result@1".into(),
                    max_request_bytes: 4 * 1024 * 1024,
                    max_result_bytes: 16 * 1024 * 1024,
                    ready: true,
                },
            }) as Arc<dyn HostProvider>
        })
        .collect()
}
