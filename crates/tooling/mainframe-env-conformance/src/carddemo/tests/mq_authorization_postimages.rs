//! Conditional source projections for COPAUA0C, never application/UOW evidence.
//!
//! The caller independently authors and admits every preimage, including guest
//! working storage and clock. Nothing here is derived from guest output or
//! invokes a provider. Real source/artifact/effect/audit/cleanup ownership stays
//! with the first-party owner. No RouteObservations or receipt can be returned.
use super::{Case, RequestOffer, SeedImage};
use mainframe_env_host_api::{ImsOperation, ImsQualifier, ImsRequest};
use mainframe_env_ims::ImsLoadRoot;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Refusal {
    Bounds,
    Fixture,
    Decimal,
    Decision,
    StaleSummaryBranch,
    Postimage,
    DuplicateNeedsDisposition,
}

/// Independent fixture preconditions, not authenticated live guest state.
#[derive(Clone)]
pub(super) struct Before {
    pub summary_work: Vec<u8>,
    pub detail_work: Vec<u8>,
    pub hierarchy: Vec<ImsLoadRoot>,
    pub previous_summary_found: bool,
    pub yyddd: u32,
    pub hhmmss: u32,
    pub milliseconds: u16,
}

/// Bytes the source would offer only if all preceding steps succeed. This is
/// neither a fabricated EffectRecord nor a statement that an offer occurred.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct WriteImage {
    pub operation: ImsOperation,
    pub pcb: u16,
    pub segments: Vec<String>,
    pub root_key: Option<Vec<u8>>,
    pub data: Vec<u8>,
}

pub(super) struct Projection {
    pub unchanged_vsam: Vec<SeedImage>,
    pub conditional_writes: Vec<WriteImage>,
    pub summary_work: Vec<u8>,
    pub detail_work: Vec<u8>,
    // None means source key collision: do not guess commit/backout outcome.
    pub conditional_success_hierarchy: Option<Vec<ImsLoadRoot>>,
}

fn need(value: bool, error: Refusal) -> Result<(), Refusal> {
    if value { Ok(()) } else { Err(error) }
}
fn digits(bytes: &[u8]) -> Result<u64, Refusal> {
    need(!bytes.is_empty() && bytes.len() <= 12, Refusal::Bounds)?;
    bytes.iter().try_fold(0u64, |n, b| {
        need(b.is_ascii_digit(), Refusal::Decimal)?;
        n.checked_mul(10)
            .and_then(|n| n.checked_add(u64::from(b - b'0')))
            .ok_or(Refusal::Bounds)
    })
}
// Only the independently authored nonnegative fixture domain is supported.
fn display(bytes: &[u8]) -> Result<u64, Refusal> {
    let (&last, prefix) = bytes.split_last().ok_or(Refusal::Decimal)?;
    let final_digit = match last {
        b'{' => 0,
        b'A'..=b'I' => last - b'A' + 1,
        b'0'..=b'9' => last - b'0',
        _ => return Err(Refusal::Decimal),
    };
    digits(prefix)?
        .checked_mul(10)
        .and_then(|n| n.checked_add(u64::from(final_digit)))
        .ok_or(Refusal::Bounds)
}
fn packed(bytes: &[u8]) -> Result<u64, Refusal> {
    need(!bytes.is_empty() && bytes.len() <= 7, Refusal::Bounds)?;
    need(
        matches!(bytes.last().unwrap() & 15, 0x0c | 0x0f),
        Refusal::Decimal,
    )?;
    let mut n = 0u64;
    for (at, byte) in bytes.iter().enumerate() {
        for digit in [byte >> 4, byte & 15]
            .into_iter()
            .take(if at + 1 == bytes.len() { 1 } else { 2 })
        {
            need(digit <= 9, Refusal::Decimal)?;
            n = n
                .checked_mul(10)
                .and_then(|n| n.checked_add(u64::from(digit)))
                .ok_or(Refusal::Bounds)?;
        }
    }
    Ok(n)
}
fn put_packed(out: &mut [u8], n: u64, width: usize) -> Result<(), Refusal> {
    let text = format!("{n:0width$}");
    need(
        width <= 12 && text.len() == width && out.len() == (width + 2) / 2,
        Refusal::Bounds,
    )?;
    let mut ns = Vec::with_capacity(width + 2);
    if width.is_multiple_of(2) {
        ns.push(0);
    }
    ns.extend(text.bytes().map(|b| b - b'0'));
    ns.push(0x0c);
    for (to, pair) in out.iter_mut().zip(ns.as_chunks::<2>().0) {
        *to = pair[0] << 4 | pair[1];
    }
    Ok(())
}
fn add_count(out: &mut [u8]) -> Result<(), Refusal> {
    let value = i16::from_be_bytes((&*out).try_into().map_err(|_| Refusal::Fixture)?);
    need((0..9_999).contains(&value), Refusal::Bounds)?;
    out.copy_from_slice(&(value + 1).to_be_bytes());
    Ok(())
}
fn add_amount(out: &mut [u8], amount: u64) -> Result<(), Refusal> {
    let n = packed(out)?.checked_add(amount).ok_or(Refusal::Bounds)?;
    put_packed(out, n, 11)
}
fn amount(bytes: &[u8]) -> Result<u64, Refusal> {
    let parts = bytes.split(|b| *b == b'.').collect::<Vec<_>>();
    need(parts.len() == 2 && parts[1].len() == 2, Refusal::Fixture)?;
    digits(parts[0])?
        .checked_mul(100)
        .and_then(|v| v.checked_add(digits(parts[1]).ok()?))
        .ok_or(Refusal::Bounds)
}
fn copy_text(out: &mut [u8], value: &[u8]) -> Result<(), Refusal> {
    need(value.len() <= out.len(), Refusal::Fixture)?;
    out.fill(b' ');
    out[..value.len()].copy_from_slice(value);
    Ok(())
}
fn seed<'a>(offer: &'a RequestOffer, name: &str, width: usize) -> Result<&'a SeedImage, Refusal> {
    let rows = offer
        .seeds
        .iter()
        .filter(|s| s.dataset == name)
        .collect::<Vec<_>>();
    need(
        rows.len() == 1 && rows[0].record.len() == width,
        Refusal::Fixture,
    )?;
    Ok(rows[0])
}

/// Source-derived conditional normal-completion image, not a vendor golden.
/// The existing pure01 fixture checks request/reply/source identities separately.
pub(super) fn derive(
    case: Case,
    offer: &RequestOffer,
    before: &Before,
) -> Result<Projection, Refusal> {
    need(
        matches!(
            case,
            Case::Approved | Case::InsufficientCredit | Case::MissingCrossReference
        ) && offer.program == "COPAUA0C"
            && offer.transaction == "CP00",
        Refusal::Fixture,
    )?;
    need(
        offer.body.len() <= 500
            && offer.seeds.len() == 3
            && before.summary_work.len() == 100
            && before.detail_work.len() == 200
            && before.hierarchy.len() <= 16
            && before.yyddd <= 99_999
            && before.hhmmss <= 235_959
            && before.milliseconds <= 999,
        Refusal::Bounds,
    )?;
    need(
        before.hhmmss / 10_000 < 24 && before.hhmmss / 100 % 100 < 60 && before.hhmmss % 100 < 60,
        Refusal::Fixture,
    )?;
    let mut seen = std::collections::BTreeSet::new();
    for root in &before.hierarchy {
        need(
            root.data.len() == 100
                && root.children.len() <= 32
                && root.children.iter().all(|c| c.len() == 200)
                && seen.insert(root.data[..6].to_vec()),
            Refusal::Fixture,
        )?;
    }
    let fields = offer.body.split(|b| *b == b',').collect::<Vec<_>>();
    need(
        fields.len() == 18 && fields[0].len() == 6 && fields[1].len() == 6 && fields[2].len() == 16,
        Refusal::Fixture,
    )?;
    let cents = amount(fields[8])?;
    let acct = seed(offer, "AWS.M2.CARDDEMO.ACCTDAT", 300)?;
    let xref = seed(offer, "AWS.M2.CARDDEMO.CARDXREF", 50)?;
    let cust = seed(offer, "AWS.M2.CARDDEMO.CUSTDAT", 500)?;
    need(
        acct.key == acct.record[..11]
            && xref.key == xref.record[..16]
            && cust.key == cust.record[..9],
        Refusal::Fixture,
    )?;
    let mut result = Projection {
        unchanged_vsam: offer.seeds.clone(),
        conditional_writes: Vec::new(),
        summary_work: before.summary_work.clone(),
        detail_work: before.detail_work.clone(),
        conditional_success_hierarchy: Some(before.hierarchy.clone()),
    };
    if case == Case::MissingCrossReference {
        need(
            fields[2] != xref.key && !before.previous_summary_found,
            Refusal::StaleSummaryBranch,
        )?;
        return Ok(result); // Source skips ALL IMS writes and never rewrites VSAM.
    }
    need(
        fields[2] == xref.key && acct.key == xref.record[25..36] && cust.key == xref.record[16..25],
        Refusal::Fixture,
    )?;
    let mut root_key = vec![0; 6];
    put_packed(&mut root_key, digits(&xref.record[25..36])?, 11)?;
    let position = before
        .hierarchy
        .iter()
        .position(|r| r.data[..6] == root_key);
    let available = if let Some(at) = position {
        let old = &before.hierarchy[at].data;
        packed(&old[26..32])?
            .checked_sub(packed(&old[38..44])?)
            .ok_or(Refusal::Bounds)?
    } else {
        display(&acct.record[24..36])?
            .checked_sub(display(&acct.record[12..24])?)
            .ok_or(Refusal::Bounds)?
    };
    let approved = cents <= available;
    need(approved == (case == Case::Approved), Refusal::Decision)?;
    let summary = &mut result.summary_work;
    if let Some(at) = position {
        *summary = before.hierarchy[at].data.clone();
    } else {
        // INITIALIZE ... REPLACING NUMERIC DATA BY ZERO leaves alpha/filler
        // bytes at the independently admitted complete working-storage preimage.
        for (a, b) in [
            (0, 6),
            (26, 32),
            (32, 38),
            (38, 44),
            (44, 50),
            (54, 60),
            (60, 66),
        ] {
            put_packed(&mut summary[a..b], 0, 11)?;
        }
        summary[6..15].fill(b'0');
        summary[50..54].fill(0);
        summary[..6].copy_from_slice(&root_key);
        summary[6..15].copy_from_slice(&xref.record[16..25]);
    }
    put_packed(&mut summary[26..32], display(&acct.record[24..36])?, 11)?;
    put_packed(&mut summary[32..38], display(&acct.record[36..48])?, 11)?;
    if approved {
        add_count(&mut summary[50..52])?;
        add_amount(&mut summary[54..60], cents)?;
        add_amount(&mut summary[38..44], cents)?;
        put_packed(&mut summary[44..50], 0, 11)?;
    } else {
        add_count(&mut summary[52..54])?;
        // Exact source operand: previous PA-TRANSACTION-AMT, NOT this request.
        // Raw all-zero COMP-3 is invalid here; do not invent a zero postimage.
        add_amount(&mut summary[60..66], packed(&before.detail_work[74..81])?)?;
    }
    result.conditional_writes.push(WriteImage {
        operation: if position.is_some() {
            ImsOperation::Replace
        } else {
            ImsOperation::Insert
        },
        pcb: 1,
        segments: vec!["PAUTSUM0".into()],
        root_key: None,
        data: summary.clone(),
    });
    let detail = &mut result.detail_work;
    put_packed(&mut detail[..3], u64::from(99_999 - before.yyddd), 5)?;
    let stamp = u64::from(before.hhmmss) * 1000 + u64::from(before.milliseconds);
    put_packed(&mut detail[3..8], 999_999_999 - stamp, 9)?;
    for (field, start, end) in [
        (0, 8, 14),
        (1, 14, 20),
        (2, 20, 36),
        (3, 36, 40),
        (4, 40, 44),
        (5, 44, 50),
        (6, 50, 56),
        (9, 88, 92),
        (10, 92, 95),
        (12, 97, 112),
        (13, 112, 134),
        (14, 134, 147),
        (15, 147, 149),
        (16, 149, 158),
        (17, 158, 173),
    ] {
        copy_text(&mut detail[start..end], fields[field])?;
    }
    detail[56..62].copy_from_slice(fields[1]);
    detail[62..64].copy_from_slice(if approved { b"00" } else { b"05" });
    detail[64..68].copy_from_slice(if approved { b"0000" } else { b"4100" });
    need(
        fields[7].len() == 6 && fields[11].len() == 2,
        Refusal::Fixture,
    )?;
    digits(fields[7])?;
    digits(fields[11])?;
    detail[68..74].copy_from_slice(fields[7]);
    detail[95..97].copy_from_slice(fields[11]);
    put_packed(&mut detail[74..81], cents, 12)?;
    put_packed(&mut detail[81..88], if approved { cents } else { 0 }, 12)?;
    detail[173] = if approved { b'P' } else { b'D' };
    detail[174..183].fill(b' ');
    result.conditional_writes.push(WriteImage {
        operation: ImsOperation::Insert,
        pcb: 1,
        segments: vec!["PAUTSUM0".into(), "PAUTDTL1".into()],
        root_key: Some(root_key),
        data: detail.clone(),
    });
    let hierarchy = result.conditional_success_hierarchy.as_mut().unwrap();
    if let Some(at) = position {
        if hierarchy[at].children.iter().any(|c| c[..8] == detail[..8]) {
            result.conditional_success_hierarchy = None; // No guessed post-error UOW state.
            return Ok(result);
        }
        hierarchy[at].data = summary.clone();
        hierarchy[at].children.push(detail.clone());
        hierarchy[at].children.sort_by(|a, b| a[..8].cmp(&b[..8]));
    } else {
        hierarchy.push(ImsLoadRoot {
            data: summary.clone(),
            children: vec![detail.clone()],
        });
        hierarchy.sort_by(|a, b| a.data[..6].cmp(&b.data[..6]));
    }
    Ok(result)
}

impl Projection {
    /// Only the supplied conditional write subsequence is compared. The future
    /// genuine owner must authenticate its extraction from the entire actual
    /// source trace and bind each mutation's real sequence/key/journal/result.
    pub(super) fn compare_write_offers(&self, actual: &[&ImsRequest]) -> Result<(), Refusal> {
        need(
            actual.len() == self.conditional_writes.len(),
            Refusal::Postimage,
        )?;
        for (request, expected) in actual.iter().zip(&self.conditional_writes) {
            let qualifiers = expected
                .root_key
                .as_ref()
                .map(|key| {
                    vec![ImsQualifier {
                        segment: "PAUTSUM0".into(),
                        field: "ACCNTID".into(),
                        value: key.clone(),
                    }]
                })
                .unwrap_or_default();
            need(
                request.operation == expected.operation
                    && request.pcb == expected.pcb
                    && request.segments == expected.segments
                    && request.data == expected.data
                    && request.qualifiers == qualifiers
                    && request.psb.is_none()
                    && request.max_segments == 1
                    && request.checkpoint_id.is_none()
                    && request.system.is_none()
                    && request.q_class.is_none()
                    && request.mutation.is_some(),
                Refusal::Postimage,
            )?;
        }
        Ok(())
    }
    pub(super) fn compare_complete_postimages(
        &self,
        vsam: &[SeedImage],
        ims: &[ImsLoadRoot],
    ) -> Result<(), Refusal> {
        let expected = self
            .conditional_success_hierarchy
            .as_ref()
            .ok_or(Refusal::DuplicateNeedsDisposition)?;
        need(
            vsam == self.unchanged_vsam && ims == expected,
            Refusal::Postimage,
        )
    }
    /// Even a complete projection match is not business duplicate/replay or
    /// atomic cross-resource proof. Real actor/journal/UOW ownership is missing.
    pub(super) fn unproved_observations(&self) -> [&'static str; 2] {
        ["duplicate/idempotent request", "cross-resource commit"]
    }
}

/// Append exactly the source's STRING fragment, preserve pointer and buffer,
/// and include the source's pointer-sized trailing byte in MQPUT1. The caller
/// obtains `fresh_reference` from the unchanged pure01 reference fixture.
pub(super) fn append_reply(
    buffer: &[u8],
    pointer: usize,
    fresh_reference: &[u8],
) -> Result<(Vec<u8>, usize, Vec<u8>), Refusal> {
    need(
        buffer.len() == 200
            && (1..=200).contains(&pointer)
            && fresh_reference.len() >= 2
            && fresh_reference.last() == Some(&b' ')
            && fresh_reference[fresh_reference.len() - 2] == b',',
        Refusal::Fixture,
    )?;
    let fragment = &fresh_reference[..fresh_reference.len() - 1];
    let next = pointer.checked_add(fragment.len()).ok_or(Refusal::Bounds)?;
    need(next <= 200, Refusal::Bounds)?; // No predicted STRING-overflow semantics.
    let mut out = buffer.to_vec();
    out[pointer - 1..next - 1].copy_from_slice(fragment);
    let message = out[..next].to_vec();
    Ok((out, next, message))
}

#[cfg(test)]
#[path = "mq_authorization_postimages_tests.rs"]
mod tests;
