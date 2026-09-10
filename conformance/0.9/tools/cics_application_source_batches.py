#!/usr/bin/env python3
"""Shared file identities for bounded CIC-901 application-source batches."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class SourceBatch:
    """Paths and identity bounds; extraction and verification stay independent."""

    name: str
    ordinal_start: int
    ordinal_end: int
    supplements_path: Path | None = None

    @property
    def row_count(self) -> int:
        return self.ordinal_end - self.ordinal_start + 1

    @property
    def map_path(self) -> Path:
        return Path(f"conformance/0.9/cics/application-api-sources-{self.name}-map.json")

    @property
    def corpus_path(self) -> Path:
        return Path(
            f"conformance/0.9/cics/application-api-sources-{self.name}-corpus.json"
        )

    @property
    def manifest_path(self) -> Path:
        return Path(
            "conformance/0.9/manifests/"
            f"cics-application-api-sources-{self.name}-topics.json"
        )

    @property
    def plan_path(self) -> Path:
        return Path(
            f"conformance/0.9/cics/application-api-sources-{self.name}-extraction.json"
        )

    @property
    def projection_path(self) -> Path:
        return Path(
            "conformance/0.9/generated/"
            f"cics-application-api-sources-{self.name}-candidates.json"
        )

    @property
    def review_path(self) -> Path:
        return Path(
            f"conformance/0.9/cics/application-api-sources-{self.name}-review.json"
        )

    @property
    def browser_receipt_path(self) -> Path | None:
        if self.name != "a":
            return None
        return Path(
            "conformance/0.9/cics/"
            "application-api-sources-a-browser-verification.json"
        )

    @property
    def cache_scope(self) -> str:
        return f"cics-application-api-sources-{self.name}"

    @property
    def project_work_package(self) -> str:
        return f"CIC-901.sources-{self.name}-project"

    @property
    def review_work_package(self) -> str:
        return f"CIC-901.sources-{self.name}-review"

    @property
    def candidate_id_prefix(self) -> str:
        return f"cics-{self.name}"

    def contains_row(self, official_row: str) -> bool:
        try:
            ordinal = int(official_row.rsplit(":", 1)[1])
        except (IndexError, ValueError):
            return False
        return self.ordinal_start <= ordinal <= self.ordinal_end


BATCHES = {
    "a": SourceBatch(
        "a",
        1,
        88,
        Path("conformance/0.9/cics/application-api-sources-a-supplements.json"),
    ),
    "b": SourceBatch("b", 89, 176),
    "c": SourceBatch("c", 177, 263),
}


def source_batch(value: str | SourceBatch = "a") -> SourceBatch:
    if isinstance(value, SourceBatch):
        return value
    try:
        return BATCHES[value]
    except KeyError as error:
        raise ValueError(f"unknown CICS application source batch: {value}") from error


def source_batch_for_row(official_row: str) -> SourceBatch:
    for batch in BATCHES.values():
        if batch.contains_row(official_row):
            return batch
    raise ValueError(f"official row is outside CIC-901 source batches: {official_row}")
