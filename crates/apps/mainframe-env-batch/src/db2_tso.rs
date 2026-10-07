//! Bounded DSN command sessions used by the owned Db2 batch launcher.
use mainframe_env_host_api::HostProblem;

pub(crate) enum Action {
    Free(Vec<String>),
    Run { program: String, command: String },
}

pub(crate) fn parse(control: &str) -> Result<Action, HostProblem> {
    if control.len() > 64 * 1024 {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut text = String::new();
    let mut rest = control;
    while let Some(start) = rest.find("/*") {
        text.push_str(&rest[..start]);
        text.push(' ');
        let end = rest[start + 2..].find("*/").ok_or(HostProblem::Malformed)?;
        rest = &rest[start + 2 + end + 2..];
    }
    text.push_str(rest);
    let mut commands = Vec::new();
    let mut continued = String::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let (line, continuation) = line
            .strip_suffix('-')
            .map_or((line, false), |line| (line.trim_end(), true));
        if !continued.is_empty() {
            continued.push(' ');
        }
        continued.push_str(line);
        if !continuation {
            commands.push(std::mem::take(&mut continued));
        }
    }
    if !continued.is_empty() || commands.is_empty() {
        return Err(HostProblem::Malformed);
    }
    if commands.len() > 34 {
        return Err(HostProblem::ResourceExhausted);
    }
    if commands[0].to_ascii_uppercase().starts_with("DSN ") {
        let system = commands.remove(0);
        let upper = system.to_ascii_uppercase();
        let value = upper
            .strip_prefix("DSN SYSTEM(")
            .and_then(|v| v.strip_suffix(')'))
            .ok_or(HostProblem::Unsupported)?;
        if !identifier(value, false) {
            return Err(HostProblem::Malformed);
        }
        if commands
            .last()
            .is_some_and(|v| v.eq_ignore_ascii_case("END"))
        {
            commands.pop();
        }
    }
    if commands.is_empty() {
        return Err(HostProblem::Malformed);
    }
    if commands
        .iter()
        .all(|v| v.to_ascii_uppercase().starts_with("FREE "))
    {
        for command in &commands {
            let upper = command.to_ascii_uppercase();
            let value = upper
                .strip_prefix("FREE PLAN(")
                .map(|v| (v, false))
                .or_else(|| upper.strip_prefix("FREE PACKAGE(").map(|v| (v, true)))
                .and_then(|(v, wildcard)| v.strip_suffix(')').map(|v| (v, wildcard)))
                .ok_or(HostProblem::Unsupported)?;
            if !identifier(value.0, value.1) {
                return Err(HostProblem::Malformed);
            }
        }
        return Ok(Action::Free(commands));
    }
    if commands.len() != 1 {
        return Err(HostProblem::Unsupported);
    }
    let command = commands.pop().expect("one command").to_ascii_uppercase();
    let tail = command
        .strip_prefix("RUN PROGRAM(")
        .ok_or(HostProblem::Unsupported)?;
    let (program, mut qualifiers) = tail.split_once(')').ok_or(HostProblem::Malformed)?;
    if !identifier(program, false) || program.contains('.') {
        return Err(HostProblem::Malformed);
    }
    let mut seen = std::collections::BTreeSet::new();
    while !qualifiers.trim().is_empty() {
        qualifiers = qualifiers.trim_start();
        let (name, tail) = qualifiers.split_once('(').ok_or(HostProblem::Unsupported)?;
        if !matches!(name, "PLAN" | "PARMS" | "LIB") || !seen.insert(name) {
            return Err(HostProblem::Unsupported);
        }
        let (value, tail) = if let Some(tail) = tail.strip_prefix('\'') {
            let (value, tail) = tail.split_once('\'').ok_or(HostProblem::Malformed)?;
            (value, tail.strip_prefix(')').ok_or(HostProblem::Malformed)?)
        } else {
            tail.split_once(')').ok_or(HostProblem::Malformed)?
        };
        match name {
            "PLAN" | "LIB" if identifier(value, false) => {}
            "PARMS"
                if matches!(
                    (program, value),
                    ("DSNTIAD", "RC0") | ("DSNTEP4", "/ALIGN(LHS) MIXED") | ("DSNTIAUL", "SQL")
                ) => {}
            _ => return Err(HostProblem::Unsupported),
        }
        qualifiers = tail;
    }
    Ok(Action::Run {
        program: program.into(),
        command,
    })
}

fn identifier(value: &str, wildcard: bool) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.split('.').enumerate().all(|(i, part)| {
            (!part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"@#$".contains(&b)))
                || (wildcard
                    && part == "*"
                    && i > 0
                    && i + 1 == value.split('.').count()
                    && value.ends_with(".*"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_free_session_retains_each_command() {
        let Action::Free(commands) = parse("/* copyright */\n DSN SYSTEM(DAZ1)\n FREE PLAN(CARDDEMO)\n FREE PLAN(COTRTLIC)\n FREE PACKAGE(COTRTLIC.*)\n END").unwrap() else { panic!() };
        assert_eq!(commands.len(), 3);
        assert_eq!(commands[2], "FREE PACKAGE(COTRTLIC.*)");
    }

    #[test]
    fn upstream_run_continuations_preserve_program_and_controls() {
        for (program, params) in [
            ("DSNTIAD", "RC0"),
            ("DSNTEP4", "/ALIGN(LHS) MIXED"),
            ("DSNTIAUL", "SQL"),
        ] {
            let input = format!(
                "DSN SYSTEM(DAZ1)\n RUN PROGRAM({program}) -\n PLAN({program}) -\n PARMS('{params}')\n"
            );
            let Action::Run {
                program: actual, ..
            } = parse(&input).unwrap()
            else {
                panic!()
            };
            assert_eq!(actual, program);
        }
        assert!(matches!(
            parse("RUN PROGRAM(DSNTIAUL)"),
            Ok(Action::Run { .. })
        ));
    }

    #[test]
    fn unknown_mixed_or_incomplete_controls_are_rejected() {
        for input in [
            "DSN SYSTEM(DAZ1)\nEND",
            "RUN PROGRAM(DSNTIAD) -",
            "RUN PROGRAM(DSNTIAD)\nDELETE X",
            "FREE PLAN(X)\nRUN PROGRAM(DSNTIAD)",
            "RUN PROGRAM(DSNTIAD) PARMS('UNKNOWN')",
            "RUN PROGRAM(DSNTIAD) PLAN(X) PLAN(Y)",
            "/* unterminated",
            "FREE PACKAGE(X.*.Y)",
        ] {
            assert!(parse(input).is_err(), "{input}");
        }
    }
}
