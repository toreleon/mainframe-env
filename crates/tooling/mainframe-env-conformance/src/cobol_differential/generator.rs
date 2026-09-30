//! Seeded, bounded COBOL program grammar for differential exploration.
use super::Program;

struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }
    fn range(&mut self, n: usize) -> usize {
        (self.next() as usize) % n
    }
    fn coin(&mut self) -> bool {
        self.range(2) == 0
    }
}

fn literal(rng: &mut Rng, digits: usize, scale: usize, signed: bool) -> String {
    let max = 10u64.pow(digits.min(18) as u32) - 1;
    let value = match rng.range(5) {
        0 => 0,
        1 => max,
        _ => rng.next() % (max + 1),
    };
    let mut text = if scale == 0 {
        value.to_string()
    } else {
        format!("{:0width$}", value, width = scale + 1)
    };
    if scale > 0 {
        text.insert(text.len() - scale, '.');
    }
    if signed && rng.coin() && value != 0 {
        text.insert(0, '-');
    }
    text
}

/// The grammar builds only one suppression/floating family at a time. Check the
/// resulting picture independently before it enters a source program.
pub(super) fn well_formed_picture(pic: &str) -> bool {
    if pic.is_empty() || pic.len() > 30 || !pic.is_ascii() {
        return false;
    }
    let (body, suffix) = if let Some(body) = pic.strip_suffix("CR") {
        (body, "CR")
    } else if let Some(body) = pic.strip_suffix("DB") {
        (body, "DB")
    } else {
        (pic, "")
    };
    if body.is_empty() || body.matches('.').count() > 1 {
        return false;
    }
    let chars: Vec<char> = body.chars().collect();
    if !chars.iter().all(|c| "Z*9,B0/.$+-".contains(*c)) {
        return false;
    }
    let floating = body.contains('$');
    if floating
        && (body.contains('Z')
            || body.contains('*')
            || body.contains('+')
            || body.contains('-')
            || !suffix.is_empty())
    {
        return false;
    }
    if !suffix.is_empty()
        && (body.contains('+')
            || body.contains('-')
            || floating
            || body.contains('Z')
            || body.contains('*'))
    {
        return false;
    }
    if body.contains('Z') && body.contains('*') {
        return false;
    }
    let signs: Vec<usize> = chars
        .iter()
        .enumerate()
        .filter_map(|(i, c)| (*c == '+' || *c == '-').then_some(i))
        .collect();
    if !signs.is_empty() && !(signs.len() == 1 && (signs[0] == 0 || signs[0] == chars.len() - 1)) {
        return false;
    }
    let main = body
        .trim_start_matches(['+', '-'])
        .trim_end_matches(['+', '-']);
    if main.is_empty() || !main.chars().any(|c| "Z*9$".contains(c)) {
        return false;
    }
    if main.starts_with([',', 'B', '0', '/', '.']) || main.ends_with([',', 'B', '0', '/']) {
        return false;
    }
    if main.contains("..") || main.contains(".,") || main.contains(",.") {
        return false;
    }
    true
}

fn edited_picture(rng: &mut Rng) -> String {
    let width = 3 + rng.range(6);
    let fraction = rng.range(4);
    let family = rng.range(7);
    let mut body = match family {
        0 => format!("{}9", "Z".repeat(width - 1)),
        1 => format!("{}9", "*".repeat(width - 1)),
        2 => format!("{}9", "$".repeat(width - 1)),
        _ => "9".repeat(width),
    };
    if rng.coin() {
        let pos = 1 + rng.range(width - 1);
        body.insert(pos, [',', 'B', '0', '/'][rng.range(4)]);
    }
    if fraction > 0 {
        body.push('.');
        body.push_str(&"9".repeat(fraction));
    }
    match family {
        3 => body.insert(0, '+'),
        4 => body.push('-'),
        5 => body.push_str("CR"),
        6 => body.push_str("DB"),
        _ => {}
    }
    assert!(
        well_formed_picture(&body),
        "invalid grammar picture: {body}"
    );
    body
}

pub(super) fn generated(seed: u64) -> Program {
    let mut rng = Rng::new(seed);
    let item_count = 2 + rng.range(5);
    let operation_count = 1 + rng.range(4);
    let mut data = Vec::new();
    let mut numeric = Vec::new();
    let mut display_numeric = Vec::new();
    let mut edited = Vec::new();
    let mut alpha = Vec::new();
    // Always include a DISPLAY destination and a numeric operand.
    for index in 0..item_count {
        let name = format!("N{index}");
        let kind = if index < 2 { 0 } else { rng.range(5) };
        if kind == 3 {
            let width = 2 + rng.range(7);
            let text: String = (0..width)
                .map(|_| char::from(b'0' + rng.range(10) as u8))
                .collect();
            data.push(format!("01 {name} PIC X({width}) VALUE '{text}'."));
            alpha.push(name);
        } else if kind == 4 {
            let pic = edited_picture(&mut rng);
            data.push(format!("01 {name} PIC {pic}."));
            edited.push(name);
        } else {
            let (integer, scale) = if rng.range(8) == 0 {
                let scale = 10 + rng.range(8);
                (1 + rng.range(18 - scale), scale)
            } else {
                let integer = 1 + rng.range(18);
                (integer, rng.range((18 - integer).min(9) + 1))
            };
            let digits = integer + scale;
            let signed = rng.coin();
            let usage = if index == 0 {
                "DISPLAY"
            } else {
                ["DISPLAY", "COMP-3", "COMP", "COMP-5", "BINARY"][rng.range(5)]
            };
            let pic = format!(
                "{}9({integer}){}",
                if signed { "S" } else { "" },
                if scale == 0 {
                    String::new()
                } else {
                    format!("V9({scale})")
                }
            );
            let value = literal(&mut rng, digits, scale, signed);
            data.push(format!("01 {name} PIC {pic} USAGE {usage} VALUE {value}."));
            data.push(format!(
                "01 {name}-O PIC {pic} USAGE DISPLAY{}.",
                if signed {
                    " SIGN IS TRAILING SEPARATE"
                } else {
                    ""
                }
            ));
            numeric.push(name.clone());
            if usage == "DISPLAY" {
                display_numeric.push(name);
            }
        }
    }
    let mut statements = Vec::new();
    for _ in 0..operation_count {
        let target = if !edited.is_empty() && rng.range(5) == 0 {
            edited[rng.range(edited.len())].clone()
        } else {
            numeric[rng.range(numeric.len())].clone()
        };
        let source = if !alpha.is_empty() && display_numeric.contains(&target) && rng.range(5) == 0
        {
            if rng.coin() {
                alpha[rng.range(alpha.len())].clone()
            } else {
                format!("'{}'", 10 + rng.range(899999))
            }
        } else if rng.coin() {
            numeric[rng.range(numeric.len())].clone()
        } else {
            (1 + rng.range(99)).to_string()
        };
        let verb = rng.range(7);
        let rounded = if rng.coin() { " ROUNDED" } else { "" };
        let operation = if edited.contains(&target)
            || alpha.contains(&source)
            || source.starts_with('\'')
            || verb == 0
        {
            format!("MOVE {source} TO {target}.")
        } else {
            match verb {
                1 => format!("ADD {source} TO {target}{rounded}."),
                2 => format!("SUBTRACT {source} FROM {target}{rounded}."),
                3 => format!("MULTIPLY {source} BY {target}{rounded}."),
                4 => format!("DIVIDE {source} INTO {target}{rounded}."),
                5 => format!(
                    "DIVIDE {source} BY {} GIVING {target}{rounded}.",
                    1 + rng.range(9)
                ),
                _ => {
                    let op = ["+", "-", "*", "/"][rng.range(4)];
                    format!(
                        "COMPUTE {target}{rounded} = ({source} {op} {}).",
                        1 + rng.range(9)
                    )
                }
            }
        };
        statements.push(operation);
        if numeric.contains(&target) {
            statements.push(format!("MOVE {target} TO {target}-O."));
            statements.push(format!("DISPLAY {target}-O."));
        } else {
            statements.push(format!("DISPLAY {target}."));
        }
    }
    Program {
        class: String::new(),
        data,
        statements,
    }
}
