#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixedValue(Vec<u8>);

impl FixedValue {
    #[must_use]
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.0
    }
    #[must_use]
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.0).trim_end().to_string()
    }
    pub fn integer(&self) -> Result<i128, ValueProblem> {
        self.text()
            .trim()
            .parse()
            .map_err(|_| ValueProblem::InvalidInteger)
    }
    pub fn fit(source: &[u8], length: usize, right: bool) -> Self {
        let mut output = vec![b' '; length];
        if right {
            let copy = source.len().min(length);
            output[length - copy..].copy_from_slice(&source[source.len() - copy..]);
        } else {
            let copy = source.len().min(length);
            output[..copy].copy_from_slice(&source[..copy]);
        }
        Self(output)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueProblem {
    InvalidInteger,
}
impl std::fmt::Display for ValueProblem {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "fixed value is not an integer")
    }
}
impl std::error::Error for ValueProblem {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fitting_is_fixed_and_deterministic() {
        assert_eq!(FixedValue::fit(b"ABCDE", 3, false).bytes(), b"ABC");
        assert_eq!(FixedValue::fit(b"1", 3, true).bytes(), b"  1");
    }
}
