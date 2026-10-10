//! Finite first-party API compatibility controls; no official or licensed row credit.

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "explicit finite fixture requires accepted external bindings, pinned Python, source and fresh run leaf"]
async fn finite_public_client_compatibility() {
    let result = controls::exercise().await;
    assert!(
        result.is_ok(),
        "finite public client compatibility failed: {result:?}"
    );
}

#[cfg(test)]
mod controls {
    // These literals and mutations were frozen before implementing the assertion helpers.
    #[test]
    fn pure_json_and_transport_controls() {
        let good = br#"{"success":true,"exitCode":0,"data":"IEFBR14"}"#;
        assert!(envelope(good, 0, true).is_ok());
        for bytes in [
            b"".as_slice(),
            b"{}",
            b"null",
            b"{",
            b"{} {}",
            br#"{"success":1,"exitCode":0,"data":"IEFBR14"}"#,
            br#"{"success":true,"exitCode":"0","data":"IEFBR14"}"#,
            br#"{"success":true,"exitCode":0.0,"data":"IEFBR14"}"#,
            br#"{"success":false,"exitCode":0,"data":"IEFBR14"}"#,
            br#"{"success":true,"exitCode":0,"data":{}}"#,
            br#"{"success":true,"exitCode":0,"data":[]}"#,
            br#"{"success":true,"exitCode":0,"data":null}"#,
            br#"{"success":true,"exitCode":0,"data":""}"#,
            br#"{"success":true,"exitCode":0,"data":"IEFBR14","error":{}}"#,
        ] {
            assert!(envelope(bytes, 0, true).is_err(), "admitted {bytes:?}");
        }
        assert!(envelope(good, 1, true).is_err());
        assert!(transport(0, Some(b"0\n"), b"").is_ok());
        for (exit, child, error) in [
            (1, Some(b"0\n".as_slice()), b"".as_slice()),
            (0, None, b""),
            (0, Some(b""), b""),
            (0, Some(b"0\n"), b"timeout"),
            (0, Some(b"0\n1\n"), b""),
            (0, Some(b"0"), b""),
        ] {
            assert!(transport(exit, child, error).is_err());
        }
    }

    #[test]
    fn pure_job_identity_and_terminal_controls() {
        let active = serde_json::json!({"jobid":"JOB00001","jobname":"PBCLNT01",
        "owner":"IBMUSER","status":"ACTIVE","retcode":null});
        assert!(job(&active, Some("JOB00001"), false).is_ok());
        let terminal = serde_json::json!({"jobid":"JOB00001","jobname":"PBCLNT01",
        "owner":"IBMUSER","status":"OUTPUT","retcode":"CC 0000"});
        assert!(job(&terminal, Some("JOB00001"), true).is_ok());
        for (field, value) in [
            ("jobid", serde_json::json!("JOB00002")),
            ("jobname", serde_json::json!("OTHER")),
            ("owner", serde_json::json!("OTHERUSR")),
            ("status", serde_json::json!("ACTIVE")),
            ("retcode", serde_json::json!(null)),
            ("retcode", serde_json::json!("CC 0004")),
        ] {
            let mut changed = terminal.clone();
            changed[field] = value;
            assert!(job(&changed, Some("JOB00001"), true).is_err());
        }
        for id in [
            "",
            "JOB00000",
            "JOB1",
            "JOB000001",
            "JOB999999999",
            "OTHER00001",
        ] {
            let mut changed = active.clone();
            changed["jobid"] = serde_json::json!(id);
            assert!(job(&changed, None, false).is_err());
        }
        let mut changed = active;
        changed["retcode"] = serde_json::json!("CC 0000");
        assert!(job(&changed, None, false).is_err());
    }

    #[test]
    fn pure_selected_content_and_file_controls() {
        assert!(content(&serde_json::json!("IEFBR14")).is_ok());
        for value in [
            serde_json::json!("IEFBR14\n"),
            serde_json::json!("IEFBR14 "),
            serde_json::json!("OTHER"),
            serde_json::json!(["IEFBR14"]),
            serde_json::json!(["IEFBR14", "IEFBR14"]),
        ] {
            assert!(content(&value).is_err());
        }
        let selected = serde_json::json!({"id":2,"jobid":"JOB00001","jobname":"PBCLNT01",
        "ddname":"STEP1:SYSPRINT","record-count":1,"byte-count":7});
        assert_eq!(
            selected_file(&serde_json::json!([selected]), "JOB00001").unwrap(),
            2
        );
        assert!(selected_file(&serde_json::json!([]), "JOB00001").is_err());
        assert!(selected_file(&serde_json::json!([selected, selected]), "JOB00001").is_err());
        for (field, value) in [
            ("id", serde_json::json!(2.0)),
            ("id", serde_json::json!(64)),
            ("jobid", serde_json::json!("JOB00002")),
            ("jobname", serde_json::json!("OTHER")),
            ("record-count", serde_json::json!(2)),
            ("byte-count", serde_json::json!(8)),
            ("ddname", serde_json::json!("SYSPRINT")),
        ] {
            let mut changed = selected.clone();
            changed[field] = value;
            assert!(selected_file(&serde_json::json!([changed]), "JOB00001").is_err());
        }
        let mut missing = selected;
        missing.as_object_mut().unwrap().remove("byte-count");
        assert!(selected_file(&serde_json::json!([missing]), "JOB00001").is_err());
    }

    #[test]
    fn pure_request_vector_controls() {
        let first = observed(
            "GET",
            "/zosmf/restjobs/jobs?owner=*&jobid=JOB00001",
            200,
            b"[]",
        );
        let second = observed(
            "GET",
            "/zosmf/restjobs/jobs/PBCLNT01/JOB00001/files",
            200,
            b"[]",
        );
        let expected = [
            ("GET", first.uri.as_str(), 200),
            ("GET", second.uri.as_str(), 200),
        ];
        assert!(request_vector(&[first.clone(), second.clone()], &expected).is_ok());
        for actual in [
            vec![],
            vec![first.clone()],
            vec![second.clone(), first.clone()],
            vec![first.clone(), second.clone(), second.clone()],
        ] {
            assert!(request_vector(&actual, &expected).is_err());
        }
        for (method, uri, status) in [
            ("PUT", first.uri.as_str(), 200),
            ("GET", "/zosmf/restjobs/jobs/PBCLNT01/JOB00001", 200),
            ("GET", first.uri.as_str(), 401),
        ] {
            assert!(
                request_vector(
                    &[observed(method, uri, status, b"[]"), second.clone()],
                    &expected
                )
                .is_err()
            );
        }
    }

    #[test]
    fn pure_observed_refusal_controls() {
        let denied = serde_json::json!({"success":false,"exitCode":1,
        "error":{"causeErrors":"Zero jobs were returned."}});
        let good = observed(
            "GET",
            "/zosmf/restjobs/jobs?owner=*&jobid=JOB00001",
            200,
            b"[]",
        );
        assert!(
            refusal(
                "other-status",
                &denied,
                std::slice::from_ref(&good),
                "JOB00001",
                None
            )
            .is_ok()
        );
        assert!(refusal("other-status", &denied, &[], "JOB00001", None).is_err());
        for (status, body) in [
            (401, b"[]".as_slice()),
            (200, b"{}"),
            (200, br#"[{"owner":"IBMUSER"}]"#),
            (500, b"[]"),
        ] {
            let actual = observed("GET", &good.uri, status, body);
            assert!(refusal("other-status", &denied, &[actual], "JOB00001", None).is_err());
        }
        let extra = observed(
            "GET",
            "/zosmf/restjobs/jobs/PBCLNT01/JOB00001/files",
            403,
            b"{}",
        );
        assert!(
            refusal(
                "other-files",
                &denied,
                &[good.clone(), extra],
                "JOB00001",
                None
            )
            .is_err()
        );
        let mut fake = denied.clone();
        fake["error"]["causeErrors"] = serde_json::json!("timeout");
        assert!(
            refusal(
                "other-status",
                &fake,
                std::slice::from_ref(&good),
                "JOB00001",
                None
            )
            .is_err()
        );
        for leaked in [
            serde_json::json!({"owner":"IBMUSER"}),
            serde_json::json!("IEFBR14"),
            serde_json::json!([{"ddname":"STEP1:SYSPRINT"}]),
        ] {
            let mut leak = denied.clone();
            leak["data"] = leaked;
            assert!(
                refusal(
                    "other-content",
                    &leak,
                    std::slice::from_ref(&good),
                    "JOB00001",
                    Some(2)
                )
                .is_err()
            );
        }
        assert!(envelope(&serde_json::to_vec(&denied).unwrap(), 0, false).is_err());
        for action in ["bad-password", "other-owner-list"] {
            let status = if action == "bad-password" { 401 } else { 403 };
            let method = if status == 401 { "PUT" } else { "GET" };
            let uri = if status == 401 {
                "/zosmf/restjobs/jobs"
            } else {
                "/zosmf/restjobs/jobs?owner=IBMUSER&prefix=PBCLNT01"
            };
            let mut real = observed(method, uri, status, br#"{"category":"not_authorized"}"#);
            if status == 401 {
                real.request = JCL.to_vec();
            }
            assert!(refusal(action, &denied, &[real.clone()], "JOB00001", None).is_ok());
            real.status = 200;
            assert!(refusal(action, &denied, &[real], "JOB00001", None).is_err());
        }
    }

    #[test]
    fn pure_opaque_state_and_artifact_mutation_controls() {
        let before = sample_snapshot();
        assert!(unchanged(&before, &before).is_ok());
        let mut variants = Vec::new();
        let mut changed = before.clone();
        changed.jobs.clear();
        variants.push(changed);
        let mut changed = before.clone();
        changed.jobs.push(changed.jobs[0].clone());
        variants.push(changed);
        let mut changed = before.clone();
        changed.jobs[0].version += 1;
        variants.push(changed);
        let mut changed = before.clone();
        changed.jobs[0].priority += 1;
        variants.push(changed);
        let mut changed = before.clone();
        changed.more = true;
        variants.push(changed);
        let mut changed = before.clone();
        changed.rows.clear();
        variants.push(changed);
        let mut changed = before.clone();
        changed.rows[0].version += 1;
        variants.push(changed);
        let mut changed = before.clone();
        changed.rows[0].payload.push(0);
        variants.push(changed);
        let mut changed = before.clone();
        changed.rows[0].key.push('x');
        variants.push(changed);
        let mut changed = before.clone();
        changed.rows[0].namespace.push('x');
        variants.push(changed);
        let mut changed = before.clone();
        changed.rows.push(changed.rows[0].clone());
        variants.push(changed);
        let mut changed = before.clone();
        changed.artifacts.clear();
        variants.push(changed);
        let mut changed = before.clone();
        changed
            .artifacts
            .insert("extra".into(), Entry::File(vec![7]));
        variants.push(changed);
        let mut changed = before.clone();
        changed
            .artifacts
            .insert("object".into(), Entry::File(vec![8]));
        variants.push(changed);
        for changed in variants {
            assert!(unchanged(&before, &changed).is_err());
        }
    }

    #[test]
    fn pure_cleanup_and_completion_controls() {
        assert!(cleanup_result(Ok(()), &[]).is_ok());
        assert!(cleanup_result(Err("body failed".into()), &[]).is_err());
        assert!(cleanup_result(Ok(()), &["delete failed".into()]).is_err());
        assert!(
            cleanup_result(Err("body failed".into()), &["join failed".into()])
                .unwrap_err()
                .contains("body failed")
        );
        assert!(complete_run(10, 10, 12, 1).is_ok());
        assert!(complete_run(21, 21, 23, 12).is_ok());
        for (starts, waits, requests, polls) in [
            (9, 9, 11, 0),
            (22, 22, 24, 13),
            (10, 9, 12, 1),
            (10, 10, 13, 1),
            (11, 11, 12, 1),
        ] {
            assert!(complete_run(starts, waits, requests, polls).is_err());
        }
    }

    // Independent request-echo literals: pinned SDK format, synthetic held port43127.
    // These controls are frozen before enabling any echo exception.
    const ECHO_JCL: &[u8] = b"//PBCLNT01 JOB CLASS=A\n//STEP1 EXEC PGM=IEFBR14\n";
    const ECHO_AUTH_BODY: &[u8] = br#"{"category":"authentication_required","message":"valid authentication is required","status":401}"#;
    const ECHO_MESSAGE: &str = "Rest API failure with HTTP(S) status 401\nvalid authentication is required\nThis operation requires authentication.\nUsername or password are not valid or expired.";
    const ECHO_DETAILS: &str = concat!(
        "Received HTTP(S) error 401 = Unauthorized.\n\n",
        "Protocol:          http\n",
        "Host:              127.0.0.1\n",
        "Port:              43127\n",
        "Base Path:         \n",
        "Resource:          /zosmf/restjobs/jobs\n",
        "Request:           PUT\n",
        "Headers:           [{\"Content-Type\":\"text/plain; charset=utf8\"},{\"X-IBM-Intrdr-Mode\":\"TEXT\"},{\"X-IBM-Intrdr-Lrecl\":\"80\"},{\"X-IBM-Intrdr-Recfm\":\"F\"},{\"X-CSRF-ZOSMF-HEADER\":true}]\n",
        "Payload:           '//PBCLNT01 JOB CLASS=A\\n//STEP1 EXEC PGM=IEFBR14\\n'\n",
        "Allow Unauth Cert: false\n",
        "Available creds:   user,password,base64EncodedAuth\n",
        "Your auth order:   basic,token,bearer,cert-pem\n",
        "Auth type used:    basic"
    );
    const ECHO_STDERR: &str = concat!(
        "Unable to perform this operation due to the following problem:\n",
        "valid authentication is required\n",
        "This operation requires authentication.\n",
        "Username or password are not valid or expired.\n\n",
        "Response From Service\n",
        "category: authentication_required\n",
        "message:  valid authentication is required\n",
        "status:   401\n\n",
        "Diagnostic Information\n",
        "Received HTTP(S) error 401 = Unauthorized.\n\n",
        "Protocol:          http\n",
        "Host:              127.0.0.1\n",
        "Port:              43127\n",
        "Base Path:         \n",
        "Resource:          /zosmf/restjobs/jobs\n",
        "Request:           PUT\n",
        "Headers:           [{\"Content-Type\":\"text/plain; charset=utf8\"},{\"X-IBM-Intrdr-Mode\":\"TEXT\"},{\"X-IBM-Intrdr-Lrecl\":\"80\"},{\"X-IBM-Intrdr-Recfm\":\"F\"},{\"X-CSRF-ZOSMF-HEADER\":true}]\n",
        "Payload:           '//PBCLNT01 JOB CLASS=A\\n//STEP1 EXEC PGM=IEFBR14\\n'\n",
        "Allow Unauth Cert: false\n",
        "Available creds:   user,password,base64EncodedAuth\n",
        "Your auth order:   basic,token,bearer,cert-pem\n",
        "Auth type used:    basic\n"
    );

    fn independent_echo_fixture() -> (Value, Vec<Observation>, SocketAddr) {
        let value = serde_json::json!({
            "success":false, "exitCode":1, "data":{}, "stdout":"",
            "message":ECHO_MESSAGE, "stderr":ECHO_STDERR,
            "error":{
                "msg":ECHO_MESSAGE,
                "causeErrors":"{\"category\":\"authentication_required\",\"message\":\"valid authentication is required\",\"status\":401}",
                "source":"http", "request":"PUT", "resource":"/zosmf/restjobs/jobs",
                "errorCode":401, "httpStatus":401, "protocol":"http",
                "host":"127.0.0.1", "port":43127, "basePath":"",
                "headers":[{"Content-Type":"text/plain; charset=utf8"},
                    {"X-IBM-Intrdr-Mode":"TEXT"}, {"X-IBM-Intrdr-Lrecl":"80"},
                    {"X-IBM-Intrdr-Recfm":"F"}, {"X-CSRF-ZOSMF-HEADER":true}],
                "payload":"//PBCLNT01 JOB CLASS=A\n//STEP1 EXEC PGM=IEFBR14\n",
                "additionalDetails":ECHO_DETAILS
            }
        });
        let mut wire = observed("PUT", "/zosmf/restjobs/jobs", 401, ECHO_AUTH_BODY);
        wire.request = ECHO_JCL.to_vec();
        (value, vec![wire], "127.0.0.1:43127".parse().unwrap())
    }

    fn echo_control(value: &Value, actual: &[Observation], address: SocketAddr) -> Check {
        refusal_with_caller_echo("bad-password", value, actual, "JOB00001", Some(2), address)
    }

    #[test]
    fn pure_exact_caller_echo_is_distinct_from_protected_content() {
        let (value, actual, address) = independent_echo_fixture();
        assert!(echo_control(&value, &actual, address).is_ok());
        for path in ["/error/payload", "/error/additionalDetails", "/stderr"] {
            for replacement in [
                "IEFBR14",
                "//PBCLNT01 JOB CLASS=A\n//STEP1 EXEC PGM=IEFBR14",
                "//STEP1 EXEC PGM=IEFBR14\n",
                "//PBCLNT01 JOB CLASS=B\n//STEP1 EXEC PGM=IEFBR14\n",
                "prefix//PBCLNT01 JOB CLASS=A\n//STEP1 EXEC PGM=IEFBR14\n",
                "//PBCLNT01 JOB CLASS=A\n//STEP1 EXEC PGM=IEFBR14\nIEFBR14",
            ] {
                let mut changed = value.clone();
                *changed.pointer_mut(path).unwrap() = serde_json::json!(replacement);
                assert!(
                    echo_control(&changed, &actual, address).is_err(),
                    "admitted {path}"
                );
            }
        }
        let mut duplicate = value.clone();
        duplicate["error"]["additionalDetails"] = serde_json::json!(format!(
            "{ECHO_DETAILS}\nPayload:           '//PBCLNT01 JOB CLASS=A\\n//STEP1 EXEC PGM=IEFBR14\\n'"
        ));
        assert!(echo_control(&duplicate, &actual, address).is_err());
    }

    #[test]
    fn pure_caller_echo_requires_exact_refused_request() {
        let (value, actual, address) = independent_echo_fixture();
        assert!(echo_control(&value, &actual, address).is_ok());
        for body in [b"".as_slice(), b"//STEP1 EXEC PGM=IEFBR14\n", b"OTHER"] {
            let mut changed = actual.clone();
            changed[0].request = body.to_vec();
            assert!(echo_control(&value, &changed, address).is_err());
        }
        for (method, uri, status) in [
            ("GET", "/zosmf/restjobs/jobs", 401),
            ("PUT", "/zosmf/restjobs/jobs/OTHER", 401),
            ("PUT", "/zosmf/restjobs/jobs", 200),
            ("PUT", "/zosmf/restjobs/jobs", 403),
        ] {
            let mut changed = observed(method, uri, status, ECHO_AUTH_BODY);
            changed.request = ECHO_JCL.to_vec();
            assert!(echo_control(&value, &[changed], address).is_err());
        }
        let extra = observed("GET", "/zosmf/restjobs/jobs", 200, b"[]");
        for changed in [
            vec![],
            vec![actual[0].clone(), actual[0].clone()],
            vec![actual[0].clone(), extra.clone()],
            vec![extra, actual[0].clone()],
        ] {
            assert!(echo_control(&value, &changed, address).is_err());
        }
        for (field, replacement) in [
            ("success", serde_json::json!(true)),
            ("exitCode", serde_json::json!(0)),
            ("exitCode", serde_json::json!(2)),
        ] {
            let mut changed = value.clone();
            changed[field] = replacement;
            assert!(echo_control(&changed, &actual, address).is_err());
        }
        assert!(envelope(&serde_json::to_vec(&value).unwrap(), 0, false).is_err());
    }

    #[test]
    fn pure_caller_echo_requires_strict_protected_response() {
        let (value, actual, address) = independent_echo_fixture();
        assert!(echo_control(&value, &actual, address).is_ok());
        for body in [
            b"IEFBR14".as_slice(), ECHO_JCL, b"null", b"[]", b"{}",
            br#"{"category":"not_authorized","message":"valid authentication is required","status":401}"#,
            br#"{"category":"authentication_required","message":"OTHER","status":401}"#,
            br#"{"category":"authentication_required","message":"valid authentication is required","status":403}"#,
            br#"{"category":"authentication_required","message":"valid authentication is required","status":401,"jobname":"PBCLNT01"}"#,
            br#"{"category":"authentication_required","message":"valid authentication is required","status":401,"record-count":1}"#,
            br#"{"category":"authentication_required","message":"valid authentication is required","status":401,"owner":"IBMUSER"}"#,
            br#"{"category":"authentication_required", "message":"valid authentication is required","status":401}"#,
            br#"{"status":401,"message":"valid authentication is required","category":"authentication_required"}"#,
            b"{\"category\":\"authentication_required\",\"message\":\"valid authentication is required\",\"status\":401}\n",
        ] {
            let mut changed = actual.clone();
            changed[0].response = body.to_vec();
            assert!(echo_control(&value, &changed, address).is_err());
        }
    }

    #[test]
    fn pure_caller_echo_does_not_exempt_other_fields() {
        let (value, actual, address) = independent_echo_fixture();
        assert!(echo_control(&value, &actual, address).is_ok());
        for (path, replacement) in [
            ("/data", serde_json::json!("IEFBR14")),
            ("/data", serde_json::json!({"new":"benign"})),
            ("/message", serde_json::json!("IEFBR14")),
            ("/stdout", serde_json::json!("IEFBR14")),
            ("/error/causeErrors", serde_json::json!("IEFBR14")),
        ] {
            let mut changed = value.clone();
            *changed.pointer_mut(path).unwrap() = replacement;
            assert!(echo_control(&changed, &actual, address).is_err());
        }
        for inside_error in [false, true] {
            for (key, replacement) in [
                ("new", serde_json::json!("IEFBR14")),
                ("jobname", serde_json::json!("PBCLNT01")),
                ("ddname", serde_json::json!("STEP1:SYSPRINT")),
                ("record-count", serde_json::json!(1)),
                ("byte-count", serde_json::json!(7)),
                ("owner", serde_json::json!("IBMUSER")),
            ] {
                let mut changed = value.clone();
                let object = if inside_error {
                    &mut changed["error"]
                } else {
                    &mut changed
                };
                object
                    .as_object_mut()
                    .unwrap()
                    .insert(key.into(), replacement);
                assert!(echo_control(&changed, &actual, address).is_err());
            }
        }
        for (path, text) in [
            ("/error/additionalDetails", ECHO_DETAILS),
            ("/stderr", ECHO_STDERR),
        ] {
            let mut changed = value.clone();
            *changed.pointer_mut(path).unwrap() =
                serde_json::json!(format!("{text}\nowner: IBMUSER"));
            assert!(echo_control(&changed, &actual, address).is_err());
        }
    }

    #[test]
    fn pure_caller_echo_binds_sdk_formatter_context() {
        let (value, actual, address) = independent_echo_fixture();
        assert!(echo_control(&value, &actual, address).is_ok());
        for (field, replacement) in [
            ("source", serde_json::json!("client")),
            ("request", serde_json::json!("GET")),
            ("resource", serde_json::json!("/OTHER")),
            ("errorCode", serde_json::json!(403)),
            ("httpStatus", serde_json::json!(403)),
            ("protocol", serde_json::json!("https")),
            ("host", serde_json::json!("localhost")),
            ("port", serde_json::json!(43128)),
            ("basePath", serde_json::json!("/OTHER")),
            ("headers", serde_json::json!([])),
            ("msg", serde_json::json!("OTHER")),
        ] {
            let mut changed = value.clone();
            changed["error"][field] = replacement;
            assert!(
                echo_control(&changed, &actual, address).is_err(),
                "admitted {field}"
            );
        }
        let mut changed = value.clone();
        changed["error"]["headers"]
            .as_array_mut()
            .unwrap()
            .swap(0, 1);
        assert!(echo_control(&changed, &actual, address).is_err());
        let mut changed = value.clone();
        changed["error"]["headers"][4]["X-CSRF-ZOSMF-HEADER"] = serde_json::json!("true");
        assert!(echo_control(&changed, &actual, address).is_err());
        for (from, to) in [
            ("user,password,base64EncodedAuth", "user,password"),
            ("basic,token,bearer,cert-pem", "token,basic,bearer,cert-pem"),
            ("Allow Unauth Cert: false", "Allow Unauth Cert: true"),
            ("Auth type used:    basic", "Auth type used:    token"),
        ] {
            let mut changed = value.clone();
            changed["error"]["additionalDetails"] =
                serde_json::json!(ECHO_DETAILS.replace(from, to));
            changed["stderr"] = serde_json::json!(ECHO_STDERR.replace(from, to));
            assert!(echo_control(&changed, &actual, address).is_err());
        }
        for (path, text) in [
            ("/error/additionalDetails", ECHO_DETAILS),
            ("/stderr", ECHO_STDERR),
        ] {
            for replacement in [
                format!("prefix{text}"),
                format!("{text}suffix"),
                format!("{text}\n"),
                format!("\u{1b}[31m{text}"),
                text.replace('\n', "\r\n"),
            ] {
                let mut changed = value.clone();
                *changed.pointer_mut(path).unwrap() = serde_json::json!(replacement);
                assert!(echo_control(&changed, &actual, address).is_err());
            }
        }
        let mut changed = value.clone();
        changed["stderr"] = serde_json::json!(ECHO_STDERR.trim_end_matches('\n'));
        assert!(echo_control(&changed, &actual, address).is_err());
        let mut changed = value.clone();
        changed["message"] = serde_json::json!("OTHER");
        assert!(echo_control(&changed, &actual, address).is_err());
        // Candidate fields agree with one another but disagree with the held listener.
        assert!(echo_control(&value, &actual, "127.0.0.1:43128".parse().unwrap()).is_err());
        assert!(echo_control(&value, &actual, "127.0.0.2:43127".parse().unwrap()).is_err());
    }

    #[test]
    fn pure_caller_echo_is_bad_password_only() {
        let (value, actual, address) = independent_echo_fixture();
        assert!(echo_control(&value, &actual, address).is_ok());
        for action in [
            "other-owner-list",
            "other-status",
            "other-files",
            "other-content",
        ] {
            assert!(
                refusal_with_caller_echo(action, &value, &actual, "JOB00001", Some(2), address)
                    .is_err()
            );
        }
        // Otherwise-valid OTHER routes isolate the action boundary: a broad
        // three-field sanitizer would admit these after removing caller echoes.
        for action in [
            "other-owner-list",
            "other-status",
            "other-files",
            "other-content",
        ] {
            let owner_route = action == "other-owner-list";
            let wire = if owner_route {
                observed(
                    "GET",
                    "/zosmf/restjobs/jobs?owner=IBMUSER&prefix=PBCLNT01",
                    403,
                    br#"{"category":"not_authorized"}"#,
                )
            } else {
                observed(
                    "GET",
                    "/zosmf/restjobs/jobs?owner=*&jobid=JOB00001",
                    200,
                    b"[]",
                )
            };
            let base = serde_json::json!({"success":false,"exitCode":1,"data":{},"stdout":"",
                "error":{"causeErrors":if owner_route {
                    "{\"category\":\"not_authorized\"}"
                } else { "Zero jobs were returned." }}});
            assert!(
                refusal_with_caller_echo(
                    action,
                    &base,
                    std::slice::from_ref(&wire),
                    "JOB00001",
                    Some(2),
                    address
                )
                .is_ok()
            );
            for path in ["/error/payload", "/error/additionalDetails", "/stderr"] {
                let mut changed = base.clone();
                let echoed = if path == "/stderr" {
                    &mut changed
                } else {
                    &mut changed["error"]
                };
                let key = path.rsplit('/').next().unwrap();
                echoed.as_object_mut().unwrap().insert(
                    key.into(),
                    serde_json::json!("//PBCLNT01 JOB CLASS=A\n//STEP1 EXEC PGM=IEFBR14\n"),
                );
                assert!(
                    refusal_with_caller_echo(
                        action,
                        &changed,
                        std::slice::from_ref(&wire),
                        "JOB00001",
                        Some(2),
                        address
                    )
                    .is_err(),
                    "OTHER echo admitted: {action} {path}"
                );
            }
            let mut all_three = base.clone();
            all_three["error"]["payload"] = value["error"]["payload"].clone();
            all_three["error"]["additionalDetails"] = value["error"]["additionalDetails"].clone();
            all_three["stderr"] = value["stderr"].clone();
            assert!(
                refusal_with_caller_echo(action, &all_three, &[wire], "JOB00001", Some(2), address)
                    .is_err()
            );
        }
        let denied = serde_json::json!({"success":false,"exitCode":1,
            "error":{"causeErrors":"Zero jobs were returned."}});
        let mut legacy = observed(
            "PUT",
            "/zosmf/restjobs/jobs",
            401,
            br#"{"category":"not_authorized"}"#,
        );
        legacy.request = ECHO_JCL.to_vec();
        assert!(
            refusal_with_caller_echo(
                "bad-password",
                &denied,
                &[legacy],
                "JOB00001",
                None,
                address
            )
            .is_ok()
        );
        let owner = observed(
            "GET",
            "/zosmf/restjobs/jobs?owner=IBMUSER&prefix=PBCLNT01",
            403,
            br#"{"category":"not_authorized"}"#,
        );
        assert!(
            refusal_with_caller_echo(
                "other-owner-list",
                &denied,
                &[owner],
                "JOB00001",
                None,
                address
            )
            .is_ok()
        );
        for action in ["other-status", "other-files", "other-content"] {
            let filtered = observed(
                "GET",
                "/zosmf/restjobs/jobs?owner=*&jobid=JOB00001",
                200,
                b"[]",
            );
            assert!(
                refusal_with_caller_echo(
                    action,
                    &denied,
                    &[filtered],
                    "JOB00001",
                    Some(2),
                    address
                )
                .is_ok()
            );
        }
    }

    fn sample_snapshot() -> Snapshot {
        Snapshot {
            jobs: vec![JobSnapshot {
                id: "JOB00001".into(),
                name: "PBCLNT01".into(),
                owner: "IBMUSER".into(),
                class: 'A',
                priority: 1,
                state: JobState::Completed,
                return_code: Some(0),
                abend_code: None,
                active_step: None,
                initiator: None,
                steps: vec![],
                attempt: 1,
                version: 2,
                kind: Default::default(),
                origin: Default::default(),
                route: Default::default(),
                cancellation: None,
            }],
            more: false,
            rows: vec![ProviderStateRecord {
                namespace: "jes-spool".into(),
                key: "opaque".into(),
                version: 1,
                payload: vec![0, 255, 7],
            }],
            artifacts: BTreeMap::from([("object".into(), Entry::File(vec![7]))]),
        }
    }

    use std::{
        collections::BTreeMap,
        fs,
        io::{Read, Write},
        net::SocketAddr,
        os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
        path::{Path, PathBuf},
        process::{Command, Stdio},
        sync::{Arc, Mutex},
        time::{Duration, Instant},
    };

    use axum::{
        body::{Body, to_bytes},
        extract::{Request, State},
        middleware::{self, Next},
        response::Response,
    };
    use mainframe_env_batch::{JobSnapshot, JobState};
    use mainframe_env_host_api::AccessIntent;
    use mainframe_env_racf::MemorySecretResolver;
    use mainframe_env_server::{
        ArtifactProfile, ProductServer, ServerConfig, StoreProfile, TlsConfig,
        default_program_router,
    };
    use mainframe_env_store::MemoryStore;
    use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore, WorkState, WorkStore};
    use serde_json::Value;
    use tokio::{
        net::{TcpListener, TcpStream},
        sync::oneshot,
        task::JoinHandle,
        time::timeout,
    };

    type Check<T = ()> = Result<T, String>;
    const JCL: &[u8] = b"//PBCLNT01 JOB CLASS=A\n//STEP1 EXEC PGM=IEFBR14\n";
    const JOBS: &str = "/zosmf/restjobs/jobs";
    const OWNER_QUERY: &str = "/zosmf/restjobs/jobs?owner=IBMUSER&prefix=PBCLNT01";
    const PROFILE: &str = "public-client-linux-x86_64";
    const TOOLCHAIN: &str = "/workspace/scratch/sandbox-toolchain";

    fn require(condition: bool, message: &str) -> Check {
        if condition {
            Ok(())
        } else {
            Err(message.into())
        }
    }

    fn problem(error: impl std::fmt::Display) -> String {
        error.to_string()
    }

    fn json(bytes: &[u8]) -> Check<Value> {
        require(
            !bytes.is_empty() && bytes.len() <= 65_536,
            "JSON bytes outside bounds",
        )?;
        serde_json::from_slice(bytes).map_err(problem)
    }

    fn envelope(bytes: &[u8], exit: i32, success: bool) -> Check<Value> {
        let value = json(bytes)?;
        require(
            value.is_object() && value["success"].as_bool() == Some(success),
            "CLI success field differs",
        )?;
        let code = value["exitCode"]
            .as_i64()
            .ok_or("CLI integer exitCode missing")?;
        require(
            if success {
                exit == 0 && code == 0
            } else {
                exit > 0 && code > 0
            },
            "CLI actual exit and envelope differ",
        )?;
        if success {
            require(
                value.get("error").is_none_or(Value::is_null),
                "positive CLI has error",
            )?;
            require(
                match &value["data"] {
                    Value::String(s) => !s.is_empty(),
                    Value::Array(a) => !a.is_empty(),
                    Value::Object(o) => !o.is_empty(),
                    _ => false,
                },
                "positive CLI data empty or wrong type",
            )?;
        } else {
            require(value["error"].is_object(), "negative CLI error missing")?;
            require(
                value.get("data").is_none_or(|data| {
                    data.is_null()
                        || data.as_object().is_some_and(|o| o.is_empty())
                        || data.as_array().is_some_and(|a| a.is_empty())
                        || data.as_str() == Some("")
                }),
                "negative CLI leaked data",
            )?;
        }
        Ok(value)
    }

    fn transport(exit: i32, child: Option<&[u8]>, error: &[u8]) -> Check<i32> {
        require(exit == 0 && error.is_empty(), "command transport failed")?;
        let child = child.ok_or("actual child exit missing")?;
        require(
            child.len() <= 32 && child.ends_with(b"\n"),
            "child exit framing differs",
        )?;
        let text = std::str::from_utf8(&child[..child.len() - 1]).map_err(problem)?;
        let code: i32 = text.parse().map_err(problem)?;
        require(text == code.to_string(), "child exit is not canonical")?;
        Ok(code)
    }

    fn job(value: &Value, expected_id: Option<&str>, terminal: bool) -> Check<String> {
        require(value.is_object(), "job is not an object")?;
        let id = value["jobid"].as_str().ok_or("job ID missing")?;
        let digits = id.strip_prefix("JOB").ok_or("job ID prefix differs")?;
        require(
            (5..=8).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit()),
            "job ID digits differ",
        )?;
        let number: u32 = digits.parse().map_err(problem)?;
        require(
            number > 0
                && id == format!("JOB{number:05}")
                && expected_id.is_none_or(|expected| expected == id),
            "job identity differs",
        )?;
        require(
            value["jobname"] == "PBCLNT01" && value["owner"] == "IBMUSER",
            "job name or owner differs",
        )?;
        require(
            if terminal {
                value["status"] == "OUTPUT" && value["retcode"] == "CC 0000"
            } else {
                value["status"] == "ACTIVE" && value.get("retcode") == Some(&Value::Null)
            },
            "job status or retcode differs",
        )?;
        Ok(id.into())
    }

    fn job_any_state(value: &Value, id: &str) -> Check {
        job(value, Some(id), value["status"] == "OUTPUT").map(|_| ())
    }

    fn singleton(value: &Value) -> Check<&Value> {
        let values = value.as_array().ok_or("job query is not an array")?;
        require(values.len() == 1, "job query must contain exactly one job")?;
        Ok(&values[0])
    }

    fn selected_file(value: &Value, id: &str) -> Check<u64> {
        let files = value.as_array().ok_or("spool data is not an array")?;
        require(
            !files.is_empty() && files.len() <= 64,
            "spool array outside bounds",
        )?;
        let mut selected = None;
        let mut ids = Vec::new();
        for file in files {
            require(
                file.is_object() && file["jobid"] == id && file["jobname"] == "PBCLNT01",
                "spool job binding differs",
            )?;
            let index = file["id"].as_u64().ok_or("spool ID is not an integer")?;
            require(
                index <= 63 && !ids.contains(&index),
                "spool ID duplicate or outside bounds",
            )?;
            ids.push(index);
            if file["ddname"] == "STEP1:SYSPRINT" {
                require(
                    selected.is_none()
                        && file["record-count"].as_u64() == Some(1)
                        && file["byte-count"].as_u64() == Some(7),
                    "selected spool record differs",
                )?;
                selected = Some(index);
            }
        }
        selected.ok_or("selected STEP1:SYSPRINT missing".into())
    }

    fn content(value: &Value) -> Check {
        require(value.as_str() == Some("IEFBR14"), "content bytes differ")
    }

    #[derive(Clone, Debug)]
    struct Observation {
        method: String,
        uri: String,
        status: u16,
        request: Vec<u8>,
        response: Vec<u8>,
    }

    fn observed(method: &str, uri: &str, status: u16, response: &[u8]) -> Observation {
        Observation {
            method: method.into(),
            uri: uri.into(),
            status,
            request: Vec::new(),
            response: response.to_vec(),
        }
    }

    fn request_vector(actual: &[Observation], expected: &[(&str, &str, u16)]) -> Check {
        require(actual.len() == expected.len(), "HTTP request count differs")?;
        for (actual, (method, uri, status)) in actual.iter().zip(expected) {
            require(
                actual.method == *method && actual.uri == *uri && actual.status == *status,
                &format!(
                    "HTTP vector differs: {} {} {}",
                    actual.method, actual.uri, actual.status
                ),
            )?;
        }
        Ok(())
    }

    fn query(id: &str) -> String {
        format!("{JOBS}?owner=*&jobid={id}")
    }
    fn files_uri(id: &str) -> String {
        format!("{JOBS}/PBCLNT01/{id}/files")
    }
    fn records_uri(id: &str, file: u64) -> String {
        format!("{}/{file}/records", files_uri(id))
    }

    fn zero_jobs_cause(value: &Value, depth: usize) -> bool {
        if depth > 16 {
            return false;
        }
        match value {
            Value::Object(o) => o.iter().any(|(key, value)| {
                (key == "causeErrors" && value.as_str() == Some("Zero jobs were returned."))
                    || zero_jobs_cause(value, depth + 1)
            }),
            Value::Array(a) => a.iter().any(|v| zero_jobs_cause(v, depth + 1)),
            _ => false,
        }
    }

    fn no_disclosure(value: &Value, depth: usize) -> Check {
        require(depth <= 16, "refusal data nesting exceeds bound")?;
        match value {
            Value::Object(fields) => {
                for (key, value) in fields {
                    require(
                        !matches!(
                            key.as_str(),
                            "jobname" | "ddname" | "record-count" | "byte-count"
                        ) && !(key == "owner" && value == "IBMUSER"),
                        "refusal disclosed job/spool metadata",
                    )?;
                    no_disclosure(value, depth + 1)?;
                }
            }
            Value::Array(values) => {
                for value in values {
                    no_disclosure(value, depth + 1)?;
                }
            }
            Value::String(text) => require(!text.contains("IEFBR14"), "refusal disclosed content")?,
            _ => (),
        }
        Ok(())
    }

    fn refusal(
        action: &str,
        value: &Value,
        actual: &[Observation],
        id: &str,
        _file: Option<u64>,
    ) -> Check {
        // Calling this directly also refuses success-looking or disclosed payloads.
        envelope(&serde_json::to_vec(value).map_err(problem)?, 1, false)?;
        no_disclosure(value, 0)?;
        match action {
            "bad-password" => {
                request_vector(actual, &[("PUT", JOBS, 401)])?;
                require(actual[0].request == JCL, "refused submit JCL differs")?;
                no_disclosure(&json(&actual[0].response)?, 0)?;
            }
            "other-owner-list" => {
                request_vector(actual, &[("GET", OWNER_QUERY, 403)])?;
                no_disclosure(&json(&actual[0].response)?, 0)?;
                require(
                    json(&actual[0].response)?["category"] == "not_authorized",
                    "owner refusal category differs",
                )?;
            }
            "other-status" | "other-files" | "other-content" => {
                request_vector(actual, &[("GET", &query(id), 200)])?;
                require(
                    actual[0].response == b"[]",
                    "filtered response is not exact empty array",
                )?;
                require(
                    zero_jobs_cause(&value["error"], 0),
                    "zero-jobs error cause missing",
                )?;
            }
            _ => return Err("unknown negative action".into()),
        }
        Ok(())
    }

    fn refusal_with_caller_echo(
        action: &str,
        value: &Value,
        actual: &[Observation],
        id: &str,
        file: Option<u64>,
        address: SocketAddr,
    ) -> Check {
        let echo_paths = ["/error/payload", "/error/additionalDetails", "/stderr"];
        let has_caller_marker = echo_paths.iter().any(|path| {
            value
                .pointer(path)
                .and_then(Value::as_str)
                .is_some_and(|text| text.contains("IEFBR14"))
        });
        if action != "bad-password" || !has_caller_marker {
            return refusal(action, value, actual, id, file);
        }

        // Bound and validate the original envelope before making a clone.
        envelope(&serde_json::to_vec(value).map_err(problem)?, 1, false)?;
        require(
            value["exitCode"].as_i64() == Some(1),
            "echo SDK exitCode differs",
        )?;
        require(
            value["data"]
                .as_object()
                .is_some_and(|data| data.is_empty())
                && value["stdout"].as_str() == Some(""),
            "echo SDK data/stdout differs",
        )?;
        request_vector(actual, &[("PUT", JOBS, 401)])?;
        require(actual[0].request == JCL, "echo refused submit JCL differs")?;
        const AUTH_BODY: &[u8] = br#"{"category":"authentication_required","message":"valid authentication is required","status":401}"#;
        require(
            actual[0].response == AUTH_BODY,
            "echo server auth bytes differ",
        )?;
        require(
            matches!(address.ip(), std::net::IpAddr::V4(ip) if ip.octets() == [127, 0, 0, 1])
                && address.port() != 0,
            "echo held listener differs",
        )?;

        // These expectations come from pinned sources and fixed session inputs,
        // never the candidate diagnostic or its reported listener port.
        let error = &value["error"];
        require(
            error["source"].as_str() == Some("http")
                && error["request"].as_str() == Some("PUT")
                && error["resource"].as_str() == Some(JOBS)
                && error["errorCode"].as_u64() == Some(401)
                && error["httpStatus"].as_u64() == Some(401)
                && error["protocol"].as_str() == Some("http")
                && error["host"].as_str() == Some("127.0.0.1")
                && error["port"].as_u64() == Some(u64::from(address.port()))
                && error["basePath"].as_str() == Some(""),
            "echo SDK request/session context differs",
        )?;
        require(
            error["headers"]
                == serde_json::json!([
                    {"Content-Type":"text/plain; charset=utf8"},
                    {"X-IBM-Intrdr-Mode":"TEXT"},
                    {"X-IBM-Intrdr-Lrecl":"80"},
                    {"X-IBM-Intrdr-Recfm":"F"},
                    {"X-CSRF-ZOSMF-HEADER":true}
                ]),
            "echo SDK headers differ",
        )?;
        const AUTH_MESSAGE: &str = concat!(
            "Rest API failure with HTTP(S) status 401\n",
            "valid authentication is required\n",
            "This operation requires authentication.\n",
            "Username or password are not valid or expired."
        );
        require(
            error["msg"].as_str() == Some(AUTH_MESSAGE)
                && value["message"].as_str() == Some(AUTH_MESSAGE)
                && error["causeErrors"]
                    .as_str()
                    .is_some_and(|text| text.as_bytes() == AUTH_BODY),
            "echo SDK auth message/cause differs",
        )?;
        require(
            error["payload"]
                .as_str()
                .is_some_and(|text| text.as_bytes() == JCL),
            "echo SDK payload is not the entire caller JCL",
        )?;
        let details = format!(
            "{}{}{}",
            concat!(
                "Received HTTP(S) error 401 = Unauthorized.\n\n",
                "Protocol:          http\n",
                "Host:              127.0.0.1\n",
                "Port:              "
            ),
            address.port(),
            concat!(
                "\nBase Path:         \n",
                "Resource:          /zosmf/restjobs/jobs\n",
                "Request:           PUT\n",
                "Headers:           [{\"Content-Type\":\"text/plain; charset=utf8\"},{\"X-IBM-Intrdr-Mode\":\"TEXT\"},{\"X-IBM-Intrdr-Lrecl\":\"80\"},{\"X-IBM-Intrdr-Recfm\":\"F\"},{\"X-CSRF-ZOSMF-HEADER\":true}]\n",
                "Payload:           '//PBCLNT01 JOB CLASS=A\\n//STEP1 EXEC PGM=IEFBR14\\n'\n",
                "Allow Unauth Cert: false\n",
                "Available creds:   user,password,base64EncodedAuth\n",
                "Your auth order:   basic,token,bearer,cert-pem\n",
                "Auth type used:    basic"
            )
        );
        let stderr = format!(
            "{}{}\n",
            concat!(
                "Unable to perform this operation due to the following problem:\n",
                "valid authentication is required\n",
                "This operation requires authentication.\n",
                "Username or password are not valid or expired.\n\n",
                "Response From Service\n",
                "category: authentication_required\n",
                "message:  valid authentication is required\n",
                "status:   401\n\n",
                "Diagnostic Information\n"
            ),
            details
        );
        require(
            error["additionalDetails"].as_str() == Some(details.as_str())
                && value["stderr"].as_str() == Some(stderr.as_str()),
            "echo SDK complete diagnostics differ",
        )?;

        // Only the three proved strings change in this validation-only clone.
        // Original captured SDK/wire bytes and all other fields stay untouched.
        let mut validation = value.clone();
        for path in echo_paths {
            *validation
                .pointer_mut(path)
                .ok_or("proved echo path missing")? = Value::String(String::new());
        }
        refusal(action, &validation, actual, id, file)
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum Entry {
        Directory,
        File(Vec<u8>),
    }
    #[derive(Clone, Debug, Eq, PartialEq)]
    struct Snapshot {
        jobs: Vec<JobSnapshot>,
        more: bool,
        rows: Vec<ProviderStateRecord>,
        artifacts: BTreeMap<String, Entry>,
    }
    fn unchanged(before: &Snapshot, after: &Snapshot) -> Check {
        require(
            before == after,
            "negative mutated job/count/opaque provider rows/artifacts",
        )
    }
    fn complete_run(starts: usize, waits: usize, requests: usize, polls: usize) -> Check {
        require(
            (1..=12).contains(&polls)
                && starts == 9 + polls
                && waits == starts
                && requests == 11 + polls,
            "actual command/wait/HTTP totals differ",
        )
    }
    fn cleanup_result(body: Check, failures: &[String]) -> Check {
        let mut failures = failures.to_vec();
        if let Err(error) = body {
            failures.insert(0, error);
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; "))
        }
    }

    #[derive(Default)]
    struct Capture {
        active: Option<String>,
        events: Vec<(Option<String>, Observation)>,
        bytes: usize,
        failures: Vec<String>,
        attempts: usize,
        starts: usize,
        waits: usize,
        launcher_waits: usize,
    }
    type SharedCapture = Arc<Mutex<Capture>>;

    async fn observe(
        State(shared): State<SharedCapture>,
        request: Request,
        next: Next,
    ) -> Response {
        // Only bounded body observation is added; every admitted request reaches the real router.
        let result = async {
            let method = request.method().to_string();
            let uri = request.uri().to_string();
            require(uri.len() <= 4096, "HTTP URI exceeds capture bound")?;
            let label = shared.lock().map_err(problem)?.active.clone();
            let (parts, body) = request.into_parts();
            let request_bytes = to_bytes(body, 65_536).await.map_err(problem)?;
            let response = next
                .run(Request::from_parts(
                    parts,
                    Body::from(request_bytes.clone()),
                ))
                .await;
            let (parts, body) = response.into_parts();
            let status = parts.status.as_u16();
            let response_bytes = to_bytes(body, 65_536).await.map_err(problem)?;
            let mut capture = shared.lock().map_err(problem)?;
            require(
                label.is_some() && label == capture.active,
                "HTTP outside command start/wait boundary",
            )?;
            require(
                capture.events.len() < 64
                    && capture.bytes + uri.len() + request_bytes.len() + response_bytes.len()
                        <= 4 * 1024 * 1024,
                "HTTP capture aggregate exceeds bound",
            )?;
            capture.bytes += uri.len() + request_bytes.len() + response_bytes.len();
            capture.events.push((
                label,
                Observation {
                    method,
                    uri,
                    status,
                    request: request_bytes.to_vec(),
                    response: response_bytes.to_vec(),
                },
            ));
            Ok::<_, String>(Response::from_parts(parts, Body::from(response_bytes)))
        }
        .await;
        match result {
            Ok(response) => response,
            Err(error) => {
                if let Ok(mut capture) = shared.lock()
                    && capture.failures.len() < 64
                {
                    capture.failures.push(error.clone());
                }
                // Capture failure is always a fixture error and receives no route/refusal credit.
                let mut response = Response::new(Body::from(error));
                *response.status_mut() = axum::http::StatusCode::INTERNAL_SERVER_ERROR;
                response
            }
        }
    }

    fn canonical(path: &Path) -> Check {
        require(
            path.is_absolute()
                && path.components().all(|c| {
                    matches!(
                        c,
                        std::path::Component::RootDir | std::path::Component::Normal(_)
                    )
                }),
            "path must be absolute and normalized",
        )?;
        require(
            fs::canonicalize(path).map_err(problem)? == path,
            "path must be resolved and nonsymlink",
        )
    }

    fn selected_path(variable: &str) -> Check<PathBuf> {
        let path = PathBuf::from(
            std::env::var_os(variable).ok_or_else(|| format!("{variable} is required"))?,
        );
        canonical(&path)?;
        Ok(path)
    }

    fn write_new(path: &Path, bytes: &[u8]) -> Check {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .map_err(problem)?;
        file.write_all(bytes).map_err(problem)
    }

    fn private_directory(path: &Path) -> Check {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(path)
            .map_err(problem)?;
        canonical(path)
    }

    struct Inputs {
        python: PathBuf,
        source: PathBuf,
        tree: PathBuf,
        files: BTreeMap<String, PathBuf>,
        bindings: Vec<u8>,
    }
    impl Inputs {
        fn load() -> Check<Self> {
            let python = selected_path("CV209_PUBLIC_CLIENT_PYTHON")?;
            require(
                python
                    == fs::canonicalize(Path::new(TOOLCHAIN).join("python/bin/python3.12"))
                        .map_err(problem)?,
                "selected Python is not the pinned tool",
            )?;
            let source = selected_path("CV209_PUBLIC_CLIENT_SOURCE_ROOT")?;
            let compiled_source = Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(3)
                .ok_or("compiled source root missing")?;
            require(
                source == compiled_source,
                "executing source root differs from compiled fixture",
            )?;
            let binding_path = selected_path("CV209_PUBLIC_CLIENT_BINDINGS")?;
            let bindings = bounded_file(&binding_path, 16_384)?;
            let value: Value = serde_json::from_slice(&bindings).map_err(problem)?;
            require(
                value["profile"] == PROFILE,
                "selected input profile differs",
            )?;
            let tree = PathBuf::from(
                value["tree_root_above_package"]
                    .as_str()
                    .ok_or("client tree missing")?,
            );
            canonical(&tree)?;
            let roles = [
                "node-archive",
                "node",
                "zowe-archive",
                "bubblewrap",
                "loader",
                "libdl",
                "libstdcxx",
                "libm",
                "libgcc",
                "libpthread",
                "libc",
                "libnss_files",
            ];
            let selected = value["files"]
                .as_object()
                .ok_or("explicit input file roles missing")?;
            require(
                selected.len() == roles.len(),
                "input role membership differs",
            )?;
            let mut files = BTreeMap::new();
            for role in roles {
                let path = PathBuf::from(
                    selected
                        .get(role)
                        .and_then(Value::as_str)
                        .ok_or("input role path missing")?,
                );
                canonical(&path)?;
                files.insert(role.into(), path);
            }
            Ok(Self {
                python,
                source,
                tree,
                files,
                bindings,
            })
        }

        fn python_command(&self) -> Command {
            let mut command = Command::new(&self.python);
            command
                .current_dir(&self.source)
                .arg("-B")
                .env(
                    "PATH",
                    format!(
                        "{TOOLCHAIN}/python/bin:{TOOLCHAIN}/cargo/bin:{TOOLCHAIN}/git/bin:{}",
                        std::env::var("PATH").unwrap_or_default()
                    ),
                )
                .env("CARGO_HOME", format!("{TOOLCHAIN}/cargo"))
                .env("RUSTUP_HOME", format!("{TOOLCHAIN}/rustup"))
                .env("LD_LIBRARY_PATH", format!("{TOOLCHAIN}/python/lib"))
                .env(
                    "CARGO_TARGET_DIR",
                    self.source.join("target/cv209-public-client-fixture"),
                )
                .env("CARGO_BUILD_JOBS", "2")
                .env("CARGO_INCREMENTAL", "0")
                .env("CARGO_PROFILE_DEV_DEBUG", "0")
                .env("CARGO_PROFILE_TEST_DEBUG", "0")
                .env_remove("HOME")
                .env_remove("CODEX_HOME");
            command
        }
    }

    fn bounded_file(path: &Path, maximum: usize) -> Check<Vec<u8>> {
        let before = fs::symlink_metadata(path).map_err(problem)?;
        require(
            before.is_file() && before.nlink() == 1 && before.len() <= maximum as u64,
            "file type/link/byte bound differs",
        )?;
        let mut file = fs::File::open(path).map_err(problem)?;
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take(maximum as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(problem)?;
        let after = fs::symlink_metadata(path).map_err(problem)?;
        require(
            bytes.len() <= maximum
                && before.dev() == after.dev()
                && before.ino() == after.ino()
                && before.len() == after.len()
                && before.mtime() == after.mtime()
                && before.mtime_nsec() == after.mtime_nsec()
                && file.metadata().map_err(problem)?.ino() == before.ino(),
            "file changed during bounded read",
        )?;
        Ok(bytes)
    }

    fn inventory(root: &Path, depth: usize) -> Check<BTreeMap<String, Entry>> {
        canonical(root)?;
        let mut entries = BTreeMap::new();
        let mut pending = vec![(root.to_path_buf(), 0)];
        let (mut files, mut directories, mut bytes) = (0, 0, 0);
        while let Some((directory, level)) = pending.pop() {
            let metadata = fs::symlink_metadata(&directory).map_err(problem)?;
            require(metadata.is_dir(), "inventory directory type differs")?;
            for entry in fs::read_dir(&directory).map_err(problem)? {
                let path = entry.map_err(problem)?.path();
                let name = path
                    .strip_prefix(root)
                    .map_err(problem)?
                    .to_str()
                    .ok_or("inventory path is not UTF-8")?
                    .to_string();
                require(
                    name.len() <= 4096 && !name.contains(['\n', '\r']),
                    "inventory path exceeds bounds",
                )?;
                let metadata = fs::symlink_metadata(&path).map_err(problem)?;
                let item = if metadata.is_dir() {
                    directories += 1;
                    require(
                        directories <= 256 && level < depth,
                        "inventory directory/depth overflow",
                    )?;
                    pending.push((path.clone(), level + 1));
                    Entry::Directory
                } else {
                    files += 1;
                    require(files <= 128, "inventory file count overflow")?;
                    let data = bounded_file(&path, 1_048_576)?;
                    bytes += data.len();
                    require(
                        bytes <= 4 * 1024 * 1024,
                        "inventory aggregate byte overflow",
                    )?;
                    Entry::File(data)
                };
                require(
                    entries.insert(name, item).is_none(),
                    "inventory duplicate path",
                )?;
            }
            let after = fs::symlink_metadata(&directory).map_err(problem)?;
            require(
                metadata.dev() == after.dev()
                    && metadata.ino() == after.ino()
                    && metadata.mtime() == after.mtime()
                    && metadata.mtime_nsec() == after.mtime_nsec(),
                "inventory membership changed during enumeration",
            )?;
        }
        Ok(entries)
    }

    fn retain_inventory(root: &Path, destination: &Path, depth: usize) -> Check {
        let entries = inventory(root, depth)?;
        private_directory(destination)?;
        let mut membership = String::new();
        for (name, entry) in &entries {
            membership.push_str(&format!("{name} {entry:?}\n"));
            if matches!(entry, Entry::Directory) {
                fs::create_dir_all(destination.join(name)).map_err(problem)?;
            }
        }
        for (name, entry) in entries {
            if let Entry::File(bytes) = entry {
                write_new(&destination.join(name), &bytes)?;
            }
        }
        write_new(&destination.join("inventory.txt"), membership.as_bytes())
    }

    fn snapshot(
        server: &ProductServer,
        store: &MemoryStore,
        artifacts: &Path,
        id: &str,
    ) -> Check<Snapshot> {
        let (jobs, more) = server
            .batch_service()
            .list(None, None, 2)
            .map_err(problem)?;
        require(
            jobs.len() == 1 && !more && jobs[0].id == id,
            "exact one-job membership differs",
        )?;
        let mut rows = Vec::new();
        for namespace in ["jes-job", "jes-meta", "jes-spool"] {
            let page = store
                .list_provider_state_bounded(namespace, 65, 2 * 1024 * 1024)
                .map_err(problem)?;
            require(
                page.len() <= 64,
                "provider namespace exceeded snapshot bound",
            )?;
            rows.extend(page);
        }
        Ok(Snapshot {
            jobs,
            more,
            rows,
            artifacts: inventory(artifacts, 3)?,
        })
    }

    async fn completion(
        server: &ProductServer,
        store: &MemoryStore,
        id: &str,
        deadline: Instant,
    ) -> Check {
        loop {
            require(
                Instant::now() < deadline,
                "job/queue completion barrier deadline",
            )?;
            let job = server.batch_service().get(id).map_err(problem)?;
            let work = store.get_work(&format!("jes:{id}")).map_err(problem)?;
            require(
                job.state != JobState::Completed || job.return_code == Some(0),
                "job completed with nonzero or absent return code",
            )?;
            if job.state == JobState::Completed
                && job.return_code == Some(0)
                && work.is_some_and(|work| work.state == WorkState::Completed)
            {
                return Ok(());
            }
            require(
                !matches!(job.state, JobState::Failed | JobState::Cancelled),
                "job failed before negative barrier",
            )?;
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    struct Run {
        root: PathBuf,
        receipts: PathBuf,
        artifacts: PathBuf,
        commands: PathBuf,
        capture: SharedCapture,
        polls: usize,
    }
    impl Run {
        fn create(inputs: &Inputs) -> Check<Self> {
            let root = PathBuf::from(
                std::env::var_os("CV209_PUBLIC_CLIENT_RUN")
                    .ok_or("CV209_PUBLIC_CLIENT_RUN is required")?,
            );
            require(
                root.file_name().is_some() && !root.exists(),
                "fixture run leaf must be fresh",
            )?;
            canonical(root.parent().ok_or("fixture run parent missing")?)?;
            let evidence_parent = Path::new(
                "/workspace/scratch/unreleased-workers/foundation/CV-209.public-client-compatibility",
            );
            require(
                root.starts_with(evidence_parent)
                    && !root.starts_with(&inputs.source)
                    && !root.starts_with(&inputs.tree),
                "fixture run must be in its external evidence owner",
            )?;
            private_directory(&root)?;
            let receipts = root.join("receipts");
            private_directory(&receipts)?;
            write_new(&receipts.join("explicit-bindings.json"), &inputs.bindings)?;
            Ok(Self {
                artifacts: root.join("artifacts"),
                commands: root.join("commands"),
                root,
                receipts,
                capture: Arc::new(Mutex::new(Capture::default())),
                polls: 0,
            })
        }

        async fn call(
            &self,
            inputs: &Inputs,
            action: &str,
            address: SocketAddr,
            id: Option<&str>,
            file: Option<u64>,
            deadline: Instant,
        ) -> Check<(Value, Vec<Observation>)> {
            let invocation_started = Instant::now();
            let remaining = deadline
                .saturating_duration_since(Instant::now())
                .as_secs_f64()
                .min(10.0);
            require(remaining > 0.0, "phase deadline expired before launch")?;
            let (index, begin, label) = {
                let mut capture = self.capture.lock().map_err(problem)?;
                require(
                    capture.active.is_none() && capture.failures.is_empty(),
                    "prior HTTP capture failed",
                )?;
                let index = capture.attempts;
                let label = format!("{index:02}-{action}");
                capture.attempts += 1;
                capture.active = Some(label.clone());
                (index, capture.events.len(), label)
            };
            let run_dir = self.commands.join(&label);
            let mut command = inputs.python_command();
            command
                .arg(inputs.source.join("tools/ci_assurance.py"))
                .arg("--root")
                .arg(&inputs.source)
                .arg("public-client-command")
                .args(["--development-profile", PROFILE])
                .arg("--profile-tree")
                .arg(&inputs.tree)
                .arg("--run-dir")
                .arg(&run_dir)
                .args([
                    "--action",
                    action,
                    "--port",
                    &address.port().to_string(),
                    "--timeout-seconds",
                    &remaining.to_string(),
                ]);
            for (role, path) in &inputs.files {
                command
                    .arg("--profile-file")
                    .arg(format!("{role}={}", path.display()));
            }
            if let Some(id) = id {
                command.args(["--job-id", id]);
            }
            if let Some(file) = file {
                command.args(["--file-id", &file.to_string()]);
            }
            write_new(
                &self.receipts.join(format!("{label}-launcher.txt")),
                format!("{command:?}\n").as_bytes(),
            )?;
            // Python remains the sole command process-group/wait owner; do not cancel this wait.
            let shared = self.capture.clone();
            let waited = tokio::task::spawn_blocking(move || {
                command
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                let child = command.spawn()?;
                if let Ok(mut capture) = shared.lock() {
                    capture.starts += 1;
                }
                child.wait_with_output()
            })
            .await
            .map_err(problem);
            let (actual, capture_errors) = {
                let mut capture = self.capture.lock().map_err(problem)?;
                capture.active = None;
                if matches!(&waited, Ok(Ok(_))) {
                    capture.launcher_waits += 1;
                }
                let events = capture.events[begin..]
                    .iter()
                    .map(|(_, event)| event.clone())
                    .collect();
                (events, capture.failures.clone())
            };
            let output = waited?.map_err(problem)?;
            write_new(
                &self.receipts.join(format!("{label}-launcher.stdout")),
                &output.stdout,
            )?;
            write_new(
                &self.receipts.join(format!("{label}-launcher.stderr")),
                &output.stderr,
            )?;
            write_new(
                &self.receipts.join(format!("{label}-launcher.exit")),
                format!("{:?}\n", output.status.code()).as_bytes(),
            )?;
            // Retain even partial run trees before evaluating transport or semantic success.
            if run_dir.exists() {
                retain_inventory(&run_dir, &self.receipts.join(&label), 4)?;
            }
            let child = bounded_file(&run_dir.join("child-exit.txt"), 32)?;
            let observed_exit = transport(0, Some(&child), b"")?;
            self.capture.lock().map_err(problem)?.waits += 1;
            self.retain_http(&format!("after-{index:02}"))?;
            require(
                capture_errors.is_empty(),
                &format!("HTTP capture failed: {capture_errors:?}"),
            )?;
            require(
                Instant::now() < deadline,
                "phase deadline expired after actual wait",
            )?;
            require(
                invocation_started.elapsed().as_secs_f64() <= remaining,
                "command allowance exceeded after actual wait",
            )?;
            let supervisor = bounded_file(&run_dir.join("supervision-error.txt"), 4096)?;
            let child = transport(
                output
                    .status
                    .code()
                    .ok_or("launcher terminated by signal")?,
                Some(&child),
                &supervisor,
            )?;
            require(child == observed_exit, "child exit capture changed")?;
            let bytes = bounded_file(&run_dir.join("stdout.bin"), 65_536)?;
            let value = envelope(
                &bytes,
                child,
                !action.starts_with("other-") && action != "bad-password",
            )?;
            Ok((value, actual))
        }

        fn retain_http(&self, label: &str) -> Check {
            let capture = self.capture.lock().map_err(problem)?;
            let mut text = format!(
                "launched_transports={} observed_child_waits={} polls={} HTTP={}\ncapture_errors={:?}\n",
                capture.starts,
                capture.waits,
                self.polls,
                capture.events.len(),
                capture.failures
            );
            text.push_str(&format!(
                "attempts={} waited_launchers={}\n",
                capture.attempts, capture.launcher_waits
            ));
            let directory = self.receipts.join(format!("HTTP-{label}"));
            private_directory(&directory)?;
            for (index, (label, event)) in capture.events.iter().enumerate() {
                text.push_str(&format!(
                    "{index} {label:?} {} {} {}\n",
                    event.method, event.uri, event.status
                ));
                write_new(
                    &directory.join(format!("{index:02}-request.bin")),
                    &event.request,
                )?;
                write_new(
                    &directory.join(format!("{index:02}-response.bin")),
                    &event.response,
                )?;
            }
            write_new(&directory.join("observations.txt"), text.as_bytes())
        }
    }

    async fn workload(
        run: &mut Run,
        inputs: &Inputs,
        server: &ProductServer,
        store: &MemoryStore,
        address: SocketAddr,
    ) -> Check {
        let deadline = Instant::now() + Duration::from_secs(120);
        let (submit, actual) = run
            .call(inputs, "submit", address, None, None, deadline)
            .await?;
        request_vector(&actual, &[("PUT", JOBS, 201)])?;
        require(actual[0].request == JCL, "actual submit JCL differs")?;
        let id = job(&submit["data"], None, false)?;
        require(
            json(&actual[0].response)? == submit["data"],
            "submit HTTP and CLI job differ",
        )?;
        let (list, actual) = run
            .call(inputs, "owner-list", address, None, None, deadline)
            .await?;
        request_vector(&actual, &[("GET", OWNER_QUERY, 200)])?;
        job_any_state(singleton(&list["data"])?, &id)?;
        require(
            json(&actual[0].response)? == list["data"],
            "owner list HTTP and CLI differ",
        )?;
        let mut terminal = false;
        for poll in 1..=12 {
            run.polls = poll;
            let (status, actual) = run
                .call(inputs, "status", address, Some(&id), None, deadline)
                .await?;
            request_vector(&actual, &[("GET", &query(&id), 200)])?;
            let response = json(&actual[0].response)?;
            require(
                singleton(&response)? == &status["data"],
                "status HTTP and CLI job differ",
            )?;
            job_any_state(&status["data"], &id)?;
            terminal = status["data"]["status"] == "OUTPUT";
            if terminal {
                job(&status["data"], Some(&id), true)?;
                break;
            }
            if poll < 12 {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
        require(
            terminal,
            "terminal OUTPUT/CC 0000 not observed in twelve polls",
        )?;
        let (files, actual) = run
            .call(inputs, "files", address, Some(&id), None, deadline)
            .await?;
        request_vector(
            &actual,
            &[("GET", &query(&id), 200), ("GET", &files_uri(&id), 200)],
        )?;
        job(singleton(&json(&actual[0].response)?)?, Some(&id), true)?;
        require(
            json(&actual[1].response)? == files["data"],
            "files HTTP and CLI data differ",
        )?;
        let file = selected_file(&files["data"], &id)?;
        let (records, actual) = run
            .call(inputs, "content", address, Some(&id), Some(file), deadline)
            .await?;
        request_vector(
            &actual,
            &[
                ("GET", &query(&id), 200),
                ("GET", &records_uri(&id, file), 200),
            ],
        )?;
        job(singleton(&json(&actual[0].response)?)?, Some(&id), true)?;
        require(
            actual[1].response == b"IEFBR14",
            "actual record bytes differ",
        )?;
        content(&records["data"])?;
        for action in [
            "bad-password",
            "other-status",
            "other-files",
            "other-content",
            "other-owner-list",
        ] {
            completion(server, store, &id, deadline).await?;
            let before = snapshot(server, store, &run.artifacts, &id)?;
            write_new(
                &run.receipts.join(format!("{action}-before.txt")),
                format!("{before:?}\n").as_bytes(),
            )?;
            let needs_job = matches!(action, "other-status" | "other-files" | "other-content");
            let result = run
                .call(
                    inputs,
                    action,
                    address,
                    needs_job.then_some(id.as_str()),
                    (action == "other-content").then_some(file),
                    deadline,
                )
                .await;
            write_new(
                &run.receipts.join(format!("{action}-command-result.txt")),
                format!("{result:?}\n").as_bytes(),
            )?;
            // Observe state even when a negative command/assertion fails, before stopping.
            let after = snapshot(server, store, &run.artifacts, &id)?;
            write_new(
                &run.receipts.join(format!("{action}-after.txt")),
                format!("{after:?}\n").as_bytes(),
            )?;
            unchanged(&before, &after)?;
            let (value, actual) = result?;
            refusal_with_caller_echo(action, &value, &actual, &id, Some(file), address)?;
        }
        let capture = run.capture.lock().map_err(problem)?;
        complete_run(
            capture.starts,
            capture.waits,
            capture.events.len(),
            run.polls,
        )
    }

    async fn fixture(inputs: Inputs, mut run: Run) -> Check {
        let (mut listener, mut address, mut server, mut store) = (None, None, None, None);
        let (mut shutdown, mut serving): (
            Option<oneshot::Sender<()>>,
            Option<JoinHandle<std::io::Result<()>>>,
        ) = (None, None);
        let body: Check = async {
            private_directory(&run.artifacts)?;
            private_directory(&run.commands)?;
            let mut version = inputs.python_command();
            version.arg("--version");
            let output = tokio::task::spawn_blocking(move || version.output())
                .await
                .map_err(problem)?
                .map_err(problem)?;
            write_new(&run.receipts.join("python-version.stdout"), &output.stdout)?;
            write_new(&run.receipts.join("python-version.stderr"), &output.stderr)?;
            require(
                output.status.success() && output.stdout == b"Python 3.12.13\n",
                "Python version differs",
            )?;
            let held = TcpListener::bind("127.0.0.1:0").await.map_err(problem)?;
            let actual_address = held.local_addr().map_err(problem)?;
            address = Some(actual_address);
            listener = Some(held);
            require(
                actual_address.port() != 0 && actual_address.ip().is_loopback(),
                "held listener address differs",
            )?;
            let config = ServerConfig {
                listen: actual_address.to_string(),
                store_profile: StoreProfile::Memory,
                artifact_root: run.artifacts.clone(),
                artifact_profile: ArtifactProfile::Local,
                timeout_millis: 5000,
                shutdown_millis: 5000,
                tls: TlsConfig {
                    enabled: false,
                    certificate_path: None,
                    private_key_reference: None,
                },
                ..ServerConfig::default()
            };
            write_new(
                &run.receipts.join("server-config.json"),
                &serde_json::to_vec_pretty(&config).map_err(problem)?,
            )?;
            let memory = Arc::new(MemoryStore::new(Default::default()));
            store = Some(memory.clone());
            let product = ProductServer::open(
                config,
                memory.clone(),
                Arc::new(MemorySecretResolver::default()),
                default_program_router(),
            )
            .map_err(problem)?;
            server = Some(product.clone());
            product
                .bootstrap_administrator("IBMUSER", b"TESTPASS")
                .map_err(problem)?;
            product
                .bootstrap_identity("OTHERUSR", b"OTHERPASS")
                .map_err(problem)?;
            product
                .racf_service()
                .permit("JESJOBS", "JOB.**", "OTHERUSR", AccessIntent::Read)
                .map_err(problem)?;
            product.start_background_workers().map_err(problem)?;
            let router = product
                .router()
                .layer(middleware::from_fn_with_state(run.capture.clone(), observe));
            let (signal, receiver) = oneshot::channel();
            shutdown = Some(signal);
            let held = listener.take().ok_or("held listener missing")?;
            serving = Some(tokio::spawn(async move {
                axum::serve(held, router)
                    .with_graceful_shutdown(async {
                        match receiver.await {
                            Ok(()) => (),
                            Err(error) => eprintln!("shutdown sender lost: {error}"),
                        }
                    })
                    .await
            }));
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut ready = false;
            for observation in 0..50 {
                require(
                    Instant::now() < deadline
                        && !serving.as_ref().ok_or("serve task missing")?.is_finished(),
                    "readiness deadline or serve failure",
                )?;
                let readiness = product.readiness();
                println!("public-client readiness {observation}: {readiness:?}");
                if readiness.ready() {
                    ready = true;
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            require(ready, "full ProductServer readiness unavailable")?;
            workload(&mut run, &inputs, &product, &memory, actual_address).await
        }
        .await;
        let mut failures = Vec::new();
        let mut cleanup = String::new();
        if let Some(signal) = shutdown.take()
            && signal.send(()).is_err()
        {
            failures.push("serve shutdown signal failed".into());
        }
        if let Some(mut task) = serving.take() {
            match timeout(Duration::from_secs(5), &mut task).await {
                Ok(Ok(Ok(()))) => {
                    cleanup.push_str("serve joined successfully within five seconds\n")
                }
                Ok(result) => failures.push(format!("joined serve failed: {result:?}")),
                Err(error) => {
                    failures.push(format!("serve join timeout: {error}"));
                    task.abort();
                    match task.await {
                        Err(error) if error.is_cancelled() => (),
                        result => failures.push(format!("aborted serve await differs: {result:?}")),
                    }
                }
            }
        }
        if let Some(product) = &server {
            match timeout(Duration::from_secs(6), product.graceful_shutdown()).await {
                Ok(true) => cleanup
                    .push_str("ProductServer graceful shutdown returned true within six seconds\n"),
                result => failures.push(format!("ProductServer shutdown failed: {result:?}")),
            }
            let metrics = product.metrics();
            cleanup.push_str(&format!("shutdown metrics: {metrics:?}\n"));
            if metrics.active != 0 || metrics.jes_active != 0 {
                failures.push(format!("owned work remains: {metrics:?}"));
            }
            if let Some(memory) = &store {
                match product.batch_service().list(None, None, 2) {
                    Ok((jobs, more)) => {
                        if more {
                            failures.push("cleanup job membership exceeded bounded scan".into());
                        }
                        for job in jobs {
                            let work = memory.get_work(&format!("jes:{}", job.id));
                            cleanup.push_str(&format!(
                                "cleanup job {} state={:?} work={work:?}\n",
                                job.id, job.state
                            ));
                            if !matches!(
                                job.state,
                                JobState::Completed | JobState::Failed | JobState::Cancelled
                            ) || !matches!(work, Ok(Some(work)) if work.state.terminal())
                            {
                                failures.push(format!("accepted JES work remains for {}", job.id));
                            }
                        }
                    }
                    Err(error) => failures.push(format!("cleanup job observation failed: {error}")),
                }
            }
        }
        drop(listener.take());
        drop(server.take());
        drop(store.take());
        if let Some(address) = address {
            match timeout(Duration::from_secs(1), TcpStream::connect(address)).await {
                Ok(Err(error)) if error.kind() == std::io::ErrorKind::ConnectionRefused => {
                    cleanup.push_str(&format!("former listener {address} refused connection\n"))
                }
                result => failures.push(format!("old listener did not refuse: {result:?}")),
            }
        }
        if let Err(error) = run.retain_http("final") {
            failures.push(error);
        }
        match run.capture.lock() {
            Ok(capture) => {
                failures.extend(capture.failures.clone());
                if capture.starts != capture.launcher_waits {
                    failures.push("a launched Python transport was not waited".into());
                }
            }
            Err(error) => failures.push(problem(error)),
        }
        for (directory, name, depth) in [
            (&run.artifacts, "final-artifacts", 3),
            (&run.commands, "final-commands", 5),
        ] {
            if directory.exists() {
                // Commands are retained individually, avoiding a whole-run inventory overflow.
                let retained = if name == "final-commands" {
                    retain_commands(directory, &run.receipts.join(name))
                } else {
                    retain_inventory(directory, &run.receipts.join(name), depth)
                };
                match retained {
                    Ok(()) => {
                        match canonical(directory)
                            .and_then(|()| fs::remove_dir_all(directory).map_err(problem))
                        {
                            Ok(()) if !directory.exists() => cleanup.push_str(&format!(
                                "retained and removed exact owned directory {}\n",
                                directory.display()
                            )),
                            result => failures
                                .push(format!("exact owned directory deletion failed: {result:?}")),
                        }
                    }
                    Err(error) => failures.push(format!("retain before cleanup failed: {error}")),
                }
            }
        }
        cleanup.push_str(&format!("cleanup failures: {failures:?}\n"));
        if let Err(error) = write_new(&run.receipts.join("cleanup.txt"), cleanup.as_bytes()) {
            failures.push(format!("cleanup result retention failed: {error}"));
        }
        let result = cleanup_result(body, &failures);
        match write_new(
            &run.root.join("result.txt"),
            format!("{result:?}\n").as_bytes(),
        ) {
            Ok(()) => (),
            Err(error) => {
                return cleanup_result(
                    result,
                    &[format!("final result retention failed: {error}")],
                );
            }
        }
        result
    }

    fn retain_commands(root: &Path, destination: &Path) -> Check {
        private_directory(destination)?;
        let mut count = 0;
        for entry in fs::read_dir(root).map_err(problem)? {
            let entry = entry.map_err(problem)?;
            count += 1;
            require(
                count <= 21 && entry.file_type().map_err(problem)?.is_dir(),
                "command membership differs",
            )?;
            retain_inventory(&entry.path(), &destination.join(entry.file_name()), 4)?;
        }
        Ok(())
    }

    pub(super) async fn exercise() -> Check {
        match Inputs::load() {
            Ok(inputs) => match Run::create(&inputs) {
                Ok(run) => fixture(inputs, run).await,
                Err(error) => Err(error),
            },
            Err(error) => Err(error),
        }
    }
}
