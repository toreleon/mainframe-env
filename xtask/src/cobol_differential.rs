use clap::Args as ClapArgs;
use mainframe_env_conformance::run_cobol_differential;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, ClapArgs)]
pub struct Args {
    #[arg(long)]
    pub check: bool,
    #[arg(long)]
    cobc: Option<PathBuf>,
    #[arg(long, help = "Inclusive seed range; requires --sweep")]
    seeds: Option<String>,
    #[arg(long, help = "Explicitly enable a larger sweep (at most 300 programs)")]
    sweep: bool,
    #[arg(long)]
    receipt: PathBuf,
}

pub fn run(root: &Path, args: &Args) -> Result<(), String> {
    let cobc = args
        .cobc
        .as_deref()
        .ok_or("cobol-differential requires --cobc <path>; an unrun campaign earns no credit")?;
    let (start, end) = seed_range(args)?;
    if !args.receipt.is_absolute() {
        return Err("--receipt must be an absolute path".into());
    }
    let parent = args
        .receipt
        .parent()
        .ok_or("receipt has no parent directory")?;
    let parent = fs::canonicalize(parent).map_err(|error| format!("receipt parent: {error}"))?;
    let root = fs::canonicalize(root).map_err(|error| error.to_string())?;
    if parent.starts_with(&root) && !parent.starts_with(root.join(".codex-runs")) {
        return Err(
            "receipt must be outside the checkout or under its .codex-runs directory".into(),
        );
    }
    let path = parent.join(args.receipt.file_name().ok_or("receipt has no filename")?);
    if path.exists() {
        return Err("receipt already exists; choose a fresh path".into());
    }
    let receipt = run_cobol_differential(cobc, start, end)?;
    assert_program_count(receipt.seeds_run, end - start + 1)?;
    let mut bytes = serde_json::to_vec_pretty(&receipt).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| format!("create receipt: {error}"))?;
    output
        .write_all(&bytes)
        .map_err(|error| format!("write receipt: {error}"))?;
    println!(
        "cobol-differential: programs={} rejected-reference={} rejected-product={} classes={} receipt={}",
        receipt.seeds_run,
        receipt.rejected_reference,
        receipt.rejected_product,
        receipt.divergences.len(),
        path.display()
    );
    if receipt
        .divergences
        .iter()
        .any(|case| case.classification == "generator-bug")
    {
        return Err("generator bug: reference rejected generated programs; see receipt".into());
    }
    Ok(())
}

fn seed_range(args: &Args) -> Result<(u64, u64), String> {
    if args.check && (args.sweep || args.seeds.is_some()) {
        return Err("--check uses its fixed 1..9 smoke program budget".into());
    }
    if !args.sweep {
        if args.seeds.is_some() {
            return Err("--seeds requires explicit --sweep".into());
        }
        Ok((1, 9))
    } else {
        let range = args
            .seeds
            .as_deref()
            .ok_or("--sweep requires --seeds <a..b>")?;
        let (a, b) = range
            .split_once("..")
            .ok_or("--seeds must have form a..b")?;
        let start = a.parse::<u64>().map_err(|_| "invalid first seed")?;
        let end = b.parse::<u64>().map_err(|_| "invalid last seed")?;
        if start > end || end - start >= 300 {
            return Err("seed range must contain 1..300 programs".into());
        }
        Ok((start, end))
    }
}

fn assert_program_count(actual: u64, expected: u64) -> Result<(), String> {
    if actual != expected {
        return Err(format!(
            "program count mismatch: expected {expected}, ran {actual}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cobol_differential_requires_explicit_sweep() {
        let mut args = Args {
            check: false,
            cobc: None,
            seeds: None,
            sweep: false,
            receipt: PathBuf::from("/receipt.json"),
        };
        assert_eq!(seed_range(&args).unwrap(), (1, 9));
        args.seeds = Some("1..50".into());
        assert!(seed_range(&args).is_err());
        args.sweep = true;
        assert_eq!(seed_range(&args).unwrap(), (1, 50));
        args.check = true;
        assert!(seed_range(&args).is_err());
        args.check = false;
        for range in ["3..2", "0..300", "bad", "18446744073709551616..1"] {
            args.seeds = Some(range.into());
            assert!(seed_range(&args).is_err(), "{range}");
        }
    }

    #[test]
    fn cobol_differential_smoke_asserts_program_count() {
        assert!(
            assert_program_count(8, 9).is_err(),
            "incomplete smoke accepted"
        );
        assert!(assert_program_count(9, 9).is_ok());
    }
}
