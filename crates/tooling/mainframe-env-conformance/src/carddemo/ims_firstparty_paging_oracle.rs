//! Separate seven-child source-authored oracle; the old six-b71 reader is unchanged.
use super::*;
use crate::carddemo::owned_source_receipt::require_owned_directory;
use mainframe_env_execution_api::AuditResourceDigest;
use mainframe_env_host_api::HostRequest;
use std::io::Read;
use std::os::unix::fs::{MetadataExt, PermissionsExt};

pub(super) type SegmentImage = (String, Option<Vec<u8>>, Vec<u8>);
const ORACLE_SHA256: &str = "672209a28c5eee08a2d417ea7b997cb01fd1c414ac3004551b1834e5c1ec7c05";

const FIELD_LENGTHS: &[(&str, usize)] = &[
    ("ACCSTAT", 1),
    ("ACCTID", 11),
    ("ADDR001", 25),
    ("ADDR002", 25),
    ("APPRAMT", 10),
    ("APPRCNT", 3),
    ("CASHBAL", 9),
    ("CASHLIM", 9),
    ("CNAME", 25),
    ("CREDBAL", 12),
    ("CREDLIM", 12),
    ("CURDATE", 8),
    ("CURTIME", 8),
    ("CUSTID", 9),
    ("DECLAMT", 10),
    ("DECLCNT", 3),
    ("ERRMSG", 78),
    ("PAMT001", 12),
    ("PAMT002", 12),
    ("PAMT003", 12),
    ("PAMT004", 12),
    ("PAMT005", 12),
    ("PAPRV01", 1),
    ("PAPRV02", 1),
    ("PAPRV03", 1),
    ("PAPRV04", 1),
    ("PAPRV05", 1),
    ("PDATE01", 8),
    ("PDATE02", 8),
    ("PDATE03", 8),
    ("PDATE04", 8),
    ("PDATE05", 8),
    ("PGMNAME", 8),
    ("PHONE1", 13),
    ("PSTAT01", 1),
    ("PSTAT02", 1),
    ("PSTAT03", 1),
    ("PSTAT04", 1),
    ("PSTAT05", 1),
    ("PTIME01", 8),
    ("PTIME02", 8),
    ("PTIME03", 8),
    ("PTIME04", 8),
    ("PTIME05", 8),
    ("PTYPE01", 4),
    ("PTYPE02", 4),
    ("PTYPE03", 4),
    ("PTYPE04", 4),
    ("PTYPE05", 4),
    ("SEL0001", 1),
    ("SEL0002", 1),
    ("SEL0003", 1),
    ("SEL0004", 1),
    ("SEL0005", 1),
    ("TITLE01", 40),
    ("TITLE02", 40),
    ("TRNID01", 16),
    ("TRNID02", 16),
    ("TRNID03", 16),
    ("TRNID04", 16),
    ("TRNID05", 16),
    ("TRNNAME", 4),
];

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Call {
    pub(super) operation: String,
    pub(super) qualifier: Option<Vec<u8>>,
    pub(super) status: String,
    pub(super) segments: Vec<SegmentImage>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Frame {
    pub(super) name: String,
    pub(super) aid: u8,
    pub(super) inputs: BTreeMap<String, String>,
    pub(super) fields: BTreeMap<String, Vec<u8>>,
    pub(super) calls: Vec<Call>,
    pub(super) dataset_reads: usize,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PagingOracle {
    schema: String,
    authority: String,
    source_commit: String,
    pub(super) account: String,
    pub(super) clock: String,
    pub(super) root: Vec<u8>,
    pub(super) children: Vec<Vec<u8>>,
    field_lengths: BTreeMap<String, usize>,
    pub(super) frames: Vec<Frame>,
    whole_private_guest_storage_postimage_claimed: bool,
    bms_attribute_or_mdt_claimed: bool,
}

fn problem() -> CorpusProblem {
    refused()
}
fn need(value: bool) -> Result<(), CorpusProblem> {
    if value { Ok(()) } else { Err(refused()) }
}

impl PagingOracle {
    pub(super) fn read(path: &Path) -> Result<Self, CorpusProblem> {
        let parent = path.parent().ok_or_else(problem)?;
        let parent_meta = fs::symlink_metadata(parent).map_err(|_| problem())?;
        require_owned_directory(parent, (parent_meta.dev(), parent_meta.ino()))?;
        let before = fs::symlink_metadata(path).map_err(|_| problem())?;
        need(
            before.is_file()
                && !before.file_type().is_symlink()
                && before.nlink() == 1
                && before.permissions().mode() & 0o777 == 0o600
                && before.len() <= 65_536
                && path.canonicalize().map_err(|_| problem())? == path,
        )?;
        let mut file = fs::File::open(path).map_err(|_| problem())?;
        let held = file.metadata().map_err(|_| problem())?;
        need(
            (
                held.dev(),
                held.ino(),
                held.len(),
                held.mode(),
                held.nlink(),
            ) == (
                before.dev(),
                before.ino(),
                before.len(),
                before.mode(),
                before.nlink(),
            ),
        )?;
        let mut bytes = Vec::new();
        (&mut file)
            .take(65_537)
            .read_to_end(&mut bytes)
            .map_err(|_| problem())?;
        let held_after = file.metadata().map_err(|_| problem())?;
        need(
            (
                held_after.dev(),
                held_after.ino(),
                held_after.len(),
                held_after.mode(),
                held_after.nlink(),
            ) == (
                before.dev(),
                before.ino(),
                before.len(),
                before.mode(),
                before.nlink(),
            ),
        )?;
        let after = fs::symlink_metadata(path).map_err(|_| problem())?;
        require_owned_directory(parent, (parent_meta.dev(), parent_meta.ino()))?;
        need(
            (
                after.dev(),
                after.ino(),
                after.len(),
                after.mode(),
                after.nlink(),
            ) == (
                before.dev(),
                before.ino(),
                before.len(),
                before.mode(),
                before.nlink(),
            ) && bytes.len() as u64 == before.len()
                && format!("{:x}", Sha256::digest(&bytes)) == ORACLE_SHA256,
        )?;
        let oracle: Self = serde_json::from_slice(&bytes).map_err(|_| problem())?;
        oracle.validate()?;
        Ok(oracle)
    }
    pub(super) fn validate(&self) -> Result<(), CorpusProblem> {
        need(
            self.schema == "external.carddemo-copaus0-seven-child-paging-oracle@1"
                && self.source_commit == "59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e"
                && !self.authority.is_empty()
                && self.authority.len() <= 512
                && self.account == "00000000050"
                && self.clock == "2022070600000000+0000"
                && self.root.len() == 100
                && self.root[..6] == [0, 0, 0, 0, 5, 0x0c]
                && self.children.len() == 7
                && self.children.iter().all(|child| child.len() == 200)
                && !self.whole_private_guest_storage_postimage_claimed
                && !self.bms_attribute_or_mdt_claimed
                && self.field_lengths
                    == FIELD_LENGTHS
                        .iter()
                        .map(|(name, width)| ((*name).into(), *width))
                        .collect()
                && self.frames.len() == 5,
        )?;
        let names = [
            "enter-first",
            "pf7-top",
            "pf8-forward",
            "pf8-bottom",
            "pf7-back",
        ];
        let aids = [0x7d, 0xf7, 0xf8, 0xf8, 0xf7];
        let counts = [8, 0, 6, 4, 8];
        let mut keys = std::collections::BTreeSet::new();
        for child in &self.children {
            need(keys.insert(child[..8].to_vec()))?;
        }
        for (index, frame) in self.frames.iter().enumerate() {
            need(
                frame.name == names[index]
                    && frame.aid == aids[index]
                    && frame.calls.len() == counts[index]
                    && frame.dataset_reads == usize::from(index == 0) * 3
                    && frame.inputs.len() == 6
                    && frame.inputs.get("ACCTID") == Some(&self.account)
                    && (1..=5)
                        .all(|i| frame.inputs.get(&format!("SEL000{i}")) == Some(&" ".into()))
                    && frame.fields.len() == 62
                    && frame
                        .fields
                        .iter()
                        .all(|(name, bytes)| self.field_lengths.get(name) == Some(&bytes.len())),
            )?;
            for call in &frame.calls {
                need(
                    matches!(call.operation.as_str(), "SCHD" | "GU" | "GNP")
                        && matches!(call.status.as_str(), "  " | "GE")
                        && call
                            .qualifier
                            .as_ref()
                            .is_none_or(|q| q.len() == if call.operation == "GU" { 6 } else { 8 }),
                )?;
            }
        }
        Ok(())
    }
    pub(super) fn image(&self) -> ImsLoadImage {
        ImsLoadImage {
            database: "DBPAUTP0".into(),
            roots: vec![ImsLoadRoot {
                data: self.root.clone(),
                children: self.children.clone(),
            }],
        }
    }
    pub(super) fn compare_hierarchy(&self, actual: &[SegmentImage]) -> Result<(), CorpusProblem> {
        let mut expected = vec![("PAUTSUM0".into(), None, self.root.clone())];
        expected.extend(self.children.iter().map(|child| {
            (
                "PAUTDTL1".into(),
                Some(self.root[..6].to_vec()),
                child.clone(),
            )
        }));
        need(actual == expected)
    }
    pub(super) fn compare_fields(
        &self,
        index: usize,
        actual: &BTreeMap<String, Vec<u8>>,
    ) -> Result<(), CorpusProblem> {
        need(
            self.frames
                .get(index)
                .is_some_and(|frame| &frame.fields == actual),
        )
    }
}

impl Call {
    fn request(&self, sequence: u64, key: &IdempotencyKey) -> Result<HostRequest, CorpusProblem> {
        let operation = match self.operation.as_str() {
            "SCHD" => ImsOperation::Schedule,
            "GU" => ImsOperation::GetUnique,
            "GNP" => ImsOperation::GetNextParent,
            _ => return Err(refused()),
        };
        let schedule = operation == ImsOperation::Schedule;
        let unique = operation == ImsOperation::GetUnique;
        let segment = if unique { "PAUTSUM0" } else { "PAUTDTL1" };
        need(sequence > 0 && (!schedule || self.qualifier.is_none()))?;
        Ok(HostRequest::Ims(ImsRequest {
            operation,
            psb: schedule.then(|| "PSBPAUTB".into()),
            pcb: 1,
            segments: if schedule {
                vec![]
            } else {
                vec![segment.into()]
            },
            data: vec![],
            qualifiers: self
                .qualifier
                .iter()
                .map(|value| ImsQualifier {
                    segment: segment.into(),
                    field: if unique {
                        "ACCNTID".into()
                    } else {
                        "PAUT9CTS".into()
                    },
                    value: value.clone(),
                })
                .collect(),
            checkpoint_id: None,
            max_segments: 1,
            mutation: Some(Mutation {
                sequence,
                idempotency_key: key.clone(),
                transaction: None,
            }),
            system: None,
            q_class: None,
        }))
    }
    // Failure-only fixed-size booleans/hashes. No expected bytes come from runtime output.
    pub(super) fn failure_metadata(
        &self,
        sequence: u64,
        key: &IdempotencyKey,
        request_digest: [u8; 32],
        resource: &AuditResourceDigest,
        status: &str,
        segments: &[SegmentImage],
    ) -> serde_json::Value {
        let expected = self.request(sequence, key).ok();
        let expected_digest = expected
            .as_ref()
            .and_then(|request| mainframe_env_host_api::canonical_request_digest(request).ok());
        let segment_hash = |rows: &[SegmentImage]| {
            serde_json::to_vec(rows)
                .ok()
                .map(|bytes| format!("{:x}", Sha256::digest(&bytes)))
        };
        serde_json::json!({
            "source_request_constructed": expected.is_some(),
            "request_digest": expected_digest.map(|value| value == request_digest),
            "resource": expected.as_ref().map(|request|
                mainframe_env_host_api::canonical_audit_resource_digest(request) == *resource),
            "status": self.status == status, "segments": self.segments == segments,
            "actual_request_digest_sha256": format!("{:x}", Sha256::digest(request_digest)),
            "expected_request_digest_sha256": expected_digest.map(|value| format!("{:x}", Sha256::digest(value))),
            "actual_status_bytes": status.len(), "actual_status_sha256": format!("{:x}", Sha256::digest(status.as_bytes())),
            "expected_status_bytes": self.status.len(), "expected_status_sha256": format!("{:x}", Sha256::digest(self.status.as_bytes())),
            "actual_segments": segments.len(), "actual_segments_sha256": segment_hash(segments),
            "expected_segments": self.segments.len(), "expected_segments_sha256": segment_hash(&self.segments)
        })
    }
    pub(super) fn compare(
        &self,
        sequence: u64,
        key: &IdempotencyKey,
        request_digest: [u8; 32],
        resource: &AuditResourceDigest,
        status: &str,
        segments: &[SegmentImage],
    ) -> Result<(), CorpusProblem> {
        let expected = self.request(sequence, key)?;
        need(
            mainframe_env_host_api::canonical_request_digest(&expected).map_err(|_| refused())?
                == request_digest
                && mainframe_env_host_api::canonical_audit_resource_digest(&expected) == *resource
                && self.status == status
                && self.segments == segments,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> PagingOracle {
        let mut root = vec![b'R'; 100];
        root[..6].copy_from_slice(&[0, 0, 0, 0, 5, 0x0c]);
        let children = (1..=7).map(|i| vec![i; 200]).collect::<Vec<_>>();
        let widths = FIELD_LENGTHS
            .iter()
            .map(|(name, width)| ((*name).into(), *width))
            .collect::<BTreeMap<String, usize>>();
        let names = [
            "enter-first",
            "pf7-top",
            "pf8-forward",
            "pf8-bottom",
            "pf7-back",
        ];
        let aids = [0x7d, 0xf7, 0xf8, 0xf8, 0xf7];
        let counts = [8, 0, 6, 4, 8];
        let inputs = BTreeMap::from([
            ("ACCTID".into(), "00000000050".into()),
            ("SEL0001".into(), " ".into()),
            ("SEL0002".into(), " ".into()),
            ("SEL0003".into(), " ".into()),
            ("SEL0004".into(), " ".into()),
            ("SEL0005".into(), " ".into()),
        ]);
        let frames = (0..5)
            .map(|index| Frame {
                name: names[index].into(),
                aid: aids[index],
                inputs: inputs.clone(),
                fields: widths
                    .iter()
                    .map(|(name, width)| (name.clone(), vec![b'A' + index as u8; *width]))
                    .collect(),
                calls: (0..counts[index])
                    .map(|_| Call {
                        operation: "GNP".into(),
                        qualifier: None,
                        status: "  ".into(),
                        segments: vec![],
                    })
                    .collect(),
                dataset_reads: usize::from(index == 0) * 3,
            })
            .collect();
        PagingOracle {
            schema: "external.carddemo-copaus0-seven-child-paging-oracle@1".into(),
            authority: "synthetic pure refusal controls, not an admitted runtime oracle".into(),
            source_commit: "59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e".into(),
            account: "00000000050".into(),
            clock: "2022070600000000+0000".into(),
            root,
            children,
            field_lengths: widths,
            frames,
            whole_private_guest_storage_postimage_claimed: false,
            bms_attribute_or_mdt_claimed: false,
        }
    }
    #[test]
    fn complete_seven_child_images_refuse_filler_parent_order_and_width_drift() {
        let oracle = fixture();
        oracle.validate().unwrap();
        let mut expected = vec![("PAUTSUM0".into(), None, oracle.root.clone())];
        expected.extend(oracle.children.iter().map(|child| {
            (
                "PAUTDTL1".into(),
                Some(oracle.root[..6].to_vec()),
                child.clone(),
            )
        }));
        oracle.compare_hierarchy(&expected).unwrap();
        for index in 0..expected.len() {
            let mut changed = expected.clone();
            *changed[index].2.last_mut().unwrap() ^= 1;
            assert!(oracle.compare_hierarchy(&changed).is_err());
            let mut changed = expected.clone();
            changed[index].2.pop();
            assert!(oracle.compare_hierarchy(&changed).is_err());
        }
        let mut changed = expected.clone();
        changed.swap(1, 7);
        assert!(oracle.compare_hierarchy(&changed).is_err());
        let mut changed = expected;
        changed[6].1 = Some(vec![0; 6]);
        assert!(oracle.compare_hierarchy(&changed).is_err());
    }
    #[test]
    fn complete_page_fields_refuse_reused_page_missing_unknown_and_binary_drift() {
        let oracle = fixture();
        for index in 0..5 {
            let expected = &oracle.frames[index].fields;
            oracle.compare_fields(index, expected).unwrap();
            assert!(
                oracle
                    .compare_fields(index, &oracle.frames[(index + 1) % 5].fields)
                    .is_err()
            );
            for name in expected.keys() {
                let mut changed = expected.clone();
                changed.remove(name);
                assert!(oracle.compare_fields(index, &changed).is_err());
                let mut changed = expected.clone();
                changed.get_mut(name).unwrap()[0] ^= 1;
                assert!(oracle.compare_fields(index, &changed).is_err());
            }
            let mut changed = expected.clone();
            changed.insert("UNKNOWN".into(), vec![0]);
            assert!(oracle.compare_fields(index, &changed).is_err());
        }
    }
    #[test]
    fn source_phase_contract_refuses_missing_forward_back_aid_clock_or_input_changes() {
        // Exercise the same production route selector: an explicit bad offer
        // refuses without invoking the legacy owner or yielding observations.
        let paging_calls = std::cell::Cell::new(0);
        let legacy_calls = std::cell::Cell::new(0);
        let paging = || {
            paging_calls.set(paging_calls.get() + 1);
            Ok::<_, CorpusProblem>(7)
        };
        let legacy = || {
            legacy_calls.set(legacy_calls.get() + 1);
            Ok::<_, CorpusProblem>(5)
        };
        assert_eq!(
            super::super::super::select_input_route(false, paging, legacy).unwrap(),
            5
        );
        assert_eq!((paging_calls.get(), legacy_calls.get()), (0, 1));
        assert_eq!(
            super::super::super::select_input_route(true, paging, legacy).unwrap(),
            7
        );
        assert_eq!((paging_calls.get(), legacy_calls.get()), (1, 1));
        let refused_offer =
            super::super::super::select_input_route(true, || Err::<usize, _>(refused()), legacy);
        assert!(refused_offer.is_err());
        assert_eq!(legacy_calls.get(), 1);
        let oracle = fixture();
        oracle.validate().unwrap();
        for variant in 0..8 {
            let mut changed = oracle.clone();
            match variant {
                0 => {
                    changed.frames.remove(2);
                }
                1 => changed.frames.swap(2, 4),
                2 => changed.frames[2].aid = 0x7d,
                3 => {
                    changed.frames[3].calls.pop().unwrap();
                }
                4 => changed.clock = "2022070700000000+0000".into(),
                5 => {
                    changed.frames[0]
                        .inputs
                        .insert("SEL0001".into(), "S".into());
                }
                6 => changed.children[1] = changed.children[0].clone(),
                _ => {
                    changed.field_lengths.insert("UNKNOWN".into(), 1);
                }
            }
            assert!(changed.validate().is_err());
        }
        // Source-persisted NEXT-Y takes the second PF8 through INIT and one
        // final GNP/GE. The existing variant3 above also refuses dropping call4.
        let final_get = Call {
            operation: "GNP".into(),
            qualifier: None,
            status: "GE".into(),
            segments: vec![],
        };
        let key = IdempotencyKey::new("source-bottom-ge", InvocationLimits::default()).unwrap();
        let request = final_get.request(4, &key).unwrap();
        let digest = mainframe_env_host_api::canonical_request_digest(&request).unwrap();
        let resource = mainframe_env_host_api::canonical_audit_resource_digest(&request);
        final_get
            .compare(4, &key, digest, &resource, "GE", &[])
            .unwrap();
        assert!(
            final_get
                .compare(4, &key, digest, &resource, "  ", &[])
                .is_err()
        );
        let extra_segment = vec![("PAUTDTL1".into(), Some(vec![0; 6]), vec![0; 200])];
        assert!(
            final_get
                .compare(4, &key, digest, &resource, "GE", &extra_segment)
                .is_err()
        );
    }
    #[test]
    fn qualified_reposition_requests_refuse_operation_key_audit_status_and_bytes_substitution() {
        let key =
            IdempotencyKey::new("synthetic-paging-control", InvocationLimits::default()).unwrap();
        for index in 0..8 {
            let first = Call {
                operation: if index == 0 {
                    "SCHD"
                } else if index == 1 {
                    "GU"
                } else {
                    "GNP"
                }
                .into(),
                qualifier: (index == 1).then(|| vec![0, 0, 0, 0, 5, 0x0c]),
                status: "  ".into(),
                segments: vec![],
            };
            let request = first.request(41, &key).unwrap();
            let expected =
                super::super::super::expected_request(index, 41, &key, &[0, 0, 0, 0, 5, 0x0c])
                    .unwrap();
            let digest = mainframe_env_host_api::canonical_request_digest(&request).unwrap();
            let resource = mainframe_env_host_api::canonical_audit_resource_digest(&request);
            super::super::super::compare_request(&expected, digest, &resource).unwrap();
            let mut changed = first.clone();
            changed.operation = if index == 0 { "GU" } else { "SCHD" }.into();
            changed.qualifier = None;
            let wrong = changed.request(41, &key).unwrap();
            assert!(
                super::super::super::compare_request(
                    &expected,
                    mainframe_env_host_api::canonical_request_digest(&wrong).unwrap(),
                    &mainframe_env_host_api::canonical_audit_resource_digest(&wrong)
                )
                .is_err()
            );
        }
        let call = Call {
            operation: "GNP".into(),
            qualifier: Some(vec![1; 8]),
            status: "  ".into(),
            segments: vec![("PAUTDTL1".into(), Some(vec![2; 6]), vec![3; 200])],
        };
        let request = call.request(41, &key).unwrap();
        let digest = mainframe_env_host_api::canonical_request_digest(&request).unwrap();
        let resource = mainframe_env_host_api::canonical_audit_resource_digest(&request);
        call.compare(41, &key, digest, &resource, "  ", &call.segments)
            .unwrap();
        let other =
            IdempotencyKey::new("other-paging-control", InvocationLimits::default()).unwrap();
        assert!(
            call.compare(41, &other, digest, &resource, "  ", &call.segments)
                .is_err()
        );
        assert!(
            call.compare(42, &key, digest, &resource, "  ", &call.segments)
                .is_err()
        );
        assert!(
            call.compare(41, &key, [0; 32], &resource, "  ", &call.segments)
                .is_err()
        );
        assert!(
            call.compare(41, &key, digest, &resource, "GE", &call.segments)
                .is_err()
        );
        let mut changed = call.clone();
        changed.qualifier.as_mut().unwrap()[0] ^= 1;
        assert!(
            changed
                .compare(41, &key, digest, &resource, "  ", &call.segments)
                .is_err()
        );
        changed.operation = "GU".into();
        let wrong = changed.request(41, &key).unwrap();
        let wrong_resource = mainframe_env_host_api::canonical_audit_resource_digest(&wrong);
        assert!(
            call.compare(41, &key, digest, &wrong_resource, "  ", &call.segments)
                .is_err()
        );
        let mut bytes = call.segments.clone();
        bytes[0].2[199] ^= 1;
        assert!(
            call.compare(41, &key, digest, &resource, "  ", &bytes)
                .is_err()
        );
    }
}
