//! Independent source-layout scenario and comparison only. It emits no journey token.
use super::need;
use crate::carddemo::CorpusProblem;
use mainframe_env_host_api::{ImsOperation, ImsRequest, ImsResult, ImsSegment};
use mainframe_env_ims::ImsLoadRoot;

#[derive(Clone)]
pub(super) struct Scenario {
    pub(super) current_yyddd: u32,
    pub(super) before: Vec<ImsLoadRoot>,
    pub(super) remaining: Vec<ImsLoadRoot>,
}

#[derive(Clone)]
pub(super) struct Call {
    pub(super) operation: ImsOperation,
    pub(super) pcb: u16,
    pub(super) segments: Vec<String>,
    pub(super) data: Vec<u8>,
    pub(super) status: String,
    pub(super) returned: Vec<ImsSegment>,
    pub(super) checkpoint: Option<String>,
    pub(super) affected_segments: u64,
}

// Fixture encoding is explicit COPY data, not a provider call or guest output.
// Only positive values are authored here; no new product numeric semantics.
fn packed(value: u64, digits: usize) -> Result<Vec<u8>, CorpusProblem> {
    let text = format!("{value:0digits$}");
    need(
        text.len() == digits && digits <= 12,
        "fixture decimal width differs",
    )?;
    let mut nibbles = Vec::with_capacity(digits + 2);
    if digits.is_multiple_of(2) {
        nibbles.push(0);
    }
    nibbles.extend(text.bytes().map(|byte| byte - b'0'));
    nibbles.push(0x0c);
    Ok(nibbles
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| pair[0] << 4 | pair[1])
        .collect())
}

fn summary(
    account: u64,
    approved: i16,
    declined: i16,
    amount: u64,
    rejected: u64,
) -> Result<Vec<u8>, CorpusProblem> {
    let mut bytes = vec![b'R'; 100];
    bytes[..6].copy_from_slice(&packed(account, 11)?);
    bytes[6..15].copy_from_slice(b"000000050");
    bytes[15] = b'A';
    bytes[16..26].copy_from_slice(b"0000000000");
    for (offset, value) in [(26, 500_000), (32, 100_000), (38, 20_000), (44, 0)] {
        bytes[offset..offset + 6].copy_from_slice(&packed(value, 11)?);
    }
    bytes[50..52].copy_from_slice(&approved.to_be_bytes());
    bytes[52..54].copy_from_slice(&declined.to_be_bytes());
    bytes[54..60].copy_from_slice(&packed(amount, 11)?);
    bytes[60..66].copy_from_slice(&packed(rejected, 11)?);
    Ok(bytes)
}

fn detail(day: u32, time_key: u64, approved: bool, cents: u64) -> Result<Vec<u8>, CorpusProblem> {
    need(day <= 99_999, "fixture day outside source field")?;
    let mut bytes = vec![b' '; 200];
    bytes[..3].copy_from_slice(&packed(u64::from(99_999 - day), 5)?);
    bytes[3..8].copy_from_slice(&packed(time_key, 9)?);
    let original_date: &[u8; 6] = match day {
        26_288 => b"261015",
        26_290 => b"261017",
        _ => {
            return Err(super::problem(
                "fixture date is not one of the independently declared dates",
            ));
        }
    };
    bytes[8..14].copy_from_slice(original_date);
    bytes[14..20].copy_from_slice(b"120000");
    bytes[20..36].copy_from_slice(b"4000000000000050");
    bytes[36..40].copy_from_slice(b"SALE");
    bytes[40..44].copy_from_slice(b"2912");
    bytes[44..50].copy_from_slice(b"020000");
    bytes[50..56].copy_from_slice(b"J18REF");
    bytes[56..62].copy_from_slice(b"000050");
    bytes[62..64].copy_from_slice(if approved { b"00" } else { b"05" });
    bytes[64..68].copy_from_slice(b"0000");
    bytes[68..74].copy_from_slice(b"000000");
    bytes[74..81].copy_from_slice(&packed(cents, 12)?);
    bytes[81..88].copy_from_slice(&packed(if approved { cents } else { 0 }, 12)?);
    bytes[88..92].copy_from_slice(b"5999");
    bytes[92..95].copy_from_slice(b"840");
    bytes[95..97].copy_from_slice(b"01");
    bytes[97..112].copy_from_slice(b"J18MERCHANT0001");
    bytes[112..134].copy_from_slice(b"INDEPENDENTJ18 FIXTURE");
    bytes[134..147].copy_from_slice(b"TEST CITY    ");
    bytes[147..149].copy_from_slice(b"NY");
    bytes[149..158].copy_from_slice(b"100000000");
    let transaction = format!("J18-{time_key:011}");
    bytes[158..173].copy_from_slice(transaction.as_bytes());
    bytes[173] = if approved { b'P' } else { b'D' };
    bytes[174] = b'R';
    bytes[175..183].copy_from_slice(b"20261016");
    bytes[183..].fill(b'Z');
    Ok(bytes)
}

/// Author-before-execution conditional case for the unmodified SYSIN expiry00.
/// `remaining` assumes normal completion with the declared date and successful
/// deletes. No real clock binding, BMP allocation, call schedule or IBM hold
/// semantics is supplied or proved by this data; it is not an execution oracle.
pub(super) fn mixed() -> Result<Scenario, CorpusProblem> {
    let current_yyddd = 26_289;
    let future_a = detail(26_290, 1, true, 10_000)?;
    let future_d = detail(26_290, 2, false, 5_000)?;
    let expired_a = detail(26_288, 3, true, 2_500)?;
    let expired_d = detail(26_288, 4, false, 1_500)?;
    let expired_only = detail(26_288, 5, true, 3_000)?;
    let neighbor = detail(26_290, 6, true, 7_000)?;
    let before = vec![
        ImsLoadRoot {
            data: summary(50, 2, 2, 12_500, 6_500)?,
            children: vec![
                future_a.clone(),
                future_d.clone(),
                expired_a.clone(),
                expired_d.clone(),
            ],
        },
        ImsLoadRoot {
            data: summary(51, 1, 0, 3_000, 0)?,
            children: vec![expired_only.clone()],
        },
        ImsLoadRoot {
            data: summary(52, 1, 0, 7_000, 0)?,
            children: vec![neighbor.clone()],
        },
    ];
    let remaining = vec![
        ImsLoadRoot {
            data: before[0].data.clone(),
            children: vec![future_a.clone(), future_d.clone()],
        },
        before[2].clone(),
    ];
    // No checkpoint schedule or successful mutation trace is generated here.
    // P-CHKP-FREQ is PIC X(05), and EXEC DLI update/hold/BMP association needs
    // verified references before an execution oracle may settle those facts.
    Ok(Scenario {
        current_yyddd,
        before,
        remaining,
    })
}

impl Scenario {
    /// Pure source/request/byte comparison only. A future trusted owner must
    /// separately authenticate artifact, invocation, effect journal, audit,
    /// selected BMP association, process teardown and reopen provenance.
    /// `reference` must be independently admitted by the future owner. This
    /// pure comparator neither generates a schedule from observed guest output
    /// nor gives a caller-authored expected list any execution authority.
    pub(super) fn compare_calls(
        &self,
        reference: &[Call],
        actual: &[(&ImsRequest, &ImsResult)],
    ) -> Result<(), CorpusProblem> {
        need(
            !reference.is_empty() && reference.len() <= 512 && actual.len() == reference.len(),
            "missing, extra or unbounded reference/source DLI calls",
        )?;
        for ((request, result), expected) in actual.iter().zip(reference) {
            let checkpoint = expected.operation == ImsOperation::Checkpoint;
            need(
                (checkpoint
                    && expected.pcb == 1
                    && expected.checkpoint.as_deref() == Some("RMAD0000")
                    && expected.segments.is_empty()
                    && expected.data.is_empty())
                    || (!checkpoint
                        && expected.pcb == 2
                        && expected.checkpoint.is_none()
                        && match expected.operation {
                            ImsOperation::GetNext => {
                                expected.segments == ["PAUTSUM0"] && expected.data.is_empty()
                            }
                            ImsOperation::GetNextParent => {
                                expected.segments == ["PAUTDTL1"] && expected.data.is_empty()
                            }
                            ImsOperation::Delete => {
                                (expected.segments == ["PAUTSUM0"] && expected.data.len() == 100)
                                    || (expected.segments == ["PAUTDTL1"]
                                        && expected.data.len() == 200)
                            }
                            _ => false,
                        }),
                "reference does not describe original source DLI forms",
            )?;
            need(
                request.operation == expected.operation
                    && request.pcb == expected.pcb
                    && request.psb.is_none()
                    && request.segments == expected.segments
                    && request.data == expected.data
                    && request.qualifiers.is_empty()
                    && request.max_segments == 1
                    && request.mutation.is_some()
                    && request.system.is_none()
                    && request.q_class.is_none()
                    && request.checkpoint_id == expected.checkpoint
                    && result.status == expected.status
                    && result.segments == expected.returned
                    && result.checkpoint_id == expected.checkpoint
                    && result.affected_segments == expected.affected_segments
                    && result.system.is_none(),
                "source DLI request or complete transferred bytes differ",
            )?;
        }
        Ok(())
    }
    pub(super) fn compare_hierarchy(&self, actual: &[ImsLoadRoot]) -> Result<(), CorpusProblem> {
        need(
            actual == self.remaining,
            "complete ordered hierarchy or unchanged summary/neighbor bytes differ",
        )
    }
    pub(super) fn source_contract_gaps(&self) -> [&'static str; 2] {
        // These original predicates remain unsatisfied, not optional. A wrapper
        // write or a provider/database reopen cannot fill either requirement.
        ["available-credit adjustment", "restart from checkpoint"]
    }
}
