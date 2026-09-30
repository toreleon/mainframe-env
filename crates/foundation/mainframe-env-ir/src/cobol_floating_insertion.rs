/// Expanded leading floating string, including embedded and immediately following
/// simple-insertion symbols. A period belongs only when another floating symbol follows it.
#[must_use]
pub fn cobol_floating_insertion_prefix(symbols: &[u8]) -> Option<(u8, usize)> {
    let symbol = *symbols.first()?;
    if !matches!(symbol, b'$' | b'+' | b'-') {
        return None;
    }
    let mut count = 0;
    let mut last_symbol = 0;
    let mut scanned = 0;
    for (index, &byte) in symbols.iter().enumerate() {
        if byte == symbol {
            count += 1;
            last_symbol = index + 1;
        } else if !matches!(byte, b'B' | b'0' | b'/' | b',' | b'.') {
            break;
        }
        scanned = index + 1;
    }
    if count < 2 {
        return None;
    }
    let end = symbols[last_symbol..scanned]
        .iter()
        .take_while(|&&byte| matches!(byte, b'B' | b'0' | b'/' | b','))
        .count()
        + last_symbol;
    Some((symbol, end))
}
