#!/usr/bin/env python3
"""Independent CardDemo TRANREPT stage reference (stdlib plus GnuCOBOL).

The checked-in COBOL SORT program translates the TRANREPT DFSORT card. Its
secondary input-position key makes equal card numbers retain input order.
This is a declared reference policy, not a claim about every DFSORT option.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile


ROOT = Path(__file__).resolve().parents[2]
DEFAULT_CORPUS = (ROOT.parents[3] / "workbench/corpora/aws-mainframe-modernization-carddemo")
INPUT = "conformance/0.8/oracles/carddemo-tranrept-input@1.bin"
SORT = "conformance/0.8/oracles/carddemo-tranrept-sort@1.cbl"
SCRIPT = "conformance/tools/carddemo_tranrept_reference.py"
MANIFEST = ROOT / "conformance/0.8/oracles/carddemo-tranrept-reference@1.json"
CORPUS_FILES = ["app/cbl/CBTRN03C.cbl"] + [
    f"app/cpy/{name}.cpy"
    for name in ("CVTRA05Y", "CVACT03Y", "CVTRA03Y", "CVTRA04Y", "CVTRA07Y")
]
SCOPE = (
    "validates the SORT/INCLUDE and CBTRN03C report stage for the pinned input; "
    "the input itself is produced by this implementation's upstream jobs and "
    "is not independently validated"
)


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def file_sha(path):
    return sha256(Path(path).read_bytes())


def dd_parameters(jcl, name):
    """Return a DD card and its continuation lines from source JCL."""
    lines = jcl.splitlines()
    for index, line in enumerate(lines):
        if re.match(rf"^//{re.escape(name)}\s+DD\b", line):
            card = [line[2:]]
            for continuation in lines[index + 1:]:
                if not continuation.startswith("// "):
                    break
                card.append(continuation[2:])
            return " ".join(card)
    raise ValueError(f"missing {name} DD in TRANREPT JCL")


def tranrept_dataset_metadata(jcl):
    """Resolve SORTOUT's DCB referback through SORTIN to the backup DD."""
    backup = dd_parameters(jcl, "PRC001.FILEOUT")
    sortin = dd_parameters(jcl, "SORTIN")
    sortout = dd_parameters(jcl, "SORTOUT")
    report = dd_parameters(jcl, "TRANREPT")
    if not re.search(r"\bDSN=AWS\.M2\.CARDDEMO\.TRANSACT\.BKUP\(\+1\)", sortin):
        raise ValueError("SORTIN does not read the backup generation")
    if not re.search(r"\bDCB=\(\*\.SORTIN\)", sortout):
        raise ValueError("SORTOUT does not refer back to SORTIN DCB")

    def explicit_dcb(card):
        match = re.search(r"\bDCB=\(LRECL=(\d+),RECFM=(F|FB),BLKSIZE=\d+\)", card)
        if not match:
            raise ValueError("expected explicit fixed-record DCB in TRANREPT JCL")
        return {"organization": "Sequential",
                "recfm": {"F": "Fixed", "FB": "FixedBlocked"}[match[2]],
                "lrecl": int(match[1]), "ccsid": 37}

    source = explicit_dcb(backup)
    output = explicit_dcb(report)
    return source, source.copy(), output


def field(digest, value):
    """Mirror carddemo.rs digest_field: u64 big-endian length, then bytes."""
    digest.update(len(value).to_bytes(8, "big"))
    digest.update(value)


def framed_dataset(records, lrecl, version, *, record_format="Fixed", record_length=None):
    """Sequential, declared RECFM/LRECL, absent key u32::MAX, CCSID 37 u16.

    The receipt frames attributes, dataset version, read version, each CP037
    logical record, then each sequential position identity (u64 big-endian).
    """
    digest = hashlib.sha256()
    for value in (
        b"Sequential", record_format.encode("ascii"), lrecl.to_bytes(4, "big"),
        (2**32 - 1).to_bytes(4, "big"), (2**32 - 1).to_bytes(4, "big"),
        (37).to_bytes(2, "big"), version.to_bytes(8, "big"),
        version.to_bytes(8, "big"),
    ):
        field(digest, value)
    for record in records:
        if len(record) != (record_length or lrecl):
            raise ValueError(f"record length {len(record)} differs from {record_length or lrecl}")
        field(digest, record)
    for position in range(len(records)):
        field(digest, position.to_bytes(8, "big"))
    return digest.hexdigest()


def records(data, lrecl):
    if len(data) % lrecl:
        raise ValueError(f"byte length {len(data)} is not divisible by {lrecl}")
    return [data[i:i + lrecl] for i in range(0, len(data), lrecl)]


def cp037_to_native(data):
    return data.decode("cp037").encode("ascii")


def transaction_to_native(data):
    """Translate the DISPLAY signed amount at byte 143 to ASCII overpunch."""
    value = bytearray(cp037_to_native(data))
    for start in range(0, len(value), 350):
        sign = value[start + 142]
        if sign in b"{ABCDEFGHI":
            value[start + 142] = b"0123456789"[b"{ABCDEFGHI".index(sign)]
        elif sign in b"}JKLMNOPQR":
            value[start + 142] = b"pqrstuvwxy"[b"}JKLMNOPQR".index(sign)]
        elif sign not in b"0123456789":
            raise ValueError(f"unsupported transaction sign byte {sign:02x}")
    return bytes(value)


def native_to_cp037(data):
    return data.decode("ascii").encode("cp037")


def compare_product(name, derived, lrecl):
    dump = os.getenv("CARDDEMO_TRANREPT_DUMP_DIR")
    if dump:
        Path(dump).mkdir(parents=True, exist_ok=True)
        (Path(dump) / name).write_bytes(b"".join(derived))
    directory = os.getenv("CARDDEMO_TRANREPT_COMPARE_DIR")
    if not directory:
        return
    product = records((Path(directory) / name).read_bytes(), lrecl)
    for position, (left, right) in enumerate(zip(derived, product)):
        if left != right:
            offset = next(i for i, pair in enumerate(zip(left, right)) if pair[0] != pair[1])
            print(f"{name}: first difference record {position + 1}, byte {offset + 1}: "
                  f"independent {left[offset]:02x}, product {right[offset]:02x}")
            break
    else:
        print(f"{name}: first {min(len(derived), len(product))} records equal; "
              f"counts {len(derived)} independent, {len(product)} product")


def run(command, *, cwd=None, env=None):
    subprocess.run(command, cwd=cwd, env=env, check=True, stdout=subprocess.PIPE,
                   stderr=subprocess.PIPE)


def build_index(cobc, directory, seed, width, key_width, name):
    """Load a corpus sequential seed into a GnuCOBOL BDB indexed file."""
    source = directory / f"{name}.input"
    source.write_bytes(cp037_to_native(seed.read_bytes()))
    loader = directory / f"{name}.cbl"
    loader.write_text(f"""       identification division.
       program-id. load-{name.lower()}.
       environment division.
       input-output section.
       file-control.
           select source-file assign to "SOURCE"
               organization is sequential.
           select index-file assign to "INDEXFILE"
               organization is indexed access mode is dynamic
               record key is index-key.
       data division.
       file section.
       fd source-file.
       01 source-record pic x({width}).
       fd index-file.
       01 index-record.
          05 index-key pic x({key_width}).
          05 index-rest pic x({width - key_width}).
       working-storage section.
       01 end-input pic 9 value zero.
       procedure division.
           open input source-file
           open output index-file
           perform until end-input = 1
               read source-file
                   at end move 1 to end-input
                   not at end
                       move source-record to index-record
                       write index-record
               end-read
           end-perform
           close source-file index-file
           goback.
""")
    executable = directory / f"load-{name}"
    run([str(cobc), "-x", "-std=ibm", "-o", str(executable), str(loader)])
    env = os.environ.copy()
    env.update(SOURCE=str(source), INDEXFILE=str(directory / name))
    run([str(executable)], cwd=directory, env=env)


def derive(cobc, corpus, out):
    cobc = Path(cobc).resolve()
    corpus = Path(corpus).resolve()
    data = (ROOT / INPUT).read_bytes()
    tool_version = subprocess.check_output([str(cobc), "--version"], text=True).splitlines()[0]
    tool_info = subprocess.check_output([str(cobc), "--info"], text=True)
    if "indexed file handler     : BDB" not in tool_info or "native EBCDIC            : no" not in tool_info:
        raise ValueError("GnuCOBOL must use BDB indexed files without native EBCDIC")
    jcl = corpus / "app/jcl/TRANREPT.jcl"
    jcl_text = jcl.read_text()
    source_metadata, selected_metadata, report_metadata = tranrept_dataset_metadata(jcl_text)
    source_metadata["version"] = 3
    selected_metadata["version"] = 3
    report_metadata["version"] = 522
    if source_metadata["lrecl"] != 350 or report_metadata["lrecl"] != 133:
        raise ValueError("TRANREPT JCL record lengths differ from the reference programs")
    source_records = records(data, source_metadata["lrecl"])
    for parameter in ("PARM-START-DATE,C'2022-01-01'", "PARM-END-DATE,C'2022-07-06'"):
        if parameter not in jcl_text:
            raise ValueError(f"TRANREPT JCL date parameter differs: {parameter}")
    with tempfile.TemporaryDirectory(prefix="carddemo-tranrept-reference-") as temporary:
        work = Path(temporary)
        (work / "sortin").write_bytes(cp037_to_native(data))
        sort_binary = work / "sort-reference"
        run([str(cobc), "-x", "-std=ibm", "-o", str(sort_binary), str(ROOT / SORT)])
        env = os.environ.copy()
        env.update(SORTIN=str(work / "sortin"), SORTOUT=str(work / "sortout"))
        run([str(sort_binary)], cwd=work, env=env)
        selected = records(native_to_cp037((work / "sortout").read_bytes()), 350)
        compare_product("tranrept-selected.bin", selected, 350)
        for name, width, key_width in (
            ("CARDXREF", 50, 16), ("TRANTYPE", 60, 2), ("TRANCATG", 60, 6)
        ):
            seed = corpus / f"app/data/EBCDIC/AWS.M2.CARDDEMO.{name}.PS"
            build_index(cobc, work, seed, width, key_width, name)
        (work / "sortout").write_bytes(transaction_to_native(b"".join(selected)))
        (work / "dateparm").write_bytes(b"2022-01-01 2022-07-06".ljust(80, b" "))
        report_binary = work / "CBTRN03C"
        run([str(cobc), "-x", "-std=ibm", "-I", str(corpus / "app/cpy"),
             "-o", str(report_binary), str(corpus / "app/cbl/CBTRN03C.cbl")])
        env.update(
            TRANFILE=str(work / "sortout"), CARDXREF=str(work / "CARDXREF"),
            TRANTYPE=str(work / "TRANTYPE"), TRANCATG=str(work / "TRANCATG"),
            DATEPARM=str(work / "dateparm"), TRANREPT=str(work / "report"),
        )
        run([str(report_binary)], cwd=work, env=env)
        report = records(native_to_cp037((work / "report").read_bytes()), 133)
        compare_product("tranrept-report.bin", report, 133)
    result = {
        "tranrept_selected_records": len(selected),
        "tranrept_report_records": len(report),
        "dataset_sha256": {
            "AWS.M2.CARDDEMO.TRANSACT.DALY.G0001V00": framed_dataset(
                selected, selected_metadata["lrecl"], selected_metadata["version"],
                record_format=selected_metadata["recfm"]),
            "AWS.M2.CARDDEMO.TRANREPT.G0001V00": framed_dataset(
                report, report_metadata["lrecl"], report_metadata["version"],
                record_format=report_metadata["recfm"]),
        },
    }
    manifest = {
        "schema_version": "mainframe-env.carddemo-tranrept-reference@1",
        "scope": SCOPE,
        "input": {
            "path": INPUT, "sha256": sha256(data),
            "raw_record_sha256": sha256(data),
            "framed_dataset_sha256": framed_dataset(
                source_records, source_metadata["lrecl"], source_metadata["version"],
                record_format=source_metadata["recfm"]),
            "dataset": "AWS.M2.CARDDEMO.TRANSACT.BKUP.G0002V00",
            "record_count": len(source_records), "lrecl": source_metadata["lrecl"],
        },
        "date_parameters": {"start": "2022-01-01", "end": "2022-07-06",
                            "jcl_sha256": file_sha(jcl)},
        "program": {"corpus_commit": "59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e", "files": {
            name: file_sha(corpus / name) for name in CORPUS_FILES
        }},
        "tool": {"version": tool_version, "sha256": file_sha(cobc),
                 "indexed_backend": "BDB", "native_ebcdic": False,
                 "flags": ["-x", "-std=ibm", "-I", "app/cpy"]},
        "derivation": {
            "script_path": SCRIPT, "script_sha256": file_sha(ROOT / SCRIPT),
            "sort_path": SORT, "sort_sha256": file_sha(ROOT / SORT),
            "canonicaliser_path": SCRIPT, "canonicaliser_sha256": file_sha(ROOT / SCRIPT),
            "sort_policy": "ascending 16-byte card key; equal keys retain input order",
            "sort_origin": "written translation of TRANREPT.jcl DFSORT card",
            "dataset_metadata": {"input": source_metadata, "selected": selected_metadata,
                                 "report": report_metadata},
        },
        "results": result,
    }
    out.write_text(json.dumps(manifest, indent=2) + "\n")
    return manifest


def check(cobc, corpus):
    manifest = json.loads(MANIFEST.read_text())
    if manifest["schema_version"] != "mainframe-env.carddemo-tranrept-reference@1":
        raise ValueError("manifest identity differs")
    for path, digest in (
        (manifest["input"]["path"], manifest["input"]["sha256"]),
        (manifest["derivation"]["script_path"], manifest["derivation"]["script_sha256"]),
        (manifest["derivation"]["sort_path"], manifest["derivation"]["sort_sha256"]),
        (manifest["derivation"]["canonicaliser_path"],
         manifest["derivation"]["canonicaliser_sha256"]),
    ):
        if file_sha(ROOT / path) != digest:
            raise ValueError(f"digest differs: {path}")
    input_data = (ROOT / manifest["input"]["path"]).read_bytes()
    if sha256(input_data) != manifest["input"]["raw_record_sha256"]:
        raise ValueError("raw record digest differs")
    metadata = manifest["derivation"]["dataset_metadata"]["input"]
    if framed_dataset(records(input_data, 350), metadata["lrecl"], metadata["version"],
                      record_format=metadata["recfm"]) != manifest["input"]["framed_dataset_sha256"]:
        raise ValueError("input framed digest differs")
    if cobc:
        if not corpus:
            raise ValueError("--corpus is required with --cobc")
        with tempfile.TemporaryDirectory(prefix="carddemo-tranrept-check-") as temporary:
            actual = derive(cobc, corpus, Path(temporary) / "manifest.json")
        if actual != manifest:
            raise ValueError("GnuCOBOL re-derivation differs from checked-in manifest")
    print("carddemo TRANREPT reference: pass")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("command", nargs="?", choices=["derive"])
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--cobc")
    parser.add_argument("--corpus")
    parser.add_argument("--out", type=Path)
    args = parser.parse_args()
    if args.command == "derive":
        if not (args.cobc and args.corpus and args.out):
            parser.error("derive requires --cobc, --corpus and --out")
        derive(args.cobc, args.corpus, args.out)
    elif args.check:
        check(args.cobc, args.corpus or os.getenv("CARDDEMO_CORPUS_DIR") or
              (DEFAULT_CORPUS if DEFAULT_CORPUS.is_dir() else None))
    else:
        parser.error("specify derive or --check")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"carddemo TRANREPT reference: {error}", file=sys.stderr)
        sys.exit(1)
