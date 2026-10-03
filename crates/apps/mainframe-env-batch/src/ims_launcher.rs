//! Bounded launch selection for the existing package-owned IMS controllers.
use crate::controller::BatchControllerSelector;
use mainframe_env_host_api::HostProblem;

pub(crate) fn selector(parameter: &str) -> Result<BatchControllerSelector, HostProblem> {
    if parameter.len() > 512 {
        return Err(HostProblem::ResourceExhausted);
    }
    let normalized = parameter
        .trim()
        .trim_matches(|character| matches!(character, '\'' | '"' | '(' | ')'));
    let fields = normalized.split(',').map(str::trim).collect::<Vec<_>>();
    if fields.len() < 2 || fields.len() > 14 {
        return Err(HostProblem::Unsupported);
    }
    // This local controller has no external DBRC authority. Accept only omitted
    // controls, or the explicit disabled form in DB batch; never drop an enabled
    // or otherwise supplied operational option on the way to package selection.
    if fields
        .iter()
        .skip(3)
        .take(10)
        .any(|value| !value.is_empty())
        || fields.get(13).is_some_and(|value| {
            !value.is_empty()
                && (!value.eq_ignore_ascii_case("N") || !fields[0].eq_ignore_ascii_case("DLI"))
        })
    {
        return Err(HostProblem::Unsupported);
    }
    BatchControllerSelector::ims(
        fields[0],
        fields[1],
        fields.get(2).copied().filter(|v| !v.is_empty()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_disabled_dbrc_and_omitted_controls_select_the_same_signed_route() {
        let expected = BatchControllerSelector::ims("DLI", "ANYPROG", Some("ANYPSB")).unwrap();
        for parameter in [
            "DLI,ANYPROG,ANYPSB",
            "DLI,ANYPROG,ANYPSB,,,,,,,,,,,N",
            "'dli,anyprog,anypsb,,,,,,,,,,,n'",
        ] {
            assert_eq!(selector(parameter), Ok(expected.clone()));
        }
        assert!(selector("BMP,ANYPROG,ANYPSB").is_ok());
    }

    #[test]
    fn operational_controls_are_never_silently_dropped() {
        for parameter in [
            "DLI,ANYPROG,ANYPSB,,,,,,,,,,,Y",
            "DLI,ANYPROG,ANYPSB,,,,,,,,,,,X",
            "BMP,ANYPROG,ANYPSB,,,,,,,,,,,N",
            "DLI,ANYPROG,ANYPSB,VALUE",
            "DLI,ANYPROG,ANYPSB,,,,,,,,,,,N,",
            "DLI",
            "DLI,,ANYPSB",
        ] {
            assert!(selector(parameter).is_err(), "{parameter}");
        }
        assert_eq!(
            selector(&"x".repeat(513)),
            Err(HostProblem::ResourceExhausted)
        );
    }
}
