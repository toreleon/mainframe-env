//! Frozen first-party keys plus the existing typed host representation.
use super::*;
use mainframe_env_execution_api::{AuditResourceDigest, AuditResourceDigestFormat};
use mainframe_env_host_api::{HostRequest, canonical_audit_resource_digest};

pub(super) struct ReadOracle {
    pub(super) logical: &'static str,
    pub(super) dataset: &'static str,
    pub(super) key: &'static [u8],
    pub(super) encoded_key: &'static [u8],
    pub(super) record_length: usize,
    pub(super) into: &'static str,
    pub(super) response: &'static str,
    pub(super) response2: &'static str,
    pub(super) cics_hash: &'static str,
    pub(super) attributes_hash: &'static str,
    pub(super) dataset_hash: &'static str,
}

pub(super) const ACCOUNT_READS: [ReadOracle; 3] = [
    ReadOracle {
        logical: "CXACAIX ",
        dataset: "AWS.M2.CARDDEMO.CARDXREF.VSAM.AIX.PATH",
        key: b"00000000050",
        encoded_key: b"\xf0\xf0\xf0\xf0\xf0\xf0\xf0\xf0\xf0\xf5\xf0",
        record_length: 50,
        into: "CARD-XREF-RECORD",
        response: "WS-MISC-STORAGE.WS-CICS-PROCESSNG-VARS.WS-RESP-CD",
        response2: "WS-MISC-STORAGE.WS-CICS-PROCESSNG-VARS.WS-REAS-CD",
        cics_hash: "9ef8e833cb962f60bf6123307caaeec1df124591d53adc46264e4278a28d2a86",
        attributes_hash: "f33a7b7359c64abec5f38fb53a4665b4c6b92d9cefa343977d962c0e28610954",
        dataset_hash: "084894baa9fa629f946c2e1b566f97e72a7a173735238a188b78602139b3d1f1",
    },
    ReadOracle {
        logical: "ACCTDAT ",
        dataset: "AWS.M2.CARDDEMO.ACCTDATA.VSAM.KSDS",
        key: b"00000000050",
        encoded_key: b"\xf0\xf0\xf0\xf0\xf0\xf0\xf0\xf0\xf0\xf5\xf0",
        record_length: 300,
        into: "ACCOUNT-RECORD",
        response: "WS-MISC-STORAGE.WS-CICS-PROCESSNG-VARS.WS-RESP-CD",
        response2: "WS-MISC-STORAGE.WS-CICS-PROCESSNG-VARS.WS-REAS-CD",
        cics_hash: "890a196627f46934d0e5812eb838263b11ea6957daef109a6105e448a8f5fe08",
        attributes_hash: "be590a7ace82bc38383e2bf0fd67d076ecbfebea8131fe27cf9addb776fafcdc",
        dataset_hash: "57eb3a186cd34c03425769a52ec6948e898fd5b73f724ff29a18777a40fb4836",
    },
    ReadOracle {
        logical: "CUSTDAT ",
        dataset: "AWS.M2.CARDDEMO.CUSTDATA.VSAM.KSDS",
        key: b"000000050",
        encoded_key: b"\xf0\xf0\xf0\xf0\xf0\xf0\xf0\xf5\xf0",
        record_length: 500,
        into: "CUSTOMER-RECORD",
        response: "WS-MISC-STORAGE.WS-CICS-PROCESSNG-VARS.WS-RESP-CD",
        response2: "WS-MISC-STORAGE.WS-CICS-PROCESSNG-VARS.WS-REAS-CD",
        cics_hash: "a3f258ba713185148b3ff590b199c0760551699f6ed4662f9b8249b1943c8bc6",
        attributes_hash: "58bb9e655bf445c6e7b6b202e3396a74d41facbe39a073b933748b18a3e88d42",
        dataset_hash: "494f7a5cbe17cba29acaa03f512768ad351e7e6d5f811d0185367321ff06ee63",
    },
];

pub(super) const SIGNON_READ: [ReadOracle; 1] = [ReadOracle {
    logical: "USRSEC  ",
    dataset: "AWS.M2.CARDDEMO.USRSEC.VSAM.KSDS",
    key: b"USER0001",
    encoded_key: b"\xe4\xe2\xc5\xd9\xf0\xf0\xf0\xf1",
    record_length: 80,
    into: "SEC-USER-DATA",
    response: "WS-VARIABLES.WS-RESP-CD",
    response2: "WS-VARIABLES.WS-REAS-CD",
    cics_hash: "bf3bbb33b5d6ee845bbedd36f045eb7ce2c6ffe0fa0486d954917f3437d835c0",
    attributes_hash: "b101ce95f37bf677268ef4e649860b75507263f56a6351434a1d953b982aa227",
    dataset_hash: "5f4919e8d871926f3ef7ff1f5b5a3102a51a62956a6a6f79c3f582d45ae72b44",
}];

const INPUTS: &[(&str, &str)] = &[
    (
        "app/cbl/COACTVWC.cbl",
        "4f1e55176f69edfb55b7af7a161aceb8c3bc3bd1b2924a704a76256e0e751b54",
    ),
    (
        "app/cbl/COSGN00C.cbl",
        "4f901ae6b113eebae60ae1d94b30f8cb9906d031a4419770bf56980bdf97e377",
    ),
    (
        "app/cbl/COMEN01C.cbl",
        "c77747b4caaf00b45177a6449562b7f59ec336db832937595edef8b32fdce6fa",
    ),
    (
        "app/cpy/CVACT01Y.cpy",
        "81a08bad15af5664326a6f0af3650f570821c4857ffdec3a6a39f91f07dca728",
    ),
    (
        "app/cpy/CVACT03Y.cpy",
        "ffc6079e09b28739e154bf6c1e1c36d408209faa91f6cf7008078dc596a1c370",
    ),
    (
        "app/cpy/CVCUS01Y.cpy",
        "944fd9a8eb10a683d0db97b9a87f2151982f649970287eb74358826056af25f8",
    ),
    (
        "app/cpy/CSUSR01Y.cpy",
        "37c34094dc4124bc6255e0b06a0387442f4f0dd9d51dd6623b559d370aa0d2cb",
    ),
    (
        "app/csd/CARDDEMO.CSD",
        "28ded93b49e188ae0100d6a964bd921a4de33414662e670b28f19bee41917cdb",
    ),
    (
        "app/data/EBCDIC/AWS.M2.CARDDEMO.ACCTDATA.PS",
        "23167cdff65ca6dfa2e5bccee89112e3e83a80247a38e251f3981377ab098ec9",
    ),
    (
        "app/data/EBCDIC/AWS.M2.CARDDEMO.CARDXREF.PS",
        "b07ab2e5fa2500501c1dcaffe478ee74c4562a290174020a093dfa6f0e6325d3",
    ),
    (
        "app/data/EBCDIC/AWS.M2.CARDDEMO.CUSTDATA.PS",
        "0435915cd35ea3077bad9d391fea1520bc7f30ec06d6e29fbe68290f3d6f27f3",
    ),
    (
        "app/data/EBCDIC/AWS.M2.CARDDEMO.USRSEC.PS",
        "8608d4b8a03f7587c1493282f9337ab24fdf527d0aadbdb85dc3713dccdc6fc8",
    ),
];

pub(super) fn check_inputs(corpus: &Path) -> Result<(), CorpusProblem> {
    for (path, digest) in INPUTS {
        let bytes = read_corpus_file(corpus, &corpus.join(path))?;
        if format!("{:x}", Sha256::digest(bytes)) != *digest {
            return Err(refuse());
        }
    }
    Ok(())
}

fn argument(schema: &str, bytes: &[u8]) -> BoundedPayload {
    BoundedPayload::new(schema, bytes.to_vec(), InvocationLimits::default())
        .expect("frozen bounded first-party request operand")
}

fn frozen_digest(request: HostRequest, frozen: &str) -> Result<AuditResourceDigest, CorpusProblem> {
    let digest = canonical_audit_resource_digest(&request);
    if digest.format != AuditResourceDigestFormat::CanonicalHostResourceV1
        || digest
            .value
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
            != frozen
    {
        return Err(refuse());
    }
    Ok(digest)
}

impl ReadOracle {
    pub(super) fn cics_request(&self) -> HostRequest {
        let mut arguments = BTreeMap::new();
        for (name, value) in [("DATASET", self.logical.as_bytes()), ("RIDFLD", self.key)] {
            arguments.insert(
                name.into(),
                argument("mainframe-env.cics.storage-value@1", value),
            );
        }
        for (name, value) in [
            ("KEYLENGTH", self.key.len()),
            ("LENGTH", self.record_length),
        ] {
            arguments.insert(
                name.into(),
                argument("mainframe-env.cics.decimal@1", value.to_string().as_bytes()),
            );
        }
        for (name, value) in [
            ("INTO", self.into),
            ("RESP", self.response),
            ("RESP2", self.response2),
        ] {
            arguments.insert(
                name.into(),
                argument("mainframe-env.cics.argument@1", value.as_bytes()),
            );
        }
        HostRequest::Cics(CicsRequest {
            operation: CicsOperation::Read,
            arguments,
            condition_policy: CicsConditionPolicy::Respond {
                response_field: self.response.into(),
                response2_field: Some(self.response2.into()),
            },
            mutation: None,
        })
    }

    pub(super) fn dataset_request(&self) -> HostRequest {
        HostRequest::Dataset(DatasetRequest::Read {
            dataset: DatasetName::new(self.dataset, 128).expect("frozen dataset"),
            member: None,
            key: Some(self.encoded_key.to_vec()),
            max_records: 1,
            control: Default::default(),
        })
    }

    pub(super) fn cics_digest(&self) -> Result<AuditResourceDigest, CorpusProblem> {
        frozen_digest(self.cics_request(), self.cics_hash)
    }

    pub(super) fn dataset_digest(&self) -> Result<AuditResourceDigest, CorpusProblem> {
        frozen_digest(self.dataset_request(), self.dataset_hash)
    }

    pub(super) fn attributes_digest(&self) -> Result<AuditResourceDigest, CorpusProblem> {
        frozen_digest(
            HostRequest::Dataset(DatasetRequest::Attributes {
                dataset: DatasetName::new(self.dataset, 128).expect("frozen dataset"),
            }),
            self.attributes_hash,
        )
    }
}
