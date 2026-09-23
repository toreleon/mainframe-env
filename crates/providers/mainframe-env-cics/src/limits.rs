/// Resource limits for the protocol-neutral CICS authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CicsLimits {
    pub max_sessions: usize,
    pub max_runs: usize,
    pub max_maps: usize,
    pub max_programs: usize,
    pub max_file_aliases: usize,
    pub max_enqueue_models: usize,
    pub max_fields: usize,
    pub max_screen_bytes: usize,
    pub max_queue_records: usize,
    pub max_queue_bytes: usize,
    pub max_documents: usize,
    pub max_document_templates: usize,
    pub max_document_bytes: usize,
    pub max_document_symbols: usize,
    pub max_document_bookmarks: usize,
}

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
        }
    }
}
