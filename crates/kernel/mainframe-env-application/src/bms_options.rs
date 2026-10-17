//! Declared BMS operands. This is metadata retention, not macro assembly.
use crate::InstallProblem;
use std::collections::BTreeMap;

// Keep quoted spelling; normalize only syntax outside quotes. Balanced lists
// and doubled quotes keep commas within an operand rather than splitting it.
pub(crate) fn parse_operands(source: &str) -> Result<BTreeMap<String, String>, InstallProblem> {
    let mut operands = BTreeMap::new();
    let mut token = String::new();
    let mut depth = 0usize;
    let mut quote = None;
    let mut chars = source.chars().peekable();
    while let Some(ch) = chars.next() {
        if let Some(delimiter) = quote {
            token.push(ch);
            if ch == delimiter {
                if chars.peek() == Some(&delimiter) {
                    token.push(chars.next().expect("peeked quote"));
                } else {
                    quote = None;
                }
            }
            continue;
        }
        match ch {
            '\'' | '"' => {
                quote = Some(ch);
                token.push(ch);
            }
            '(' => {
                depth = depth.checked_add(1).ok_or(InstallProblem::LimitExceeded)?;
                token.push(ch);
            }
            ')' => {
                depth = depth
                    .checked_sub(1)
                    .ok_or(InstallProblem::InvalidIdentity)?;
                token.push(ch);
            }
            ',' if depth == 0 => insert_operand(&mut operands, &mut token)?,
            ch if ch.is_whitespace() => {}
            ch => token.push(ch.to_ascii_uppercase()),
        }
    }
    if quote.is_some() || depth != 0 {
        return Err(InstallProblem::InvalidIdentity);
    }
    insert_operand(&mut operands, &mut token)?;
    Ok(operands)
}

fn insert_operand(
    operands: &mut BTreeMap<String, String>,
    token: &mut String,
) -> Result<(), InstallProblem> {
    if token.is_empty() {
        return Ok(());
    }
    let token = std::mem::take(token);
    let (key, value) = token.split_once('=').unwrap_or((&token, ""));
    if key.is_empty()
        || !key
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == b'_')
        || (token.contains('=') && value.is_empty())
        || operands.insert(key.into(), value.into()).is_some()
    {
        return Err(InstallProblem::InvalidIdentity);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_bms;

    #[test]
    fn lists_quoted_commas_and_doubled_quotes_remain_one_declared_operand() {
        let operands =
            parse_operands("dsatts=(color, hilight), INITIAL='A, B''s', X=(A,(B,C)),ACTIVATE")
                .unwrap();
        assert_eq!(operands["DSATTS"], "(COLOR,HILIGHT)");
        assert_eq!(operands["INITIAL"], "'A, B''s'");
        assert_eq!(operands["X"], "(A,(B,C))");
        assert_eq!(operands["ACTIVATE"], "");
    }

    #[test]
    fn ambiguous_duplicate_or_unbalanced_declared_operands_refuse() {
        for source in [
            "COLOR=BLUE,COLOR=RED",
            "COLOR=",
            "=(X)",
            "DSATTS=(COLOR",
            "X=COLOR)",
            "INITIAL='OPEN",
        ] {
            assert!(parse_operands(source).is_err(), "{source}");
        }
    }

    #[test]
    fn mapset_map_and_field_continuations_preserve_declared_options() {
        let map = parse_bms("SET DFHMSD EXTATT=YES,-\n TIOAPFX=YES,TYPE=&&SYSPARM\nMAP DFHMDI SIZE=(24,80),-\n DSATTS=(COLOR,HILIGHT),MAPATTS=(COLOR,HILIGHT)\nFIELD DFHMDF LENGTH=2,POS=(1,1),-\n COLOR=BLUE,HILIGHT=UNDERLINE\n DFHMSD TYPE=FINAL").unwrap();
        assert_eq!(map.mapset_options["TYPE"], "&&SYSPARM");
        assert_eq!(map.mapset_options["EXTATT"], "YES");
        assert_eq!(map.mapset_options["TIOAPFX"], "YES");
        assert_eq!(map.map_options["DSATTS"], "(COLOR,HILIGHT)");
        assert_eq!(map.map_options["MAPATTS"], "(COLOR,HILIGHT)");
        assert_eq!(map.fields[0].options["COLOR"], "BLUE");
        assert_eq!(map.fields[0].options["HILIGHT"], "UNDERLINE");
    }

    #[test]
    fn explicit_field_map_mapset_precedence_retains_default_labels_without_synthesis() {
        let mut map = parse_bms("SET DFHMSD COLOR=BLUE,HILIGHT=BLINK\nMAP DFHMDI SIZE=(24,80),COLOR=RED,HILIGHT=REVERSE\nFIELD DFHMDF LENGTH=2,POS=(1,1),COLOR=DEFAULT,HILIGHT=OFF").unwrap();
        assert_eq!(map.field_color(&map.fields[0]), Some("DEFAULT"));
        assert_eq!(map.field_highlight(&map.fields[0]), Some("OFF"));
        map.fields[0].options.clear();
        assert_eq!(map.field_color(&map.fields[0]), Some("RED"));
        assert_eq!(map.field_highlight(&map.fields[0]), Some("REVERSE"));
        map.map_options.clear();
        assert_eq!(map.field_color(&map.fields[0]), Some("BLUE"));
        assert_eq!(map.field_highlight(&map.fields[0]), Some("BLINK"));
        map.mapset_options.clear();
        assert_eq!(map.field_color(&map.fields[0]), None);
        assert_eq!(map.field_highlight(&map.fields[0]), None);
    }

    #[test]
    fn exact_seventy_star_literal_continuation_preserves_next_anonymous_statement() {
        let source = format!(
            "SET DFHMSD TYPE=MAP\nMAP DFHMDI SIZE=(24,80)\n DFHMDF COLOR=YELLOW,LENGTH=70,POS=(8,6),-\n INITIAL='{}-\n {}'\n DFHMDF LENGTH=11,POS=(11,6),INITIAL='First Name:'",
            "*".repeat(47),
            "*".repeat(23)
        );
        let map = parse_bms(&source).unwrap();
        assert_eq!(map.fields.len(), 2);
        assert_eq!(
            map.fields[0].initial.as_deref(),
            Some("*".repeat(70).as_str())
        );
        assert_eq!(
            map.fields[0].options["INITIAL"],
            format!("'{}'", "*".repeat(70))
        );
        assert_eq!(map.fields[1].name, None);
        assert_eq!(map.fields[1].initial.as_deref(), Some("First Name:"));
        assert_eq!(map.fields[1].position, Some((11, 6)));
    }

    #[test]
    fn escaped_quote_inside_continued_string_and_real_comments_keep_statement_boundaries() {
        let map = parse_bms("* comment with an unmatched ' quote\nSET DFHMSD TYPE=MAP\nMAP DFHMDI SIZE=(24,80)\nFIELD DFHMDF LENGTH=20,POS=(1,1),INITIAL='A''-\n *B',COLOR=BLUE\n* real comment with ' and DFHMDF\nNEXT DFHMDF LENGTH=2,POS=(2,1),INITIAL='OK'").unwrap();
        assert_eq!(map.fields.len(), 2);
        assert_eq!(map.fields[0].options["INITIAL"], "'A''*B'");
        assert_eq!(map.fields[0].options["COLOR"], "BLUE");
        assert_eq!(map.fields[1].name.as_deref(), Some("NEXT"));
        assert_eq!(map.fields[1].initial.as_deref(), Some("OK"));
        assert_eq!(map.fields[0].initial.as_deref(), Some("A'*B"));
    }

    #[test]
    fn open_string_prefix_is_not_a_comment_and_incomplete_string_refuses() {
        let map = parse_bms("SET DFHMSD TYPE=MAP\nMAP DFHMDI SIZE=(24,80)\nFIELD DFHMDF LENGTH=5,POS=(1,1),INITIAL='AB-\n ***'").unwrap();
        assert_eq!(map.fields[0].initial.as_deref(), Some("AB***"));
        let spaced = parse_bms("SET DFHMSD TYPE=MAP\nMAP DFHMDI SIZE=(24,80)\nFIELD DFHMDF LENGTH=7,POS=(1,1),INITIAL='AB -\n ***'").unwrap();
        assert_eq!(spaced.fields[0].initial.as_deref(), Some("AB ***"));
        for source in [
            "SET DFHMSD TYPE=MAP\nMAP DFHMDI SIZE=(24,80)\nFIELD DFHMDF INITIAL='OPEN",
            "SET DFHMSD TYPE=MAP\nMAP DFHMDI SIZE=(24,80)\nFIELD DFHMDF INITIAL='OPEN-",
        ] {
            assert!(parse_bms(source).is_err());
        }
    }
}
