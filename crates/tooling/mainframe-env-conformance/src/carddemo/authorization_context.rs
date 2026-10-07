//! Explicit participant ownership for CardDemo MQ clients and CICS tasks.
use super::*;

pub(super) fn authorization_invocation(
    run: &str,
    granted: bool,
    service_class: ServiceClass,
) -> Result<Invocation, CorpusProblem> {
    let limits = InvocationLimits::default();
    let grants = if granted {
        [
            "host.mq.read",
            "host.mq.write",
            "host.ims.read",
            "host.ims.write",
            "host.db2.read",
            "host.db2.write",
            "host.cics.execute",
            "host.security.authorize",
        ]
        .into_iter()
        .map(|capability| CapabilityId::new(capability, limits))
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(|_| CorpusProblem::new("carddemo.authorization.invocation", "grant invalid"))?
    } else {
        BTreeSet::new()
    };
    Invocation::new(
        RequestId::new(format!("carddemo-auth-request-{run}"), limits).map_err(|_| {
            CorpusProblem::new("carddemo.authorization.invocation", "request invalid")
        })?,
        ExecutionId::new(format!("carddemo-auth-execution-{run}"), limits).map_err(|_| {
            CorpusProblem::new("carddemo.authorization.invocation", "execution invalid")
        })?,
        RunUnitId::new(run, limits)
            .map_err(|_| CorpusProblem::new("carddemo.authorization.invocation", "run invalid"))?,
        None,
        Selector::new("program:CARDDEMO-AUTH", limits).expect("static selector"),
        ArtifactRef::new("carddemo-authorization", limits).expect("static artifact"),
        Principal::new(
            PrincipalId::new("IBMUSER", limits).expect("static principal"),
            grants,
            limits,
        )
        .expect("bounded principal"),
        service_class,
        0,
        1_000_000,
        TraceId::new(format!("carddemo-auth-trace-{run}"), limits).expect("bounded trace"),
        IdempotencyKey::new(format!("carddemo-auth-invocation-{run}"), limits)
            .expect("bounded invocation key"),
        1,
        ResourceLimits::default(),
        BTreeMap::from([
            (
                "mq.host-context".into(),
                BoundedPayload::new(
                    "mainframe-env.mq.host-context@1",
                    b"zos-cics|host-coordinator".to_vec(),
                    limits,
                )
                .expect("bounded CICS MQ context"),
            ),
            (
                "cics.execution-context".into(),
                BoundedPayload::new(
                    "mainframe-env.cics.execution-context@1",
                    b"local".to_vec(),
                    limits,
                )
                .expect("bounded local context"),
            ),
        ]),
        limits,
    )
    .map_err(|_| CorpusProblem::new("carddemo.authorization.invocation", "invocation invalid"))
}

pub(super) fn mq_client_invocation(
    run: &str,
    granted: bool,
    service_class: ServiceClass,
) -> Result<Invocation, CorpusProblem> {
    let mut invocation = authorization_invocation(run, granted, service_class)?;
    invocation.bindings.remove("cics.execution-context");
    invocation.bindings.insert(
        "mq.host-context".into(),
        BoundedPayload::new(
            "mainframe-env.mq.host-context@1",
            b"mqi-client|queue-manager".to_vec(),
            InvocationLimits::default(),
        )
        .expect("bounded MQ client context"),
    );
    Ok(invocation)
}
