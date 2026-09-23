use crate::service::CicsLimits;

impl Default for CicsLimits {
    fn default() -> Self {
        Self {
            max_sessions: 4096,
            max_runs: 4096,
            max_maps: 1024,
            max_programs: 4096,
            max_file_aliases: 1024,
            max_enqueue_models: 1024,
            max_fields: 512,
            max_screen_bytes: 4 * 1024 * 1024,
            max_queue_records: 65536,
            max_queue_bytes: 64 * 1024 * 1024,
            max_documents: 4096,
            max_document_templates: 1024,
            max_document_bytes: 64 * 1024 * 1024,
            max_document_symbols: 4096,
            max_document_bookmarks: 4096,
            max_transform_resources: 1024,
            max_transform_containers: 4096,
            max_transform_bytes: 64 * 1024 * 1024,
            max_spool_reports: 4096,
            max_spool_records: 65_536,
            max_spool_replays: 65_536,
            max_spool_bytes: 4 * 1024 * 1024,
            max_spool_outdescr_bytes: 4096,
        }
    }
}
