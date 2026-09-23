//! Source-pinned BRXA Init COMMAREA layout for the local 3270 bridge.
//!
//! The 6.2/6.1 BRARC topic fixes the three area lengths and offsets. It does
//! not publish `brxa_current_version_no`, so construction requires a reviewed
//! version supplied by the eventual bridge adapter. No guessed version or Bind
//! command code is embedded here.

use mainframe_env_host_api::HostProblem;
use std::num::NonZeroU32;

const HEADER_LEN: usize = 56;
const TRANSACTION_LEN: usize = 180;
const COMMAND_LEN: usize = 48;
const TRANSACTION_AT: usize = HEADER_LEN;
const COMMAND_AT: usize = HEADER_LEN + TRANSACTION_LEN;
const DATA_AT: usize = COMMAND_AT + COMMAND_LEN;
const POINTER_OFFSET_LIMIT: usize = 1 << 20;

const HEADER_EYE: &[u8; 8] = b">BRAREA ";
const TRANSACTION_EYE: &[u8; 8] = b">BRTRANA";
const COMMAND_EYE: &[u8; 8] = b">BRCOMMA";

/// One Init call image. The caller must supply the selected exit program's
/// virtual address of DFHCOMMAREA and its reviewed BRXA version constant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::service) struct BrxaInitFrame {
    bytes: Vec<u8>,
}

/// Source-defined Init fields that the bridge exit may return.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::service) struct BrxaInitReply {
    pub start_code: [u8; 2],
    pub load_ads_descriptor: bool,
    pub facility_like: [u8; 4],
    pub facility_token: [u8; 8],
    pub identifier: [u8; 48],
    pub formatter: [u8; 8],
    pub user_abend_code: [u8; 4],
}

/// Bind follows a validated Init image and retains its BRDATA pointer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::service) struct BrxaBindFrame {
    bytes: Vec<u8>,
}

/// Source-defined Bind fields the bridge exit may return.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::service) struct BrxaBindReply {
    pub start_code: [u8; 2],
    pub load_ads_descriptor: bool,
    pub facility_keep_time: u32,
    pub identifier: [u8; 48],
    pub user_abend_code: [u8; 4],
}

impl BrxaInitFrame {
    /// Build only the documented Init call; the value of `version` is not
    /// inferred from the BRARC topic.
    pub(in crate::service) fn new(
        commarea_address: u32,
        commarea_capacity: usize,
        version: NonZeroU32,
        bridge_transid: [u8; 4],
        target_transid: [u8; 4],
        userid: [u8; 8],
        brdata: &[u8],
    ) -> Result<Self, HostProblem> {
        let total = DATA_AT
            .checked_add(brdata.len())
            .ok_or(HostProblem::ResourceExhausted)?;
        if commarea_address == 0 || total > commarea_capacity || total > POINTER_OFFSET_LIMIT {
            return Err(HostProblem::ResourceExhausted);
        }
        let pointer = |offset: usize| -> Result<[u8; 4], HostProblem> {
            let value = commarea_address
                .checked_add(u32::try_from(offset).map_err(|_| HostProblem::ResourceExhausted)?)
                .ok_or(HostProblem::ResourceExhausted)?;
            if value >> 20 != commarea_address >> 20 {
                return Err(HostProblem::ResourceExhausted);
            }
            Ok(value.to_be_bytes())
        };
        // Validate the highest addressed byte before allocating an image.
        pointer(total - 1)?;
        let mut bytes = vec![0; total];
        bytes[0..8].copy_from_slice(HEADER_EYE);
        put_u32(&mut bytes, 0x08, HEADER_LEN)?;
        bytes[0x0c..0x10].copy_from_slice(&version.get().to_be_bytes());
        bytes[0x10..0x14].copy_from_slice(&pointer(TRANSACTION_AT)?);
        put_u32(&mut bytes, 0x14, TRANSACTION_LEN)?;
        bytes[0x18..0x1c].copy_from_slice(&pointer(COMMAND_AT)?);
        put_u32(&mut bytes, 0x1c, COMMAND_LEN)?;

        let transaction = &mut bytes[TRANSACTION_AT..COMMAND_AT];
        transaction[0..8].copy_from_slice(TRANSACTION_EYE);
        transaction[0x08..0x0c].copy_from_slice(&bridge_transid);
        transaction[0x0c..0x10].copy_from_slice(&target_transid);
        transaction[0x20..0x28].copy_from_slice(&userid);
        transaction[0x30..0x32].copy_from_slice(b"TD");
        transaction[0x32] = b'N';
        for (start, end) in [(0x34, 0x38), (0x4c, 0x7c), (0x7c, 0x84)] {
            transaction[start..end].fill(b' ');
        }
        if !brdata.is_empty() {
            transaction[0x94..0x98].copy_from_slice(&pointer(DATA_AT)?);
            put_u32(transaction, 0x98, brdata.len())?;
            bytes[DATA_AT..].copy_from_slice(brdata);
        }

        let command = &mut bytes[COMMAND_AT..DATA_AT];
        command[0..8].copy_from_slice(COMMAND_EYE);
        command[0x08..0x0a].copy_from_slice(b"XM");
        command[0x0a..0x0c].copy_from_slice(b"IN");
        command[0x0c..0x10].fill(b' ');
        Ok(Self { bytes })
    }

    pub(in crate::service) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Reject changes outside the BRARC Init writable-field list. In
    /// particular, an exit cannot replace CICS-owned area pointers or BRDATA.
    pub(in crate::service) fn validate_reply(
        &self,
        returned: &[u8],
    ) -> Result<BrxaInitReply, HostProblem> {
        if returned.len() != self.bytes.len()
            || self
                .bytes
                .iter()
                .zip(returned)
                .enumerate()
                .any(|(index, (expected, actual))| expected != actual && !init_writable(index))
        {
            return Err(HostProblem::Malformed);
        }
        let transaction = &returned[TRANSACTION_AT..COMMAND_AT];
        let command = &returned[COMMAND_AT..DATA_AT];
        Ok(BrxaInitReply {
            start_code: normalized_start_code(transaction),
            load_ads_descriptor: transaction[0x32] == b'Y',
            facility_like: transaction[0x34..0x38]
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            facility_token: transaction[0x3c..0x44]
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            identifier: transaction[0x4c..0x7c]
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            formatter: transaction[0x7c..0x84]
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            user_abend_code: command[0x0c..0x10]
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        })
    }

    /// Preserve a validated Init result when advancing to Bind. The committed
    /// BRARC topic does not supply the Bind command code, so the caller must
    /// provide one from separately reviewed authority.
    pub(in crate::service) fn bind(
        &self,
        init_reply: &[u8],
        bind_code: [u8; 2],
    ) -> Result<BrxaBindFrame, HostProblem> {
        self.validate_reply(init_reply)?;
        if bind_code.contains(&0)
            || bind_code == *b"IN"
            || bind_code == *b"TM"
            || bind_code == *b"AB"
        {
            return Err(HostProblem::Malformed);
        }
        let mut bytes = init_reply.to_vec();
        bytes[COMMAND_AT + 0x0a..COMMAND_AT + 0x0c].copy_from_slice(&bind_code);
        Ok(BrxaBindFrame { bytes })
    }
}

impl BrxaBindFrame {
    pub(in crate::service) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Accept only the fields BRARC identifies as writable on Bind. Values
    /// above one week are capped to the source-defined facility keep limit.
    pub(in crate::service) fn validate_reply(
        &self,
        returned: &[u8],
    ) -> Result<BrxaBindReply, HostProblem> {
        if returned.len() != self.bytes.len()
            || self
                .bytes
                .iter()
                .zip(returned)
                .enumerate()
                .any(|(index, (expected, actual))| expected != actual && !bind_writable(index))
        {
            return Err(HostProblem::Malformed);
        }
        let transaction = &returned[TRANSACTION_AT..COMMAND_AT];
        let command = &returned[COMMAND_AT..DATA_AT];
        let start_code = normalized_start_code(transaction);
        Ok(BrxaBindReply {
            start_code,
            load_ads_descriptor: transaction[0x32] == b'Y',
            facility_keep_time: u32::from_be_bytes(
                transaction[0x38..0x3c]
                    .try_into()
                    .map_err(|_| HostProblem::InfrastructureFailure)?,
            )
            .min(604_800),
            identifier: transaction[0x4c..0x7c]
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            user_abend_code: command[0x0c..0x10]
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        })
    }
}

fn init_writable(index: usize) -> bool {
    let transaction = index.checked_sub(TRANSACTION_AT);
    if transaction.is_some_and(|offset| {
        matches!(
            offset,
            0x30..=0x32 | 0x34..=0x37 | 0x3c..=0x43 | 0x4c..=0x83
        )
    }) {
        return true;
    }
    index
        .checked_sub(COMMAND_AT)
        .is_some_and(|offset| (0x0c..0x10).contains(&offset))
}

fn bind_writable(index: usize) -> bool {
    if index
        .checked_sub(TRANSACTION_AT)
        .is_some_and(|offset| matches!(offset, 0x30..=0x32 | 0x38..=0x3b | 0x4c..=0x7b))
    {
        return true;
    }
    index
        .checked_sub(COMMAND_AT)
        .is_some_and(|offset| (0x0c..0x10).contains(&offset))
}

fn normalized_start_code(transaction: &[u8]) -> [u8; 2] {
    match &transaction[0x30..0x32] {
        b"S " => *b"S ",
        b"SD" => *b"SD",
        _ => *b"TD",
    }
}

fn put_u32(bytes: &mut [u8], offset: usize, value: usize) -> Result<(), HostProblem> {
    let value = u32::try_from(value).map_err(|_| HostProblem::ResourceExhausted)?;
    bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> BrxaInitFrame {
        BrxaInitFrame::new(
            0x0030_0000,
            4096,
            NonZeroU32::new(7).unwrap(), // Test input, not an IBM version claim.
            *b"MONI",
            *b"NX00",
            *b"USER    ",
            b"ABCD",
        )
        .unwrap()
    }

    #[test]
    fn init_image_uses_pinned_offsets_and_virtual_pointers() {
        let frame = frame();
        let bytes = frame.bytes();
        assert_eq!(bytes.len(), 56 + 180 + 48 + 4);
        assert_eq!(&bytes[0..8], HEADER_EYE);
        assert_eq!(&bytes[0x08..0x0c], &56u32.to_be_bytes());
        assert_eq!(&bytes[0x0c..0x10], &7u32.to_be_bytes());
        assert_eq!(&bytes[0x10..0x14], &0x0030_0038u32.to_be_bytes());
        assert_eq!(&bytes[0x14..0x18], &180u32.to_be_bytes());
        assert_eq!(&bytes[0x18..0x1c], &0x0030_00ecu32.to_be_bytes());
        assert_eq!(&bytes[0x1c..0x20], &48u32.to_be_bytes());
        let transaction = &bytes[TRANSACTION_AT..COMMAND_AT];
        assert_eq!(&transaction[0..8], TRANSACTION_EYE);
        assert_eq!(&transaction[0x08..0x10], b"MONINX00");
        assert_eq!(&transaction[0x20..0x28], b"USER    ");
        assert_eq!(&transaction[0x30..0x32], b"TD");
        assert_eq!(&transaction[0x94..0x98], &0x0030_011cu32.to_be_bytes());
        assert_eq!(&transaction[0x98..0x9c], &4u32.to_be_bytes());
        let command = &bytes[COMMAND_AT..DATA_AT];
        assert_eq!(&command[0..8], COMMAND_EYE);
        assert_eq!(&command[0x08..0x0c], b"XMIN");
        assert_eq!(&bytes[DATA_AT..], b"ABCD");
    }

    #[test]
    fn init_reply_accepts_only_documented_writable_fields() {
        let frame = frame();
        let mut returned = frame.bytes().to_vec();
        returned[TRANSACTION_AT + 0x30..TRANSACTION_AT + 0x32].copy_from_slice(b"SD");
        returned[TRANSACTION_AT + 0x34..TRANSACTION_AT + 0x38].copy_from_slice(b"TERM");
        returned[COMMAND_AT + 0x0c..COMMAND_AT + 0x10].copy_from_slice(b"ABCD");
        let reply = frame.validate_reply(&returned).unwrap();
        assert_eq!(reply.start_code, *b"SD");
        assert_eq!(reply.facility_like, *b"TERM");
        assert_eq!(reply.user_abend_code, *b"ABCD");
        returned[TRANSACTION_AT + 0x30..TRANSACTION_AT + 0x32].copy_from_slice(b"??");
        assert_eq!(frame.validate_reply(&returned).unwrap().start_code, *b"TD");
        returned[0x10] ^= 1;
        assert_eq!(frame.validate_reply(&returned), Err(HostProblem::Malformed));
        returned[0x10] ^= 1;
        returned[DATA_AT] ^= 1;
        assert_eq!(frame.validate_reply(&returned), Err(HostProblem::Malformed));
    }

    #[test]
    fn init_without_brdata_keeps_the_source_pointer_null() {
        let frame = BrxaInitFrame::new(
            0x0030_0000,
            DATA_AT,
            NonZeroU32::new(7).unwrap(),
            *b"MONI",
            *b"NX00",
            *b"USER    ",
            b"",
        )
        .unwrap();
        assert_eq!(frame.bytes().len(), DATA_AT);
        let transaction = &frame.bytes()[TRANSACTION_AT..COMMAND_AT];
        assert_eq!(&transaction[0x94..0x9c], &[0; 8]);
    }

    #[test]
    fn bind_preserves_init_state_and_limits_exit_mutation() {
        let init = frame();
        let mut init_returned = init.bytes().to_vec();
        init_returned[TRANSACTION_AT + 0x30..TRANSACTION_AT + 0x32].copy_from_slice(b"SD");
        init_returned[TRANSACTION_AT + 0x4c..TRANSACTION_AT + 0x50].copy_from_slice(b"FLOW");
        let bind = init.bind(&init_returned, *b"XY").unwrap();
        assert_eq!(&bind.bytes()[COMMAND_AT + 0x08..COMMAND_AT + 0x0c], b"XMXY");
        assert_eq!(&bind.bytes()[DATA_AT..], b"ABCD");
        let mut returned = bind.bytes().to_vec();
        returned[TRANSACTION_AT + 0x38..TRANSACTION_AT + 0x3c]
            .copy_from_slice(&700_000u32.to_be_bytes());
        let reply = bind.validate_reply(&returned).unwrap();
        assert_eq!(reply.start_code, *b"SD");
        assert_eq!(&reply.identifier[..4], b"FLOW");
        assert_eq!(reply.facility_keep_time, 604_800);
        returned[TRANSACTION_AT + 0x34] ^= 1;
        assert_eq!(bind.validate_reply(&returned), Err(HostProblem::Malformed));
        assert_eq!(
            init.bind(&init_returned, *b"IN"),
            Err(HostProblem::Malformed)
        );
        assert_eq!(
            init.bind(&init_returned, [0, 0]),
            Err(HostProblem::Malformed)
        );
        init_returned[0x10] ^= 1;
        assert_eq!(
            init.bind(&init_returned, *b"XY"),
            Err(HostProblem::Malformed)
        );
    }

    #[test]
    fn init_image_rejects_out_of_address_or_commarea_bounds() {
        assert_eq!(
            BrxaInitFrame::new(
                0x003f_ffff,
                4096,
                NonZeroU32::new(7).unwrap(),
                *b"MONI",
                *b"NX00",
                *b"USER    ",
                b"ABCD",
            ),
            Err(HostProblem::ResourceExhausted)
        );
        assert_eq!(
            BrxaInitFrame::new(
                0x0030_0000,
                283,
                NonZeroU32::new(7).unwrap(),
                *b"MONI",
                *b"NX00",
                *b"USER    ",
                b"ABCD",
            ),
            Err(HostProblem::ResourceExhausted)
        );
    }
}
