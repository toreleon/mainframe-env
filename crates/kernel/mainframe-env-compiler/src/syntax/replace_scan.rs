//! Recognize COBOL REPLACE directives outside EXEC blocks.

pub(super) fn is_replace_directive(word: &str, in_exec: &mut bool) -> bool {
    if word.eq_ignore_ascii_case("EXEC") {
        *in_exec = true;
    } else if word.eq_ignore_ascii_case("END-EXEC") {
        *in_exec = false;
    }
    !*in_exec && word.eq_ignore_ascii_case("REPLACE")
}
