//! Admission of complete fixed-point literals from active, expanded source.

use crate::syntax::{CobolSyntaxKind, LosslessSyntax, next_token};

impl LosslessSyntax {
    pub(crate) fn oversized_numeric_literal(&self, max_digits: usize) -> Option<&str> {
        // COPY/REPLACE and conditional compilation have already selected the
        // semantic spelling. The lossless source can still contain inactive values.
        let source = self.semantic_text();
        let separator = decimal_separator(source);
        let mut remaining = source;
        while !remaining.is_empty() {
            let (kind, mut length) = next_token(remaining);
            if matches!(kind, CobolSyntaxKind::Number | CobolSyntaxKind::Punctuation)
                && let Some(literal_length) = numeric_literal_length(remaining, separator)
            {
                length = literal_length;
                let literal = &remaining[..length];
                if literal.bytes().filter(u8::is_ascii_digit).count() > max_digits {
                    return Some(literal);
                }
            }
            remaining = &remaining[length..];
        }
        None
    }
}

fn numeric_literal_length(source: &str, separator: u8) -> Option<usize> {
    let bytes = source.as_bytes();
    let sign = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let integer_digits = bytes[sign..]
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    let mut length = sign + integer_digits;
    let mut fractional_digits = 0;
    // A trailing period is a sentence delimiter, not part of the literal.
    if bytes.get(length) == Some(&separator)
        && bytes.get(length + 1).is_some_and(u8::is_ascii_digit)
    {
        fractional_digits = bytes[length + 1..]
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        length += 1 + fractional_digits;
    }
    (integer_digits + fractional_digits > 0).then_some(length)
}

fn decimal_separator(source: &str) -> u8 {
    let mut remaining = source;
    let mut previous = "";
    let mut environment = false;
    let mut configuration = false;
    let mut special_names = false;
    let mut clause_word = 0;
    while !remaining.is_empty() {
        let (kind, length) = next_token(remaining);
        let text = &remaining[..length];
        remaining = &remaining[length..];
        if matches!(
            kind,
            CobolSyntaxKind::Whitespace | CobolSyntaxKind::Newline | CobolSyntaxKind::Comment
        ) {
            continue;
        }
        if kind == CobolSyntaxKind::Word {
            if text.eq_ignore_ascii_case("DIVISION") {
                environment = previous.eq_ignore_ascii_case("ENVIRONMENT");
                configuration = false;
                special_names = false;
            } else if text.eq_ignore_ascii_case("SECTION") {
                configuration = environment && previous.eq_ignore_ascii_case("CONFIGURATION");
                special_names = false;
            } else if configuration && text.eq_ignore_ascii_case("SPECIAL-NAMES") {
                special_names = true;
            }
            if special_names {
                clause_word = match (clause_word, text) {
                    (_, word) if word.eq_ignore_ascii_case("DECIMAL-POINT") => 1,
                    (1, word) if word.eq_ignore_ascii_case("IS") => 2,
                    (2, word) if word.eq_ignore_ascii_case("COMMA") => return b',',
                    _ => 0,
                };
            }
        } else {
            clause_word = 0;
        }
        previous = text;
    }
    b'.'
}

#[cfg(test)]
mod tests {
    use crate::syntax::{LosslessSyntax, SyntaxLimits, decode_and_lex};
    use mainframe_env_source::{
        LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat, SourceLimits,
    };
    use std::collections::BTreeMap;

    fn syntax(source: &str) -> LosslessSyntax {
        let limits = SourceLimits::default();
        let path = LogicalPath::new("literal.cbl", limits.max_path_bytes).unwrap();
        let file = SourceFile::input(
            path.as_str(),
            source.as_bytes().to_vec(),
            SourceFormat::Free,
            SourceEncoding::Utf8,
            limits,
        )
        .unwrap();
        let bundle =
            SourceBundle::new(&path, vec![file], BTreeMap::new(), Vec::new(), limits).unwrap();
        decode_and_lex(&bundle, SyntaxLimits::default()).unwrap()
    }

    const COMMA: &str =
        "ENVIRONMENT DIVISION. CONFIGURATION SECTION. SPECIAL-NAMES. DECIMAL-POINT IS COMMA.";

    #[test]
    fn compat_counts_complete_period_literal_digits() {
        let syntax = syntax("COMPUTE A = 1234567890.123456789 - 1234567890.123456789.");
        assert_eq!(
            syntax.oversized_numeric_literal(18),
            Some("1234567890.123456789")
        );
    }

    #[test]
    fn compat_counts_complete_comma_literal_digits() {
        let syntax = syntax(&format!(
            "{COMMA} DATA DIVISION. PROCEDURE DIVISION. COMPUTE A = 1234567890,123456789 - 1234567890,123456789."
        ));
        assert_eq!(
            syntax.oversized_numeric_literal(18),
            Some("1234567890,123456789")
        );
    }

    #[test]
    fn complete_literals_at_the_profile_digit_limit_are_accepted() {
        for (configuration, separator) in [("", '.'), (COMMA, ',')] {
            for limit in [18, 31] {
                let fractional = "9".repeat(limit - 10);
                let syntax = syntax(&format!(
                    "{configuration} DATA DIVISION. PROCEDURE DIVISION. COMPUTE A = 1234567890{separator}{fractional}."
                ));
                assert_eq!(syntax.oversized_numeric_literal(limit), None);
            }
        }
    }

    #[test]
    fn signs_leading_zeros_and_fractional_only_literals_count_spelled_digits() {
        for literal in [
            "+1234567890.123456789",
            "-0000000000.000000001",
            ".1234567890123456789",
        ] {
            let syntax = syntax(&format!("COMPUTE A = {literal}."));
            assert_eq!(syntax.oversized_numeric_literal(18), Some(literal));
        }
        let literal = format!("1234567890.{}", "9".repeat(22));
        let syntax = syntax(&format!("COMPUTE A = {literal}."));
        assert_eq!(syntax.oversized_numeric_literal(31), Some(literal.as_str()));
    }

    #[test]
    fn separator_selection_uses_configuration_words_and_ignores_trivia() {
        let selected = syntax(
            "environment division. configuration section. special-names. decimal-point\n*> separator clause\nis comma. DATA DIVISION. COMPUTE A = 1234567890,123456789.",
        );
        assert_eq!(
            selected.oversized_numeric_literal(18),
            Some("1234567890,123456789")
        );
        for source in [
            "DISPLAY 'SPECIAL-NAMES. DECIMAL-POINT IS COMMA.' 1234567890.123456789.",
            "PROCEDURE DIVISION. SPECIAL-NAMES. DECIMAL-POINT IS COMMA. DISPLAY 1234567890.123456789.",
            "ENVIRONMENT DIVISION. CONFIGURATION SECTION. SPECIAL-NAMES. DISPLAY 'DECIMAL-POINT IS COMMA'. DATA DIVISION. DISPLAY 1234567890.123456789.",
        ] {
            assert_eq!(
                syntax(source).oversized_numeric_literal(18),
                Some("1234567890.123456789")
            );
        }
    }

    #[test]
    fn replacement_expansion_is_checked_instead_of_pseudotext_spelling() {
        let expanded =
            syntax("REPLACE ==SMALL== BY ==1234567890.123456789==.\nCOMPUTE A = SMALL.\n");
        assert_eq!(
            expanded.oversized_numeric_literal(18),
            Some("1234567890.123456789")
        );
        let replaced = syntax(
            "REPLACE ==1234567890.123456789== BY ==1==.\nCOMPUTE A = 1234567890.123456789.\n",
        );
        assert_eq!(replaced.oversized_numeric_literal(18), None);
    }

    #[test]
    fn separators_and_trivia_do_not_join_distinct_numbers() {
        for source in [
            "DISPLAY 1234567890,123456789.",
            "DISPLAY 1234567890. 123456789.",
            "DISPLAY 1234567890.\n123456789.",
            "DISPLAY 1234567890 .123456789.",
            "DISPLAY 1234567890. *> comment\n123456789.",
        ] {
            assert_eq!(syntax(source).oversized_numeric_literal(18), None);
        }
        assert_eq!(
            syntax(&format!(
                "{COMMA} DATA DIVISION. DISPLAY 1234567890.123456789."
            ))
            .oversized_numeric_literal(18),
            None
        );
    }

    #[test]
    fn quoted_text_comments_and_inactive_branches_are_not_numeric_literals() {
        let syntax = syntax(
            "DISPLAY '1234567890.123456789' X'1234567890123456789'.\n*> 1234567890.123456789\n>>IF 0 = 1\nCOMPUTE A = 1234567890.123456789.\n>>END-IF\n",
        );
        assert_eq!(syntax.oversized_numeric_literal(18), None);
    }
}
