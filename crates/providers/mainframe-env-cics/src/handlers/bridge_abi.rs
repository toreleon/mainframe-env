//! Source-pinned BRXA COMMAREA layouts for the local 3270 bridge.
//!
//! The 6.2/6.1 BRARC topic fixes the three area lengths and offsets. It does
//! not publish `brxa_current_version_no`, so construction requires a reviewed
//! version supplied by the deployment profile. No guessed version or Bind
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
pub struct BrxaInitFrame {
    bytes: Vec<u8>,
}

/// Source-defined Init fields that the bridge exit may return.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrxaInitReply {
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
pub struct BrxaBindFrame {
    bytes: Vec<u8>,
}

/// Source-defined Bind fields the bridge exit may return.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrxaBindReply {
    pub start_code: [u8; 2],
    pub load_ads_descriptor: bool,
    pub facility_keep_time: u32,
    pub identifier: [u8; 48],
    pub user_abend_code: [u8; 4],
}

/// Source-defined Term or Abend notification after the target finishes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrxaEndFrame {
    bytes: Vec<u8>,
}

/// One bounded BMS callback image. All pointers address the selected exit's
/// DFHCOMMAREA; external GETMAIN pointers are intentionally unsupported.
pub struct BrxaBmsFrame {
    bytes: Vec<u8>,
    output_at: Option<usize>,
}

impl BrxaBmsFrame {
    pub fn new(
        bound: &[u8],
        commarea_address: u32,
        capacity: usize,
        eibfn: [u8; 2],
        mapset: Option<&str>,
        map: Option<&str>,
        from: &[u8],
        receive: bool,
    ) -> Result<Self, HostProblem> {
        if bound.len() < DATA_AT
            || capacity > 32_767
            || bound.len() > capacity
            || &bound[..8] != HEADER_EYE
            || &bound[TRANSACTION_AT..TRANSACTION_AT + 8] != TRANSACTION_EYE
            || &bound[COMMAND_AT..COMMAND_AT + 8] != COMMAND_EYE
        {
            return Err(HostProblem::Malformed);
        }
        let command_at = bound.len();
        let from_at = command_at
            .checked_add(108)
            .ok_or(HostProblem::ResourceExhausted)?;
        let output_at = from_at
            .checked_add(from.len())
            .ok_or(HostProblem::ResourceExhausted)?;
        let output_len = if receive {
            capacity.saturating_sub(output_at).min(4096)
        } else {
            0
        };
        if output_at > capacity || receive && output_len == 0 {
            return Err(HostProblem::ResourceExhausted);
        }
        let total = output_at + output_len;
        let pointer = |offset: usize| -> Result<[u8; 4], HostProblem> {
            if offset >= 1 << 20 {
                return Err(HostProblem::ResourceExhausted);
            }
            let address = commarea_address
                .checked_add(u32::try_from(offset).map_err(|_| HostProblem::ResourceExhausted)?)
                .ok_or(HostProblem::ResourceExhausted)?;
            if address >> 20 != commarea_address >> 20 {
                return Err(HostProblem::ResourceExhausted);
            }
            Ok(address.to_be_bytes())
        };
        pointer(total - 1)?;
        let mut bytes = vec![0; total];
        bytes[..bound.len()].copy_from_slice(bound);
        bytes[0x18..0x1c].copy_from_slice(&pointer(command_at)?);
        bytes[0x1c..0x20].copy_from_slice(&108u32.to_be_bytes());
        let command = &mut bytes[command_at..from_at];
        command[..8].copy_from_slice(COMMAND_EYE);
        let code = format!("{:02X}{:02X}", eibfn[0], eibfn[1]);
        command[8..12].copy_from_slice(code.as_bytes());
        command[12..16].fill(b' ');
        for (value, start) in [(mapset, 0x30), (map, 0x38)] {
            command[start..start + 7].fill(b' ');
            if let Some(value) = value {
                if value.is_empty()
                    || value.len() > 7
                    || !value
                        .bytes()
                        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
                {
                    return Err(HostProblem::Malformed);
                }
                command[start..start + value.len()].copy_from_slice(value.as_bytes());
            }
        }
        command[0x44..0x46].copy_from_slice(&(-2i16).to_be_bytes());
        command[0x4a] = b'N';
        for index in [0x4b, 0x4c, 0x4d, 0x4e, 0x4f] {
            command[index] = b'N';
        }
        command[0x26] = 0x7d;
        if !from.is_empty() {
            command[0x10..0x14].copy_from_slice(&pointer(from_at)?);
            command[0x14..0x18].copy_from_slice(
                &u32::try_from(from.len())
                    .map_err(|_| HostProblem::ResourceExhausted)?
                    .to_be_bytes(),
            );
            bytes[from_at..output_at].copy_from_slice(from);
        }
        Ok(Self {
            bytes,
            output_at: receive.then_some(output_at),
        })
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn validate_reply(
        &self,
        returned: &[u8],
        commarea_address: u32,
    ) -> Result<Vec<u8>, HostProblem> {
        let command_at = u32::from_be_bytes(
            self.bytes[0x18..0x1c]
                .try_into()
                .map_err(|_| HostProblem::Malformed)?,
        )
        .checked_sub(commarea_address)
        .ok_or(HostProblem::Malformed)? as usize;
        if returned.len() != self.bytes.len()
            || self
                .bytes
                .iter()
                .zip(returned)
                .enumerate()
                .any(|(index, (expected, actual))| {
                    expected != actual
                        && !self.output_at.is_some_and(|at| index >= at)
                        && !(command_at + 0x18..command_at + 0x20).contains(&index)
                        && !(command_at + 0x24..command_at + 0x26).contains(&index)
                        && index != command_at + 0x26
                        && !(command_at + 0x0c..command_at + 0x10).contains(&index)
                })
            || returned[command_at + 0x0c..command_at + 0x10] != *b"    "
        {
            return Err(HostProblem::Malformed);
        }
        let command = &returned[command_at..command_at + 108];
        if command[0x20..0x24] != [0; 4] {
            return Err(HostProblem::ProviderFailure);
        }
        let Some(output_at) = self.output_at else {
            return Ok(Vec::new());
        };
        let pointer = u32::from_be_bytes(
            command[0x18..0x1c]
                .try_into()
                .map_err(|_| HostProblem::Malformed)?,
        )
        .checked_sub(commarea_address)
        .ok_or(HostProblem::Malformed)? as usize;
        let length = u32::from_be_bytes(
            command[0x1c..0x20]
                .try_into()
                .map_err(|_| HostProblem::Malformed)?,
        ) as usize;
        let end = pointer.checked_add(length).ok_or(HostProblem::Malformed)?;
        if pointer < output_at || end > returned.len() {
            return Err(HostProblem::Malformed);
        }
        Ok(returned[pointer..end].to_vec())
    }
}

impl BrxaEndFrame {
    pub fn new(bound: &[u8], abend: bool) -> Result<Self, HostProblem> {
        if bound.len() < DATA_AT
            || &bound[..8] != HEADER_EYE
            || &bound[TRANSACTION_AT..TRANSACTION_AT + 8] != TRANSACTION_EYE
            || &bound[COMMAND_AT..COMMAND_AT + 8] != COMMAND_EYE
        {
            return Err(HostProblem::Malformed);
        }
        let mut bytes = bound.to_vec();
        bytes[COMMAND_AT + 0x08..COMMAND_AT + 0x0a].copy_from_slice(b"XM");
        bytes[COMMAND_AT + 0x0a..COMMAND_AT + 0x0c].copy_from_slice(if abend {
            b"AB"
        } else {
            b"TM"
        });
        bytes[COMMAND_AT + 0x0c..COMMAND_AT + 0x10].fill(b' ');
        Ok(Self { bytes })
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn validate_reply(&self, returned: &[u8]) -> Result<(), HostProblem> {
        if returned.len() != self.bytes.len()
            || self
                .bytes
                .iter()
                .zip(returned)
                .enumerate()
                .any(|(index, (expected, actual))| {
                    expected != actual
                        && index != TRANSACTION_AT + 0x38
                        && !(TRANSACTION_AT + 0x39..TRANSACTION_AT + 0x3c).contains(&index)
                        && !(COMMAND_AT + 0x0c..COMMAND_AT + 0x10).contains(&index)
                })
            || returned[COMMAND_AT + 0x0c..COMMAND_AT + 0x10] != *b"    "
        {
            Err(HostProblem::Malformed)
        } else {
            Ok(())
        }
    }
}

impl BrxaInitFrame {
    /// Build only the documented Init call; the value of `version` is not
    /// inferred from the BRARC topic.
    pub fn new(
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

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Reject changes outside the BRARC Init writable-field list. In
    /// particular, an exit cannot replace CICS-owned area pointers or BRDATA.
    pub fn validate_reply(&self, returned: &[u8]) -> Result<BrxaInitReply, HostProblem> {
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
    pub fn bind(
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
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Accept only the fields BRARC identifies as writable on Bind. Values
    /// above one week are capped to the source-defined facility keep limit.
    pub fn validate_reply(&self, returned: &[u8]) -> Result<BrxaBindReply, HostProblem> {
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

    #[test]
    fn bms_callback_accepts_only_its_checked_output_area() {
        let init = frame();
        let bind = init.bind(init.bytes(), *b"XY").unwrap();
        let base = 0x0030_0000u32;
        let send = BrxaBmsFrame::new(
            bind.bytes(),
            base,
            4096,
            [0x04, 0x02],
            None,
            None,
            b"HELLO",
            false,
        )
        .unwrap();
        let command_at = bind.bytes().len();
        assert_eq!(
            &send.bytes()[0x18..0x1c],
            &(base + command_at as u32).to_be_bytes()
        );
        assert_eq!(&send.bytes()[command_at + 8..command_at + 12], b"0402");
        assert_eq!(send.validate_reply(send.bytes(), base), Ok(Vec::new()));
        let mut changed = send.bytes().to_vec();
        changed[0x10] ^= 1;
        assert_eq!(
            send.validate_reply(&changed, base),
            Err(HostProblem::Malformed)
        );

        let receive = BrxaBmsFrame::new(
            bind.bytes(),
            base,
            4096,
            [0x04, 0x04],
            Some("MAPSET"),
            Some("MAP"),
            b"",
            true,
        )
        .unwrap();
        let output_at = command_at + 108;
        let mut returned = receive.bytes().to_vec();
        returned[output_at..output_at + 3].copy_from_slice(b"ABC");
        returned[command_at + 0x18..command_at + 0x1c]
            .copy_from_slice(&(base + output_at as u32).to_be_bytes());
        returned[command_at + 0x1c..command_at + 0x20].copy_from_slice(&3u32.to_be_bytes());
        assert_eq!(receive.validate_reply(&returned, base), Ok(b"ABC".to_vec()));
        returned[command_at + 0x18..command_at + 0x1c]
            .copy_from_slice(&(base + DATA_AT as u32).to_be_bytes());
        assert_eq!(
            receive.validate_reply(&returned, base),
            Err(HostProblem::Malformed)
        );
    }

    #[test]
    fn term_and_abend_reject_unreviewed_exit_mutation() {
        let init = frame();
        let bind = init.bind(init.bytes(), *b"XY").unwrap();
        for (abend, command) in [(false, b"TM"), (true, b"AB")] {
            let end = BrxaEndFrame::new(bind.bytes(), abend).unwrap();
            assert_eq!(&end.bytes()[COMMAND_AT + 0x0a..COMMAND_AT + 0x0c], command);
            assert_eq!(end.validate_reply(end.bytes()), Ok(()));
            let mut changed = end.bytes().to_vec();
            changed[TRANSACTION_AT + 0x34] ^= 1;
            assert_eq!(end.validate_reply(&changed), Err(HostProblem::Malformed));
        }
    }
}
