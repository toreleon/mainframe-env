//! Private admission fixture only; no selected service/SAF/participant credit.
use super::*;
use mainframe_env_host_api::mq_md_value::*;
use mainframe_env_host_api::mq_object_route::MqRouteLookup;

#[test]
fn complete_message_forms_are_explicitly_unsupported_without_rewriting_original_effect() {
    let invocation = invocation();
    let p = provider();
    let mut registry = mainframe_env_host_api::MqHandleRegistry::new(7, 8).unwrap();
    let connection = registry
        .connect(owner(), MqHandleSharing::NonShared)
        .unwrap();
    let object = registry.create_object(owner(), connection).unwrap();
    let descriptor = MqMdValue::V1 {
        characters: MqMdCharacterEncoding::AsciiCompatible,
        fields: MqMdFields {
            struc_id: *b"MD  ",
            report: i32::MIN,
            msg_type: -7,
            expiry: -42,
            feedback: -13,
            encoding: -19,
            coded_char_set_id: -23,
            format: [0; 8],
            priority: -5,
            persistence: -9,
            msg_id: [0; 24],
            correl_id: [255; 24],
            backout_count: -1,
            reply_to_q: [32; 48],
            reply_to_q_mgr: [32; 48],
            user_identifier: [0; 12],
            accounting_token: [255; 32],
            appl_identity_data: [0; 32],
            put_appl_type: -1,
            put_appl_name: [0; 28],
            put_date: [0; 8],
            put_time: [0; 8],
            appl_origin_data: [0; 4],
        },
    };
    let put = MqMqiFullPut {
        message: MqFullMessage {
            descriptor: descriptor.clone(),
            body: vec![0, 255],
            properties: vec![],
        },
        message_handle: None,
        context: MqMqiMessageContext::Default,
        options: MqMqiOptions::ContractDefault,
        unit: MqMqiUnitOfWork::NoSyncpoint,
    };
    let get = MqMqiFullGet {
        connection,
        object,
        descriptor,
        mode: mainframe_env_host_api::MqGetMode::Remove,
        wait: mainframe_env_host_api::MqWait::NoWait,
        truncation: mainframe_env_host_api::MqTruncation::Reject,
        buffer_capacity: 2,
        message_handle: None,
        options: MqMqiOptions::ContractDefault,
        unit: MqMqiUnitOfWork::NoSyncpoint,
    };
    for request in [
        MqMqiRequest::QualifiedFullGet(get.clone()),
        MqMqiRequest::FullGet(get),
        MqMqiRequest::FullPut {
            connection,
            object,
            put: put.clone(),
        },
        MqMqiRequest::FullPutOne {
            connection,
            lookup: MqRouteLookup::Queue {
                name: mainframe_env_host_api::mq_object_route::MqRouteName::new("QUEUE").unwrap(),
                manager: None,
                dynamic_pattern: None,
            },
            alternate_user: None,
            put,
        },
    ] {
        let mut envelope = envelope();
        envelope.request = request;
        let original = effect(&invocation, &mutation(), &envelope);
        let before = original.clone();
        let scope = scope(&invocation, owner(), &original, &p).unwrap();
        // Admission keeps the original occurrence pending. The selected service
        // refuses this disposition as Unsupported, before any queue transition.
        let admitted = admit_mqi(&scope, &invocation, 10).unwrap();
        assert!(matches!(
            admitted,
            MqMqiAdmission::Pending {
                _reason: MqMqiPending::StructureAndWireMapping,
                ..
            }
        ));
        assert_eq!(original, before);
        assert_eq!(registry.validate_connection(owner(), connection), Ok(()));
    }
}
