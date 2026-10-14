//! Delta debugging that retains only candidates accepted by the caller's trial.
use super::{Program, generator::well_formed_picture};

pub(super) fn minimize<F>(mut program: Program, mut divergent: F) -> Program
where
    F: FnMut(&Program) -> bool,
{
    loop {
        let before = program.source();
        for statements in [true, false] {
            let mut chunk = if statements {
                program.statements.len()
            } else {
                program.data.len()
            };
            while chunk > 0 {
                let mut index = 0;
                loop {
                    let len = if statements {
                        program.statements.len()
                    } else {
                        program.data.len()
                    };
                    if index >= len {
                        break;
                    }
                    let mut candidate = program.clone();
                    let items = if statements {
                        &mut candidate.statements
                    } else {
                        &mut candidate.data
                    };
                    items.drain(index..(index + chunk).min(len));
                    if divergent(&candidate) {
                        program = candidate;
                    } else {
                        index += chunk;
                    }
                }
                chunk /= 2;
            }
        }
        for index in 0..program.data.len() {
            while let Some((name, picture)) = declaration(&program.data[index]) {
                let name = name.trim_end_matches("-O").to_owned();
                let picture = picture.to_owned();
                let mut changed = false;
                for smaller in pictures(&picture) {
                    let mut candidate = program.clone();
                    // Numeric storage and its canonical DISPLAY companion shrink together.
                    for item in &mut candidate.data {
                        if let Some((item_name, item_picture)) = declaration(item)
                            && (item_name == name || item_name == format!("{name}-O"))
                            && item_picture == picture
                        {
                            *item = item.replacen(
                                &format!("PIC {picture}"),
                                &format!("PIC {smaller}"),
                                1,
                            );
                        }
                    }
                    if candidate.data.iter().all(|item| value_fits(item)) && divergent(&candidate) {
                        program = candidate;
                        changed = true;
                        break;
                    }
                }
                if !changed {
                    break;
                }
            }
        }
        if program.source() == before {
            return program;
        }
    }
}

fn declaration(item: &str) -> Option<(&str, &str)> {
    Some((
        item.split_whitespace().nth(1)?,
        item.split("PIC ")
            .nth(1)?
            .split_whitespace()
            .next()?
            .trim_end_matches('.'),
    ))
}

fn pictures(picture: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let mut offset = 0;
    while let Some(start) = picture[offset..].find("9(") {
        let start = offset + start;
        let Some(end) = picture[start..].find(')').map(|end| start + end) else {
            break;
        };
        if let Ok(width) = picture[start + 2..end].parse::<usize>() {
            for reduced in 1..width {
                candidates.push(format!(
                    "{}9({reduced}){}",
                    &picture[..start],
                    &picture[end + 1..]
                ));
            }
        }
        offset = end + 1;
    }
    // Edited pictures use expanded runs. Keep punctuation and the sign family.
    if !picture.contains('(') {
        for (index, symbol) in picture.char_indices() {
            if "9Z*$+-".contains(symbol) {
                let mut smaller = picture.to_owned();
                smaller.remove(index);
                if well_formed_picture(&smaller) {
                    candidates.push(smaller);
                }
            }
        }
    }
    candidates
}

fn value_fits(item: &str) -> bool {
    let Some((_, picture)) = declaration(item) else {
        return true;
    };
    let Some(value) = item.split(" VALUE ").nth(1) else {
        return true;
    };
    if !picture.starts_with(['9', 'S']) || picture.contains(['.', ',', 'B', '/']) {
        return true;
    }
    let digits = |part: &str| {
        part.strip_prefix("9(")
            .and_then(|s| s.strip_suffix(')'))
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or_else(|| part.chars().filter(|c| *c == '9').count())
    };
    let (integer, scale) = picture
        .trim_start_matches('S')
        .split_once('V')
        .unwrap_or((picture.trim_start_matches('S'), ""));
    let value = value.trim_end_matches('.');
    let (whole, fraction) = value
        .trim_start_matches(['+', '-'])
        .split_once('.')
        .unwrap_or((value.trim_start_matches(['+', '-']), ""));
    whole.trim_start_matches('0').len() <= digits(integer)
        && fraction.trim_end_matches('0').len() <= digits(scale)
        && (!value.starts_with('-') || picture.starts_with('S'))
}
