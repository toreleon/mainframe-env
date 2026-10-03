//! Bounded ASCII identity copying, independent of code-page selection.

use std::fmt;

/// A checked ASCII identity copy failed before returning any output bytes.
///
/// Input validity takes precedence over the caller's output bound; allocation
/// is attempted only after both checks succeed. These failures do not describe
/// CCSID conversion or numeric encoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AsciiEncodingProblem {
    /// At least one input character is outside ASCII's U+0000 through U+007F.
    /// No replacement or transliteration is attempted.
    NonAscii,
    /// The exact ASCII byte length exceeds the caller's inclusive output bound.
    /// No output capacity has been reserved and no prefix is returned.
    OutputLimit,
    /// Fallible output reservation failed, including capacity overflow or an
    /// allocation failure reported by the allocator. No partial output is returned.
    Allocation,
}

impl fmt::Display for AsciiEncodingProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NonAscii => "input contains a non-ASCII character",
            Self::OutputLimit => "ASCII output exceeds the caller's byte limit",
            Self::Allocation => "ASCII output capacity could not be reserved",
        })
    }
}

impl std::error::Error for AsciiEncodingProblem {}

/// Copies checked ASCII input into independently owned bytes without changes.
///
/// Every ASCII value, including NUL, other controls and DEL, is preserved. This
/// performs no code-page mapping, trimming, normalization or substitution.
/// `max_output_bytes` is an inclusive byte bound, not a requested allocation size;
/// empty input therefore succeeds even with a zero bound.
///
/// # Errors
///
/// Returns [`AsciiEncodingProblem::NonAscii`] first if any input character is
/// outside ASCII, even when the input also exceeds the bound. Otherwise returns
/// [`AsciiEncodingProblem::OutputLimit`] before allocating if `input.len()` exceeds
/// the bound. Only valid, bounded input reaches `try_reserve_exact`; a failed
/// reservation returns [`AsciiEncodingProblem::Allocation`]. Bytes are copied
/// only after reservation succeeds, so errors never return truncated output.
///
/// ```
/// use mainframe_env_encoding::{AsciiEncodingProblem, encode_ascii};
/// assert_eq!(encode_ascii("A\0\x7f", 3).unwrap(), vec![b'A', 0, 127]);
/// assert_eq!(encode_ascii("", 0).unwrap(), Vec::<u8>::new());
/// assert_eq!(encode_ascii("AB", 1), Err(AsciiEncodingProblem::OutputLimit));
/// assert_eq!(encode_ascii("é", 0), Err(AsciiEncodingProblem::NonAscii));
/// ```
pub fn encode_ascii(input: &str, max_output_bytes: usize) -> Result<Vec<u8>, AsciiEncodingProblem> {
    encode_with_reservation(input, max_output_bytes, reserve_output)
}

fn encode_with_reservation(
    input: &str,
    max_output_bytes: usize,
    reserve: fn(&mut Vec<u8>, usize) -> Result<(), AsciiEncodingProblem>,
) -> Result<Vec<u8>, AsciiEncodingProblem> {
    if !input.is_ascii() {
        return Err(AsciiEncodingProblem::NonAscii);
    }
    if input.len() > max_output_bytes {
        return Err(AsciiEncodingProblem::OutputLimit);
    }
    let mut output = Vec::new();
    reserve(&mut output, input.len())?;
    output.extend_from_slice(input.as_bytes());
    Ok(output)
}

fn reserve_output(output: &mut Vec<u8>, additional: usize) -> Result<(), AsciiEncodingProblem> {
    output
        .try_reserve_exact(additional)
        .map_err(|_| AsciiEncodingProblem::Allocation)
}

#[cfg(test)]
mod tests;
