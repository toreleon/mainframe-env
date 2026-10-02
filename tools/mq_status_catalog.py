"""Bounded, source-reviewed MQ call-return status authority (zero execution credit)."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import re
import sys

import generate_mq_mqi_registry as registry

ROOT = registry.ROOT
CATALOG = Path("conformance/0.15/mq/completion-reason-catalog.json")
SCHEMA = Path("conformance/0.15/schemas/mq-completion-reason-catalog.schema.json")
OUTPUT = Path("crates/contracts/mainframe-env-host-api/src/mq_status/generated")
COMPLETIONS = {"MQCC_OK": "Ok", "MQCC_WARNING": "Warning", "MQCC_FAILED": "Failed"}
# Reviewed immutable source-projection fixtures, not another numeric authority.
WIRE_PROJECTION_SHA256 = "1b67e82e0c85139fd0bf2ce58bb851f4eca5184bebb081ed7b1ccadee71da5cd"
CALL_RETURN_SHA256 = "02bd30d078388918b002fe4e8272bf8c4b3403b1d7e383d580cc1496dd64edaa"
SUPPLEMENT_MANIFEST = Path("conformance/0.15/manifests/mq-programming-supplements-topics.json")
ISSUES = {"admitted", "pending-number", "pending-numeric-conflict", "pending-symbol-conflict", "pending-symbol-spelling"}
CALL_VARIANTS = (
    "Back", "Begin", "BufferToHandle", "Callback", "CallbackFunction", "Close",
    "Commit", "Connect", "ConnectExtended", "CreateMessageHandle", "Control",
    "Disconnect", "DeleteMessageHandle", "DeleteProperty", "Get", "Inquire",
    "InquireProperty", "HandleToBuffer", "Open", "Put", "PutOne", "Set",
    "SetProperty", "Stat", "Subscribe", "SubscriptionRequest",
)


def digest(value: object) -> str:
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def read_json(path: Path) -> dict:
    def unique(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError(f"duplicate JSON field: {key}")
            result[key] = value
        return result
    if path.stat().st_size > 2 * 1024 * 1024:
        raise ValueError("MQ status input byte bound")
    result = json.loads(path.read_text(), object_pairs_hook=unique)
    if not isinstance(result, dict):
        raise ValueError("MQ status object required")
    return result


def fields(value: dict, expected: set[str], label: str) -> None:
    if not isinstance(value, dict) or set(value) != expected:
        raise ValueError(f"MQ status {label} fields differ")


def projection(call: dict) -> dict:
    return {key: call[key] for key in ("return_kind", "reason_section", "context_notes", "pairs")}


def load_wire(catalog: dict, root: Path) -> None:
    wire = catalog["completion_wire_mapping"]
    fields(wire, {"review", "review_work_package", "scope_id", "baseline_id", "topic_manifest",
        "canonical_call_return_sha256", "topic_path", "topic_sha256", "classes", "excluded_unknown",
        "corroboration", "projection_sha256"}, "completion wire projection")
    projected = {key: value for key, value in wire.items() if key != "projection_sha256"}
    if wire["projection_sha256"] != digest(projected) or digest(projected) != WIRE_PROJECTION_SHA256:
        raise ValueError("MQ reviewed completion wire projection differs")
    # Reconstruct the unchanged @1 source artifact. Its digest is already part
    # of canonical status bytes; supplemental review must not reframe it.
    original = {key: value for key, value in catalog.items() if key != "completion_wire_mapping"}
    original["schema_version"] = "mainframe-env.mq-completion-reason-catalog@1"
    original["completion_numeric_mapping"] = "pending-not-in-call-pages"
    legacy = hashlib.sha256((json.dumps(original, indent=2) + "\n").encode()).hexdigest()
    if legacy != CALL_RETURN_SHA256 or wire["canonical_call_return_sha256"] != legacy:
        raise ValueError("MQ immutable call-return catalog identity differs")
    sys.path.insert(0, str(ROOT / "conformance/tools"))
    import ibm_docs
    manifest = read_json(root / SUPPLEMENT_MANIFEST)
    if wire["topic_manifest"] != {"path": SUPPLEMENT_MANIFEST.as_posix(),
            "sha256": registry._sha256(root / SUPPLEMENT_MANIFEST),
            "topic_manifest_digest": manifest["topic_manifest_digest"]}:
        raise ValueError("MQ supplemental manifest binding differs")
    topics, _, _ = ibm_docs.validate_manifest(manifest, root / SUPPLEMENT_MANIFEST,
        target_version="0.15.0", baseline=wire["baseline_id"], subsystem="mq")
    index = read_json(root / SUPPLEMENT_MANIFEST.parent / "index.json")
    entries = [entry for entry in index["manifests"] if entry["scope_id"] == wire["scope_id"]]
    expected = {"scope_id": wire["scope_id"], "subsystem": "mq", "baseline_id": wire["baseline_id"],
        "manifest": SUPPLEMENT_MANIFEST.as_posix(), "manifest_sha256": "sha256:" + wire["topic_manifest"]["sha256"],
        "topic_count": len(topics), "topic_manifest_sha256": "sha256:" + manifest["topic_manifest_digest"],
        "semantic_authority": False, "coverage_credit": 0}
    if (index["schema_version"] != "mainframe-env.topic-manifest-registry@1"
            or index["target_version"] != "0.15.0" or index["semantic_authority"] is not False
            or type(index["coverage_credit"]) is not int or index["coverage_credit"] != 0
            or entries != [expected] or entries[0]["semantic_authority"] is not False
            or type(entries[0]["coverage_credit"]) is not int):
        raise ValueError("MQ supplemental registry binding differs")
    by_topic = {topic["topic_path"]: topic for topic in topics}
    for source in [wire, wire["corroboration"][1]]:
        if by_topic[source["topic_path"]]["sha256"] != source["topic_sha256"]:
            raise ValueError("MQ supplemental topic binding differs")
    call = next(call for call in catalog["calls"] if call["official_row"] == wire["corroboration"][0]["official_row"])
    if any(call[key] != wire["corroboration"][0][key] for key in ("topic_path", "topic_sha256")):
        raise ValueError("MQ completion call context binding differs")


def wire_source_lines(catalog: dict, supplemental_cache: Path, call_cache: Path) -> dict[str, list[str]]:
    """Read only the selected wire topic/context pins through shared infrastructure."""
    sys.path.insert(0, str(ROOT / "conformance/tools"))
    import ibm_docs
    result = {}
    for scope, cache, sources in [
        (catalog["completion_wire_mapping"]["scope_id"], supplemental_cache,
         [catalog["completion_wire_mapping"], catalog["completion_wire_mapping"]["corroboration"][1]]),
        (registry.BASELINE, call_cache, [catalog["completion_wire_mapping"]["corroboration"][0]])]:
        pins, tocs = ibm_docs.select(*ibm_docs.load_pins(), scope, None)
        by_topic = {pin.topic: pin for pin in pins}
        for toc in tocs:
            if hashlib.sha256(ibm_docs.read_bounded(cache / toc.key)).hexdigest() != toc.sha256:
                raise ValueError("MQ wire source TOC mismatch")
        for source in sources:
            pin = by_topic[source["topic_path"]]
            raw = ibm_docs.read_bounded(cache / pin.key)
            if hashlib.sha256(raw).hexdigest() != source["topic_sha256"] or len(raw) != pin.size:
                raise ValueError("MQ wire source hash/size mismatch")
            result[pin.topic] = ibm_docs.plain_text(raw)
    return result


def load(root: Path = ROOT) -> dict:
    catalog = read_json(root / CATALOG)
    fields(catalog, {"schema_version", "target_version", "work_package", "baseline_id",
        "topic_manifest", "source_call_list", "structure_status_catalog", "unique_call_count",
        "source_position_count", "call_return_count", "callback_notification_count", "pair_count",
        "source_occurrence_count", "admitted_pair_count", "pending_pair_count", "completion_symbols",
        "completion_numeric_mapping", "completion_wire_mapping", "behavioral_coverage_credit", "licensed_execution_credit", "calls"}, "catalog")
    constants = {"schema_version": "mainframe-env.mq-completion-reason-catalog@2",
        "target_version": "0.15.0", "work_package": "MQ-1501.completion-reason-catalog",
        "baseline_id": registry.BASELINE, "unique_call_count": 26, "source_position_count": 27,
        "call_return_count": 25, "callback_notification_count": 1,
        "pair_count": 1030, "source_occurrence_count": 1031,
        "admitted_pair_count": 1020, "pending_pair_count": 10,
        "completion_symbols": list(COMPLETIONS), "completion_numeric_mapping": "reviewed-supplemental",
        "behavioral_coverage_credit": 0, "licensed_execution_credit": 0}
    for key, value in constants.items():
        if catalog[key] != value or (isinstance(value, int) and type(catalog[key]) is not int):
            raise ValueError(f"MQ status {key} differs")
    load_wire(catalog, root)
    for key, path in [("topic_manifest", registry.TOPIC_MANIFEST_PATH),
                      ("source_call_list", registry.SOURCE_LIST_PATH),
                      ("structure_status_catalog", registry.CONTRACT_CATALOG_PATH)]:
        if catalog[key] != {"path": path.as_posix(), "sha256": registry._sha256(root / path)}:
            raise ValueError(f"MQ status {key} binding differs")
    identities = registry.load(root)
    registry.load_contract(root)
    if not isinstance(catalog["calls"], list) or len(catalog["calls"]) != 26:
        raise ValueError("MQ status missing/duplicate call membership")
    admitted = pending = occurrences = pairs = 0
    numeric_symbols = {}
    for call, identity in zip(catalog["calls"], identities):
        fields(call, {"official_row", "label", "topic_path", "topic_sha256", "source_positions",
            "return_kind", "reason_section", "context_notes", "pairs", "reviewed_projection_sha256"}, "call")
        for key in identity:
            if call[key] != identity[key]:
                raise ValueError(f"MQ status foreign call/pin/position: {identity['label']}")
        if call["reviewed_projection_sha256"] != digest(projection(call)):
            raise ValueError(f"MQ status reviewed projection differs: {call['label']}")
        callback = call["label"] == "MQCB_FUNCTION"
        if call["return_kind"] != ("callback-notification-no-call-return" if callback else "call-return"):
            raise ValueError("MQ callback return non-applicability differs")
        section = call["reason_section"]
        if callback:
            if section is not None or call["pairs"]:
                raise ValueError("MQ callback cannot have return pairs")
        elif not isinstance(section, dict) or set(section) != {"first_line", "last_line"} or any(type(n) is not int for n in section.values()) or not 1 <= section["first_line"] < section["last_line"] <= 2000:
            raise ValueError("MQ status return section differs")
        if not isinstance(call["context_notes"], list) or len(call["context_notes"]) > 16:
            raise ValueError("MQ context notes bound")
        seen_notes = set()
        for note in call["context_notes"]:
            fields(note, {"kind", "first_line", "last_line", "fragment_sha256"}, "context note")
            if not isinstance(note["kind"], str) or not re.fullmatch(r"[a-z][a-z0-9-]{1,80}", note["kind"]) or any(type(note[k]) is not int for k in ("first_line", "last_line")) or not 1 <= note["first_line"] <= note["last_line"] <= 2000 or not isinstance(note["fragment_sha256"], str) or not re.fullmatch(r"[0-9a-f]{64}", note["fragment_sha256"]):
                raise ValueError("MQ context locator differs")
            key = (note["kind"], note["first_line"], note["last_line"])
            if key in seen_notes:
                raise ValueError("duplicate MQ context note")
            seen_notes.add(key)
        if not isinstance(call["pairs"], list) or len(call["pairs"]) > 256 or (not callback and not call["pairs"]):
            raise ValueError("MQ status pair bound")
        seen = set()
        prior_line = 0
        for pair in call["pairs"]:
            fields(pair, {"completion_symbol", "reason_symbol", "declared_decimal", "declared_hex",
                "review", "source_locations"}, "pair")
            comp, reason = pair["completion_symbol"], pair["reason_symbol"]
            symbol_pattern = r"MQRC_[A-Z0-9_ ]{1,80}" if pair["review"] == "pending-symbol-spelling" else r"MQRC_[A-Z0-9_]{1,80}"
            if not isinstance(comp, str) or comp not in COMPLETIONS or not isinstance(reason, str) or not re.fullmatch(symbol_pattern, reason) or (pair["review"] == "pending-symbol-spelling" and " " not in reason):
                raise ValueError("MQ status symbolic identity differs")
            key = (comp, reason)
            if key in seen:
                raise ValueError("duplicate MQ status pair")
            seen.add(key)
            dec, hexa = pair["declared_decimal"], pair["declared_hex"]
            if dec is not None and (type(dec) is not int or not 0 <= dec <= 2**31-1):
                raise ValueError("MQ reason decimal bound")
            if hexa is not None and (not isinstance(hexa, str) or not re.fullmatch(r"[0-9A-F]{1,8}", hexa)):
                raise ValueError("MQ reason hex bound")
            review = pair["review"]
            if not isinstance(review, str) or review not in ISSUES:
                raise ValueError("MQ status review differs")
            if review == "pending-number":
                if dec is not None or hexa is not None:
                    raise ValueError("MQ unresolved number must stay pending")
            elif dec is None or hexa is None:
                raise ValueError("MQ reviewed numeric identity missing")
            elif review == "pending-numeric-conflict":
                if dec == int(hexa, 16):
                    raise ValueError("MQ numeric conflict disposition differs")
            elif dec != int(hexa, 16):
                raise ValueError("MQ decimal/hex identity conflict")
            if not isinstance(pair["source_locations"], list) or not 1 <= len(pair["source_locations"]) <= 4:
                raise ValueError("MQ source occurrence bound")
            previous = 0
            for loc in pair["source_locations"]:
                fields(loc, {"completion_line", "reason_line", "number_line"}, "location")
                a,b,c = loc["completion_line"],loc["reason_line"],loc["number_line"]
                if any(type(n) is not int for n in (a,b,c)) or not section["first_line"] < a < b < c <= section["last_line"] or c != b+1 or b <= previous:
                    raise ValueError("MQ return-table locator differs")
                previous = b
            line = pair["source_locations"][0]["reason_line"]
            if line <= prior_line:
                raise ValueError("MQ status source order differs")
            prior_line = line
            pairs += 1
            occurrences += len(pair["source_locations"])
            if review == "admitted":
                admitted += 1
                previous = numeric_symbols.setdefault(reason, dec)
                if previous != dec:
                    raise ValueError("MQ numeric/symbol identity conflict")
            else:
                pending += 1
    for key, count in {"pair_count": pairs, "source_occurrence_count": occurrences,
                       "admitted_pair_count": admitted, "pending_pair_count": pending}.items():
        if catalog[key] != count:
            raise ValueError(f"MQ status source count differs: {key}")
    return catalog
