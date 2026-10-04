//! Actual original selected/private fixture flows, not installed LE or official coverage.
use super::*;
use crate::MqBatchLeDllCodesetSource;
use mainframe_env_host_api::mq_md_value::MqMdValue;
use mainframe_env_host_api::mq_mqi::property::mq_property_initial_descriptor;
use std::sync::atomic::AtomicU64;

#[path = "rfh2/failures.rs"]
mod failures;
#[path = "rfh2/quota.rs"]
mod quota;
#[path = "rfh2/restart.rs"]
mod restart;
#[path = "rfh2/source.rs"]
mod source;

#[derive(Default)]
struct Codeset {
    calls: AtomicU64,
    mode: AtomicU64,
    reentry: Mutex<Option<std::sync::Weak<MqService>>>,
}
impl MqBatchLeDllCodesetSource for Codeset {
    fn capture_codeset(
        &self,
        inv: &Invocation,
        original: &MqMqiEffectOccurrence<'_>,
    ) -> Result<i32, HostProblem> {
        assert_eq!(inv.principal.id().as_str(), "TEST");
        assert!(matches!(
            original.envelope().request.call(),
            MqMqiCall::Connect | MqMqiCall::ConnectExtended
        ));
        assert_eq!(original.effect().sequence, original.mutation().sequence);
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(service) = self
            .reentry
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|v| v.upgrade())
        {
            assert!(matches!(
                service.lock_selected(),
                Err(HostProblem::Unsupported)
            ));
            assert!(matches!(service.lock(), Err(HostProblem::Unsupported)));
        }
        match self.mode.load(Ordering::SeqCst) {
            0 => Ok(1208),
            1 => Ok(1047),
            2 => Err(HostProblem::InfrastructureFailure),
            _ => panic!("private source fixture failure"),
        }
    }
}
fn configure(f: &mut Fixture) -> Arc<Codeset> {
    let source = Arc::new(Codeset::default());
    Arc::get_mut(&mut f.service).unwrap().rfh2_source = Some(source.clone());
    source
}
fn md() -> MqMdValue {
    let mut md = mq_property_initial_descriptor();
    let MqMdValue::V1 { fields, .. } = &mut md else {
        panic!()
    };
    fields.encoding = 785;
    fields.coded_char_set_id = 1208;
    fields.msg_id = [255; 24];
    fields.correl_id = [128; 24];
    md
}
fn export(c: MqHconn, h: MqHmsg, options: i32, capacity: usize) -> MqMqiRequest {
    MqMqiRequest::Rfh2(MqMqiRfh2Request::HandleToBuffer {
        connection: c,
        handle: h,
        profile: MqRfh2Profile::ZosBatchUtf8NativeV1,
        options: MqRfh2Options::checked(MqMqiCall::HandleToBuffer, 1, options).unwrap(),
        descriptor: md(),
        name: named("invoice.id"),
        buffer_capacity: capacity,
    })
}
fn import(c: MqHconn, h: MqHmsg, descriptor: MqMdValue, bytes: Vec<u8>) -> MqMqiRequest {
    MqMqiRequest::Rfh2(MqMqiRfh2Request::BufferToHandle {
        connection: c,
        handle: h,
        profile: MqRfh2Profile::ZosBatchUtf8NativeV1,
        options: MqRfh2Options::checked(MqMqiCall::BufferToHandle, 1, 0).unwrap(),
        descriptor,
        buffer: bytes,
    })
}
fn observed(reply: EffectResult) -> MqRfh2Observation {
    let MqMqiOutput::Rfh2Observation(v) = output(reply) else {
        panic!("actual RFH2 output")
    };
    v
}
fn both(sqlite: bool) -> (super::super::restart::Database, Fixture) {
    let db = super::super::restart::Database::new();
    let f = if sqlite {
        Fixture::from_store(db.open())
    } else {
        Fixture::new(false)
    };
    (db, f)
}
#[test]
fn actual_memory_owned_sqlite_original_scalar_export_import_inquire_and_exact_replay() {
    for sqlite in [false, true] {
        let (_db, mut f) = both(sqlite);
        let source = configure(&mut f);
        let policy = install_policy(&mut f);
        let c = f.connect();
        let h = hmsg(f.call(2, create(c)));
        let originals = f
            .rows()
            .into_iter()
            .filter(|v| v.namespace == "mq-delivery-live-v1-queue")
            .collect::<Vec<_>>();
        let mut seq = 3;
        for (kind, bytes, lexical, text) in [
            (MqPropertyType::Null, vec![], " xsi:nil='true'", ""),
            (
                MqPropertyType::ByteString,
                vec![0, 255, 128],
                " dt='bin.hex'",
                "00FF80",
            ),
            (MqPropertyType::ByteString, vec![], " dt='bin.hex'", ""),
            (MqPropertyType::String, vec![], " dt='string'", ""),
            (
                MqPropertyType::String,
                b"  a<&b>  ".to_vec(),
                " dt='string'",
                "  a&lt;&amp;b>  ",
            ),
            (MqPropertyType::Int8, vec![128], " dt='i1'", "-128"),
            (
                MqPropertyType::Int16,
                (-32768i16).to_be_bytes().to_vec(),
                " dt='i2'",
                "-32768",
            ),
            (
                MqPropertyType::Int32,
                i32::MAX.to_be_bytes().to_vec(),
                " dt='i4'",
                "2147483647",
            ),
            (
                MqPropertyType::Int64,
                i64::MIN.to_be_bytes().to_vec(),
                " dt='i8'",
                "-9223372036854775808",
            ),
        ] {
            f.call(seq, set(c, h, "invoice.id", kind, bytes.clone()));
            seq += 1;
            let reply = f.call(seq, export(c, h, 1, 4096));
            seq += 1;
            assert_eq!(pair(&reply), (MqCompletion::Ok, 0));
            let observation = observed(reply);
            let MqRfh2BufferObservation::WrittenPrefix(mut buffer) = observation.buffer else {
                panic!()
            };
            let xml = format!("<invoice content='properties'><id{lexical}>{text}</id></invoice>");
            let padded = (xml.len() + 3) & !3;
            assert_eq!(&buffer[..4], b"RFH ");
            assert_eq!(i32::from_be_bytes(buffer[4..8].try_into().unwrap()), 2);
            assert_eq!(
                i32::from_be_bytes(buffer[8..12].try_into().unwrap()) as usize,
                40 + padded
            );
            assert_eq!(i32::from_be_bytes(buffer[12..16].try_into().unwrap()), 785);
            assert_eq!(i32::from_be_bytes(buffer[16..20].try_into().unwrap()), -2);
            assert_eq!(&buffer[20..28], b"        ");
            assert_eq!(&buffer[28..32], [0; 4]);
            assert_eq!(i32::from_be_bytes(buffer[32..36].try_into().unwrap()), 1208);
            assert_eq!(
                i32::from_be_bytes(buffer[36..40].try_into().unwrap()) as usize,
                padded
            );
            assert_eq!(&buffer[40..40 + xml.len()], xml.as_bytes());
            assert!(buffer[40 + xml.len()..].iter().all(|v| *v == b' '));
            assert_eq!(observation.data_length, Some(buffer.len() as i32));
            let descriptor = observation.descriptor.unwrap();
            let mut expected = md();
            let MqMdValue::V1 { fields, .. } = &mut expected else {
                panic!()
            };
            fields.format = *b"MQHRF2  ";
            assert_eq!(descriptor, expected);
            assert_eq!(
                value(f.call(seq, inquire(c, h, "invoice.id", 128, 128))).copied_value,
                bytes
            );
            seq += 1;
            let destination = hmsg(f.call(seq, create(c)));
            seq += 1;
            assert_ne!(destination, h);
            buffer.extend([0, 255, 128, b'<', b'&']);
            let request = import(c, destination, descriptor.clone(), buffer.clone());
            let captured = request.clone();
            let reply = f.call(seq, request);
            seq += 1;
            assert_eq!(pair(&reply), (MqCompletion::Ok, 0));
            assert_eq!(
                observed(reply),
                MqRfh2Observation {
                    descriptor: None,
                    data_length: Some(buffer.len() as i32),
                    buffer: MqRfh2BufferObservation::Unchanged
                }
            );
            let actual = value(f.call(seq, inquire(c, destination, "invoice.id", 128, 128)));
            seq += 1;
            assert_eq!(actual.kind, kind);
            assert_eq!(actual.copied_value, bytes);
            assert_eq!(actual.descriptor, MqPropertyDescriptor::source_default());
            assert_eq!(actual.returned_encoding, 785);
            assert_eq!(
                actual.returned_ccsid,
                if kind == MqPropertyType::String {
                    Some(1208)
                } else {
                    None
                }
            );
            assert_eq!(captured, import(c, destination, descriptor, buffer));
            assert_eq!(
                value(f.call(seq, inquire(c, destination, "Root.MQMD.MsgId", 128, 24)))
                    .copied_value,
                vec![255; 24]
            );
            seq += 1;
        }
        assert_eq!(source.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            f.rows()
                .into_iter()
                .filter(|v| v.namespace == "mq-delivery-live-v1-queue")
                .collect::<Vec<_>>(),
            originals
        );
        assert!(
            policy
                .observed
                .lock()
                .unwrap()
                .iter()
                .all(|(p, r)| p == f.inv.principal.id()
                    && r.class == EnterpriseResourceClass::MqUnitOfWork
                    && r.name.as_str() == "CURRENT")
        );
    }
}
#[test]
fn actual_required_short_missing_delete_and_descriptor_only_import_are_not_success_placeholders() {
    for sqlite in [false, true] {
        let (_db, mut f) = both(sqlite);
        configure(&mut f);
        let c = f.connect();
        let h = hmsg(f.call(2, create(c)));
        f.call(
            3,
            set(
                c,
                h,
                "invoice.id",
                MqPropertyType::String,
                b" abc ".to_vec(),
            ),
        );
        let short = f.call(4, export(c, h, 3, 0));
        assert_eq!(pair(&short), (MqCompletion::Failed, 2469));
        let short = observed(short);
        assert!(short.data_length.unwrap() > 0);
        assert!(short.descriptor.is_none());
        assert_eq!(short.buffer, MqRfh2BufferObservation::Unchanged);
        assert_eq!(
            value(f.call(5, inquire(c, h, "invoice.id", 128, 128))).copied_value,
            b" abc "
        );
        let deleted = f.call(6, export(c, h, 3, short.data_length.unwrap() as usize));
        assert_eq!(pair(&deleted), (MqCompletion::Ok, 0));
        let missing = f.call(7, export(c, h, 3, 4096));
        assert_eq!(pair(&missing), (MqCompletion::Failed, 2471));
        assert_eq!(
            observed(missing),
            MqRfh2Observation {
                descriptor: None,
                data_length: Some(0),
                buffer: MqRfh2BufferObservation::Unchanged
            }
        );
        assert_eq!(
            value(f.call(8, inquire(c, h, "Root.MQMD.MsgId", 128, 24))).copied_value,
            vec![0; 24]
        );
        let reply = f.call(9, import(c, h, md(), vec![]));
        assert_eq!(pair(&reply), (MqCompletion::Ok, 0));
        assert_eq!(observed(reply).data_length, Some(0));
        assert_eq!(
            value(f.call(10, inquire(c, h, "Root.MQMD.MsgId", 128, 24))).copied_value,
            vec![255; 24]
        );
    }
}
