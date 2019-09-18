use std::path::Path;
use std::time::Duration;

pub struct Config<'c> {
    pub source: &'c Path,
    pub destination: &'c Path,
    pub chunk_size: usize,
    pub tracing_path: &'c Path,
    pub sys_path: &'c Path,
    pub trace_buffer_size: usize,
    pub progress_update_period: Duration,
    pub exclusive_progress_updates: bool,
    pub max_diagram_size: usize,
}
