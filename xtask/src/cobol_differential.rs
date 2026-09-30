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
    #[arg(long, help = "Inclusive seed range, such as 1..60")]
    seeds: Option<String>,
    #[arg(long)]
    receipt: PathBuf,
}

pub fn run(root: &Path, args: &Args) -> Result<(), String> {
    let cobc = args
        .cobc
        .as_deref()
        .ok_or("cobol-differential requires --cobc <path>; an unrun campaign earns no credit")?;
    let (start, end) = if args.check {
        if args.seeds.is_some() {
            return Err("--check uses its fixed 1..9 smoke seed budget".into());
        }
        (1, 9)
    } else {
        let range = args.seeds.as_deref().ok_or("--seeds <a..b> is required")?;
        let (a, b) = range
            .split_once("..")
            .ok_or("--seeds must have form a..b")?;
        (
            a.parse::<u64>().map_err(|_| "invalid first seed")?,
            b.parse::<u64>().map_err(|_| "invalid last seed")?,
        )
    };
    if !args.receipt.is_absolute() {
        return Err("--receipt must be an absolute path".into());
    }
    let parent = args
        .receipt
        .parent()
        .ok_or("receipt has no parent directory")?;
    let parent = fs::canonicalize(parent).map_err(|error| format!("receipt parent: {error}"))?;
    let root = fs::canonicalize(root).map_err(|error| error.to_string())?;
    if parent.starts_with(&root) && parent != root.join(".codex-runs") {
        return Err("receipt must be outside the checkout or in its .codex-runs directory".into());
    }
    let path = parent.join(args.receipt.file_name().ok_or("receipt has no filename")?);
    if path.exists() {
        return Err("receipt already exists; choose a fresh path".into());
    }
    let receipt = run_cobol_differential(cobc, start, end)?;
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
        "cobol-differential: seeds={} rejected-reference={} rejected-product={} classes={} receipt={}",
        receipt.seeds_run,
        receipt.rejected_reference,
        receipt.rejected_product,
        receipt.divergences.len(),
        path.display()
    );
    Ok(())
}
