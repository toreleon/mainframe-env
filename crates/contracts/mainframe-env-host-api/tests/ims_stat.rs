use mainframe_env_host_api::*;

#[test]
fn ims_stat_exact_function_extension_and_specific_capacity_classes() {
    use ImsStatisticsFamily::{Dbas, Dbes, Vbas, Vbes};
    use ImsStatisticsFormat::{Full, Osam, Summary, Unformatted};
    for (family, format, minimum) in [
        (Dbas, Full, 360),
        (Vbas, Full, 360),
        (Dbes, Full, 600),
        (Vbes, Full, 600),
        (Dbas, Summary, 180),
        (Vbas, Summary, 180),
        (Dbes, Summary, 360),
        (Vbes, Summary, 360),
        (Dbas, Unformatted, 72),
        (Vbas, Unformatted, 72),
        (Dbes, Unformatted, 84),
        (Vbes, Unformatted, 104),
        (Dbes, Osam, 360),
    ] {
        let function = ImsStatisticsFunction {
            family,
            format,
            extended: false,
        };
        assert_eq!(function.minimum_io_area_bytes(), Ok(minimum));
        for size in [0, minimum - 1, minimum, minimum + 1] {
            let req = ImsSystemRequest {
                context: ImsExecutionContext::DbDc,
                syntax: ImsCallSyntax::Call,
                call: ImsSystemCall::StatisticsV2 {
                    function,
                    io_area_bytes: size,
                },
            };
            assert_eq!(
                req.validate(HostLimits::default()),
                if size < minimum {
                    Err(HostProblem::Malformed)
                } else {
                    Ok(())
                },
                "{function:?}/{size}"
            );
        }
    }
    for family in [Dbas, Dbes, Vbas, Vbes] {
        for format in [Full, Osam, Summary, Unformatted] {
            let function = ImsStatisticsFunction {
                family,
                format,
                extended: true,
            };
            let expected = if family == Dbes && format != Summary {
                Ok(())
            } else {
                Err(HostProblem::Malformed)
            };
            assert_eq!(function.validate(), expected);
            assert_eq!(
                function.minimum_io_area_bytes(),
                expected.and(Err(HostProblem::Unsupported))
            );
        }
    }
    for family in [Vbas, Vbes] {
        assert_eq!(
            ImsStatisticsFunction {
                family,
                format: Osam,
                extended: false
            }
            .validate(),
            Err(HostProblem::Malformed)
        );
    }
    assert_eq!(
        ImsStatisticsFunction {
            family: Dbas,
            format: Osam,
            extended: false
        }
        .minimum_io_area_bytes(),
        Err(HostProblem::Unsupported)
    );
}

#[test]
fn ims_stat_historical_runtime_serialization_stays_exact() {
    let old = r#"{"directory":null,"dedb_areas":[],"buffer_pools":[{"name":"OSAM","kind":"Osam","buffer_bytes":4096,"buffers":8}]}"#;
    let runtime: ImsSystemRuntimeDefinition = serde_json::from_str(old).unwrap();
    assert!(runtime.vsam_subpools_v2.is_empty());
    assert_eq!(serde_json::to_string(&runtime).unwrap(), old);
    let invalid = r#"{"subpool":"S","lsr_pool":1,"definition_order":0,"subpool_type":"Data","guessed_dsr":true}"#;
    assert!(serde_json::from_str::<ImsVsamSubpoolMetadata>(invalid).is_err());
}
