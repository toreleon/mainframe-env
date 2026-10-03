use super::*;

#[test]
fn ims_stat_old_wire_and_additive_capacity_variant_are_frozen() {
    // Independently framed from EFFECT-CANONICAL-V1, not IBM raw STAT bytes.
    let function = ImsStatisticsFunction {
        family: ImsStatisticsFamily::Dbas,
        format: ImsStatisticsFormat::Full,
        extended: false,
    };
    let old = ImsSystemCall::Statistics { function };
    assert_eq!(
        hex(&bytes(&old, b"")),
        "41010d00000000000000496d7353797374656d43616c6c010a0000000000000053746174697374696373010000000000000001080000000000000066756e6374696f6e40011500000000000000496d735374617469737469637346756e6374696f6e0300000000000000010800000000000000657874656e6465640301060000000000000066616d696c7941011300000000000000496d735374617469737469637346616d696c79010400000000000000446261730000000000000000010600000000000000666f726d617441011300000000000000496d7353746174697374696373466f726d617401040000000000000046756c6c0000000000000000"
    );
    assert_eq!(
        hex(&bytes(&ImsSystemResult::Statistics { pool: None }, b"")),
        "41010f00000000000000496d7353797374656d526573756c74010a00000000000000537461746973746963730100000000000000010400000000000000706f6f6c20"
    );
    let v2 = ImsSystemCall::StatisticsV2 {
        function,
        io_area_bytes: 360,
    };
    assert_eq!(
        hex(&bytes(&v2, b"")),
        "41010d00000000000000496d7353797374656d43616c6c010c00000000000000537461746973746963735632020000000000000001080000000000000066756e6374696f6e40011500000000000000496d735374617469737469637346756e6374696f6e0300000000000000010800000000000000657874656e6465640301060000000000000066616d696c7941011300000000000000496d735374617469737469637346616d696c79010400000000000000446261730000000000000000010600000000000000666f726d617441011300000000000000496d7353746174697374696373466f726d617401040000000000000046756c6c0000000000000000010d00000000000000696f5f617265615f62797465731268010000"
    );
    assert_ne!(bytes(&old, b""), bytes(&v2, b""));
    assert_ne!(
        bytes(&v2, b""),
        bytes(
            &ImsSystemCall::StatisticsV2 {
                function,
                io_area_bytes: 361
            },
            b""
        )
    );
    let full = ImsSystemResult::StatisticsV2 {
        function,
        observation: Some(ImsStatisticsObservationV2::Totals {
            buffers: 8,
            storage_bytes: 32768,
            reads: 12,
            writes: 3,
        }),
    };
    let mut summary = full.clone();
    if let ImsSystemResult::StatisticsV2 { function, .. } = &mut summary {
        function.format = ImsStatisticsFormat::Summary;
    }
    assert_ne!(bytes(&full, b""), bytes(&summary, b""));
    assert_eq!(
        serde_json::from_str::<ImsSystemResult>("{\"Statistics\":{\"pool\":null}}").unwrap(),
        ImsSystemResult::Statistics { pool: None }
    );
}
