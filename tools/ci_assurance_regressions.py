"""Exercise actual fast gates against intentional corruptions in a disposable Git worktree."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import time


def run(root, binary, command, output, label, expected_error=None):
    start = time.monotonic()
    result = subprocess.run(
        [str(binary), command, "--check"], cwd=root, stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT, timeout=900,
    )
    log = output / (label + ".log")
    log.write_bytes(result.stdout)
    if expected_error is None:
        ok = result.returncode == 0
    else:
        ok = result.returncode != 0 and expected_error.encode() in result.stdout
    row = {
        "gate": command,
        "purpose": "baseline" if expected_error is None else "intentional-checker-regression",
        "exit_code": result.returncode,
        "expected_error": expected_error,
        "detected_as_expected": ok,
        "seconds": round(time.monotonic() - start, 6),
        "log_sha256": hashlib.sha256(result.stdout).hexdigest(),
    }
    print(json.dumps(row), flush=True)
    return row


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--xtask", type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    binary = args.xtask.resolve()
    output = args.output.resolve()
    if subprocess.check_output(["git", "status", "--porcelain"], cwd=root).strip():
        raise ValueError("commit the intended candidate before exercising gates")
    if not binary.is_file():
        raise ValueError("build this candidate with cargo build --locked -p xtask first")
    candidate = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
    tree = subprocess.check_output(["git", "rev-parse", "HEAD^{tree}"], cwd=root, text=True).strip()
    output.mkdir(parents=True, exist_ok=False)
    receipt = {
        "schema_version": "mainframe-env.ci-gate-experiment@1",
        "candidate": candidate,
        "tree": tree,
        "checker_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "rustc": subprocess.check_output(["rustc", "-Vv"], text=True),
        "cargo": subprocess.check_output(["cargo", "-V"], text=True),
        "experiments": [],
        "product_mutation_credit": 0,
        "licensed_credit": 0,
        "cost_boundary": "command wall seconds on the same runner; excludes runner provisioning, "
                         "parallel jobs and billing rounding",
    }
    with tempfile.TemporaryDirectory(prefix="ci-gates-") as temporary:
        work = Path(temporary) / "tree"
        subprocess.run(["git", "worktree", "add", "--detach", str(work), candidate],
                       cwd=root, check=True, stdout=subprocess.DEVNULL)
        try:
            # Identical source, toolchain and runner; the broader architecture gate
            # additionally executes two targeted runtime tests, not a release build.
            for label, command in [("architecture-fast", "architecture-fast"),
                                   ("architecture-with-runtime-tests", "architecture")]:
                receipt["experiments"].append(run(work, binary, command, output, label))
            manifest = work / "crates/contracts/mainframe-env-host-api/Cargo.toml"
            original = manifest.read_text()
            try:
                manifest.write_text(original + "\ntokio.workspace = true\n")
                receipt["experiments"].append(run(work, binary, "architecture-fast", output,
                                                  "bad-architecture", "depends on infrastructure tokio"))
            finally:
                manifest.write_text(original)
        finally:
            subprocess.run(["git", "worktree", "remove", "--force", str(work)], cwd=root, check=True)
    receipt["success"] = all(row["detected_as_expected"] for row in receipt["experiments"])
    (output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    return 0 if receipt["success"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
