"""Guard PostgreSQL quota fencing and durable artifact-profile composition."""

from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def require(path: Path, fragments: tuple[str, ...]) -> str:
    source = path.read_text()
    missing = [fragment for fragment in fragments if fragment not in source]
    if missing:
        raise ValueError(f"{path.relative_to(ROOT)} is missing: {missing}")
    return source


def check(root: Path) -> None:
    require(
        root / "crates/stores/mainframe-env-store/src/lib.rs",
        (
            'SQLITE_MIGRATION_HEAD: &str = "0002-retention-lifecycle"',
            'POSTGRES_MIGRATION_HEAD: &str = "0003-executable-artifact-metadata"',
        ),
    )
    postgres = require(
        root / "crates/stores/mainframe-env-store/src/postgres.rs",
        (
            'PROVIDER_STATE_QUOTA: &str = "provider-state"',
            "initialize_quota(",
            "adjust_quota(",
            "finish_transaction(",
            "SELECT max_rows,used_rows FROM store_quota",
        ),
    )
    if postgres.count('"SELECT COUNT(*) FROM provider_state"') != 1:
        raise ValueError("PostgreSQL provider-state counting escaped startup reconciliation")

    artifact_store = require(
        root / "crates/stores/mainframe-env-store/src/postgres_artifact.rs",
        (
            "pub struct PostgresArtifactStore",
            "ARTIFACT_OBJECT_QUOTA",
            "INSERT INTO artifact_object",
            "ON CONFLICT DO NOTHING",
            "validation::artifact(&record)",
        ),
    )
    if postgres.count('"../migrations/postgres/0003-executable-artifact-metadata.sql"') != 1:
        raise ValueError("PostgreSQL state startup escaped artifact migration head")
    if artifact_store.count('"../migrations/postgres/0003-executable-artifact-metadata.sql"') != 1:
        raise ValueError("PostgreSQL artifact startup escaped artifact migration head")
    require(
        root / "crates/stores/mainframe-env-store/migrations/postgres/0001-durable-state.sql",
        ("CREATE TABLE IF NOT EXISTS store_quota", "CREATE TABLE IF NOT EXISTS artifact_object"),
    )
    require(
        root
        / "crates/stores/mainframe-env-store/migrations/postgres/0003-executable-artifact-metadata.sql",
        (
            "LOCK TABLE artifact_object IN ACCESS EXCLUSIVE MODE",
            "ADD COLUMN IF NOT EXISTS executable_metadata BYTEA",
            "artifact_object_schema_version_v2_check",
            "CHECK(schema_version IN (1, 2))",
        ),
    )
    local = require(
        root / "crates/stores/mainframe-env-store/src/local_artifact.rs",
        ("std::fs::hard_link(&temporary, &path)", "fn sync_directory(", ".sync_all()"),
    )
    if "std::fs::rename(&temporary, &path)" in local:
        raise ValueError("local artifact publication regressed to replace-capable rename")

    require(
        root / "crates/apps/mainframe-env-server/src/config.rs",
        (
            "pub enum ArtifactProfile",
            "ArtifactProfile::Shared",
            '"MAINFRAME_ENV_ARTIFACT_STORE"',
        ),
    )
    require(
        root / "crates/apps/mainframe-env-server/src/main.rs",
        ("PostgresArtifactStore::open(", "open_with_package_trust_and_artifact_store("),
    )


if __name__ == "__main__":
    check(ROOT)
    print("durable storage profile architecture guard: pass")
