"""Reject diagnostic formatting in durable effect, replay, and outbox encodings."""

from pathlib import Path
import re


ROOT = Path(__file__).resolve().parents[1]
TEST_MODULE = re.compile(r"(?m)^\s*#\[cfg\(test\)\]")
DEBUG_FORMAT = r"format!\s*\([^;]{0,512}\?[^;]{0,512}\)"
PERSISTED_DIAGNOSTIC_PATTERNS = (
    re.compile(rf"Sha256::digest\s*\(\s*{DEBUG_FORMAT}", re.DOTALL),
    re.compile(rf"(?:digest|hash|hasher)\.update\s*\(\s*{DEBUG_FORMAT}", re.DOTALL),
    re.compile(rf"payload\s*:\s*{DEBUG_FORMAT}", re.DOTALL),
    re.compile(
        rf"(?:request_digest|request_sha256)\s*[:=][^;]{{0,256}}{DEBUG_FORMAT}",
        re.DOTALL,
    ),
)


def production_source(path: Path) -> str:
    source = path.read_text()
    marker = TEST_MODULE.search(source)
    return source if marker is None else source[: marker.start()]


def reject_persisted_diagnostic_formatting(root: Path) -> None:
    violations = []
    for path in sorted((root / "crates").rglob("*.rs")):
        source = production_source(path)
        if "digest_saf_debug" in source or any(
            pattern.search(source) for pattern in PERSISTED_DIAGNOSTIC_PATTERNS
        ):
            violations.append(str(path.relative_to(root)))
    if violations:
        raise ValueError(
            "diagnostic formatting is not a durable identity or outbox encoding: "
            + ", ".join(violations)
        )


def require(source: str, fragments: tuple[str, ...], label: str) -> None:
    missing = [fragment for fragment in fragments if fragment not in source]
    if missing:
        raise ValueError(f"{label} is missing canonical encoding controls: {missing}")


def check(root: Path) -> None:
    reject_persisted_diagnostic_formatting(root)

    coordinator = production_source(
        root / "crates/kernel/mainframe-env-interpreter/src/coordinator.rs"
    )
    service = production_source(
        root / "crates/contracts/mainframe-env-host-api/src/service.rs"
    )
    canonical = production_source(
        root / "crates/contracts/mainframe-env-host-api/src/canonical.rs"
    )
    require(
        coordinator,
        (
            "canonical_request_digest(&effect.request)",
            "canonical_result_digest(&result.outcome)",
            'LIFECYCLE_OUTBOX_TOPIC: &str = "execution.lifecycle.v1"',
            "mainframe_env_execution_api::lifecycle_notification_payload(kind)",
            "payload: lifecycle_payload(&event.kind)",
        ),
        "coordinator",
    )
    require(
        production_source(
            root / "crates/contracts/mainframe-env-execution-api/src/lifecycle_notification.rs"
        ),
        (
            'b"mainframe-env.execution-lifecycle@1\\0"',
            "pub fn lifecycle_notification_payload(kind: &LifecycleEventKind)",
            "payload.extend_from_slice(DOMAIN)",
            "payload.extend_from_slice(&sequence.to_be_bytes())",
            "payload.extend_from_slice(&return_code.to_be_bytes())",
        ),
        "shared frozen lifecycle outbox encoder",
    )
    require(
        service,
        ("canonical_request_size(", "canonical_result_size("),
        "host limits",
    )
    require(
        canonical,
        (
            "canonical_db2_request_digest",
            "canonical_ims_request_digest",
            "canonical_mq_request_digest",
            'PROVIDER_REPLAY_DIGEST_FORMAT: &str = "mainframe-env.provider-replay-canonical@1"',
        ),
        "host canonical encoder",
    )
    if "format!" in canonical or "Debug" in canonical:
        raise ValueError("canonical production encoder must not use diagnostic formatting")

    providers = {
        "MQ": (
            root / "crates/providers/mainframe-env-mq/src/service.rs",
            "canonical_mq_request_digest(request)",
        ),
        "IMS": (
            root / "crates/providers/mainframe-env-ims/src/service.rs",
            "canonical_ims_request_digest(request)",
        ),
        "Db2": (
            root / "crates/providers/mainframe-env-db2/src/service.rs",
            "canonical_db2_request_digest(request)",
        ),
    }
    for name, (path, encoder) in providers.items():
        source = production_source(path)
        if name == "IMS":
            source += "\n" + production_source(path.parent / "service/execution.rs")
        require(
            source,
            (
                encoder,
                "request_digest_format",
                'serde(rename = "legacy-debug@0")',
                'serde(rename = "mainframe-env.provider-replay-canonical@1")',
                "reconcile_legacy_replay",
            ),
            f"{name} replay",
        )
    require(
        production_source(root / "crates/providers/mainframe-env-mq/src/message.rs"),
        ('b"mainframe-env.mq-message-id@1\\0"', "canonical_message_id"),
        "MQ generated message identity",
    )

    racf_command = production_source(
        root / "crates/providers/mainframe-env-racf/src/command_processor.rs"
    )
    racf_saf = production_source(root / "crates/providers/mainframe-env-racf/src/saf.rs")
    racf_database = production_source(
        root / "crates/providers/mainframe-env-racf/src/database.rs"
    )
    require(
        racf_command,
        (
            'b"mainframe-env.racf-command@1\\0"',
            "command_request_digest(&parsed)",
            "credential_operand(command",
            "RacfCommandCanonicalV1",
            "reconcile_legacy_replay",
        ),
        "RACF command replay",
    )
    if "Sha256::digest(input.as_bytes())" in racf_command:
        raise ValueError("RACF command persistence must not retain a raw credential oracle")
    require(
        racf_saf,
        (
            'b"mainframe-env.racroute-request@1\\0"',
            "racroute_request_tag",
            "RacrouteCanonicalV1",
            "reconcile_legacy_replay",
        ),
        "RACROUTE replay",
    )
    require(
        racf_database,
        (
            "scrub_legacy_replay_digests",
            'b"mainframe-env.racf-legacy-replay-scrub@1\\0"',
            '"REQUEST_DIGEST_FORMAT"',
        ),
        "RACF legacy migration",
    )

    durable = production_source(root / "crates/stores/mainframe-env-store/src/durable.rs")
    require(
        durable,
        ("request_canonical_v1", "result_canonical_v1", "digest_format"),
        "effect persistence",
    )

    samples = (
        'let digest = Sha256::digest(format!("{request:?}").as_bytes());',
        'payload: format!("{:?}", event.kind).into_bytes(),',
    )
    if not all(
        any(pattern.search(sample) for pattern in PERSISTED_DIAGNOSTIC_PATTERNS)
        for sample in samples
    ):
        raise ValueError("persisted-diagnostic guard patterns do not cover known regressions")


if __name__ == "__main__":
    check(ROOT)
    print("effect-encoding architecture guard: pass")
