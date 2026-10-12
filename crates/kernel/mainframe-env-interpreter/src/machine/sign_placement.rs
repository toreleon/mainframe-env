use super::{Attribute, MachineProblem, Operation};

pub(super) fn sign_leading(operation: &Operation) -> Result<bool, MachineProblem> {
    // Older executable layouts default to the COBOL trailing sign.
    match operation.attributes.get("sign_leading") {
        None | Some(Attribute::Integer(0)) => Ok(false),
        Some(Attribute::Integer(1)) => Ok(true),
        _ => Err(MachineProblem::InvalidArtifact(
            "invalid sign_leading".into(),
        )),
    }
}

pub(super) fn encode_sign(
    output: &mut Vec<u8>,
    signed: bool,
    leading: bool,
    separate: bool,
    negative: bool,
) -> Result<(), MachineProblem> {
    if signed && !separate {
        let sign_digit = if leading {
            output.first_mut()
        } else {
            output.last_mut()
        }
        .ok_or(MachineProblem::SizeError)?;
        let overpunch = if negative {
            b"}JKLMNOPQR"
        } else {
            b"{ABCDEFGHI"
        };
        *sign_digit = overpunch[usize::from(*sign_digit - b'0')];
    }
    if separate {
        let sign = if negative { b'-' } else { b'+' };
        if leading {
            output.insert(0, sign);
        } else {
            output.push(sign);
        }
    }
    Ok(())
}
