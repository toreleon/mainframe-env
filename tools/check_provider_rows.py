"""Guard bounded versioned row persistence for the stateful Db2, IMS, and MQ providers."""

from pathlib import Path
import re


ROOT = Path(__file__).resolve().parents[1]
TEST_MODULE = re.compile(r"(?m)^\s*#\[cfg\(test\)\]")


def production_source(path: Path) -> str:
    source = path.read_text()
    marker = TEST_MODULE.search(source)
    return source if marker is None else source[: marker.start()]


def require(source: str, fragments: tuple[str, ...], label: str) -> None:
    missing = [fragment for fragment in fragments if fragment not in source]
    if missing:
        raise ValueError(f"{label} row persistence is incomplete: {missing}")


def check(root: Path) -> None:
    targets = {
        "MQ": (
            root / "crates/providers/mainframe-env-mq/src/service.rs",
            (
                'ROW_STORE_SCHEMA: &str = "mainframe-env.mq-row-store@1"',
                'OBJECT_ROW_SCHEMA: &str = "mainframe-env.mq-object-row@1"',
                'QUEUE_NAMESPACE: &str = "mq-v1-queue"',
                'HANDLE_NAMESPACE: &str = "mq-v1-handle-index"',
                'PENDING_NAMESPACE: &str = "mq-v1-unit-of-work"',
                'REPLAY_NAMESPACE: &str = "mq-v1-replay"',
                "queues: BTreeMap<String, Arc<Queue>>",
            ),
        ),
        "IMS": (
            root / "crates/providers/mainframe-env-ims/src/service.rs",
            (
                'ROW_STORE_SCHEMA: &str = "mainframe-env.ims-row-store@1"',
                'OBJECT_ROW_SCHEMA: &str = "mainframe-env.ims-object-row@1"',
                'DATABASE_NAMESPACE: &str = "ims-v1-database"',
                'SESSION_NAMESPACE: &str = "ims-v1-session-index"',
                'CHECKPOINT_NAMESPACE: &str = "ims-v1-checkpoint"',
                'PENDING_NAMESPACE: &str = "ims-v1-unit-of-work"',
                'REPLAY_NAMESPACE: &str = "ims-v1-replay"',
                "databases: BTreeMap<String, Arc<DatabaseState>>",
            ),
        ),
        "Db2": (
            root / "crates/providers/mainframe-env-db2/src/service.rs",
            (
                'ROW_STORE_SCHEMA: &str = "mainframe-env.db2-row-store@1"',
                'OBJECT_ROW_SCHEMA: &str = "mainframe-env.db2-object-row@1"',
                'TABLE_NAMESPACE: &str = "db2-v1-table"',
                'SCHEMA_NAMESPACE: &str = "db2-v1-schema"',
                'INSTALLATION_NAMESPACE: &str = "db2-v1-installation"',
                'GENERATION_NAMESPACE: &str = "db2-v1-catalog-generation"',
                'PROVENANCE_NAMESPACE: &str = "db2-v1-table-provenance"',
                'LEGACY_SNAPSHOT_NAMESPACE: &str = "db2-v1-legacy-snapshot"',
                'PENDING_NAMESPACE: &str = "db2-v1-unit-of-work"',
                'CURSOR_NAMESPACE: &str = "db2-v1-cursor"',
                'CURSOR_DECLARATION_NAMESPACE: &str = "db2-v1-cursor-declaration"',
                'REPLAY_NAMESPACE: &str = "db2-v1-replay"',
                "tables: BTreeMap<String, Arc<Table>>",
                "fn db2_table_fences(",
                "base_tables: BTreeMap<String, Arc<Table>>",
            ),
        ),
    }
    for label, (path, specific) in targets.items():
        source = production_source(path)
        require(source, specific, label)
        persistence = source
        if label in ("MQ", "IMS"):
            require(
                source,
                (
                    "mod row_store;",
                    "use row_store::{",
                    "load_or_migrate(",
                    "commit_row_changes(",
                ),
                label,
            )
            persistence += "\n" + production_source(path.with_suffix("") / "row_store.rs")
        require(
            persistence,
            (
                "struct RowStoreManifest",
                "struct ObjectRow<T>",
                "fn load_or_migrate(",
                "fn ensure_row_namespaces_empty(",
                "fn row_changes(",
                "force_manifest_write: bool",
                "fn commit_row_changes(",
                ".mutate_provider_states_atomic(",
                "fn scoped_snapshot(&self) -> Self",
                "Arc::make_mut(",
            ),
            label,
        )
        forbidden = (
            "serde_json::to_vec(&state)",
            "state.tables.clone()",
            "state.databases.clone()",
            "payload: serde_json::to_vec(&next)",
            "durable.state.clone()",
        )
        present = [fragment for fragment in forbidden if fragment in persistence]
        if present:
            raise ValueError(f"{label} regressed to whole-state persistence: {present}")


if __name__ == "__main__":
    check(ROOT)
    print("provider-row persistence architecture guard: pass")
