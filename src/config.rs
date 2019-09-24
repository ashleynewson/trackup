use std::path::Path;
use std::time::Duration;
use job::Job;

pub struct Config<'c> {
    pub jobs: &'c Vec<Job>,
    pub chunk_size: usize,
    pub tracing_path: &'c Path,
    pub sys_path: &'c Path,
    pub trace_buffer_size: usize,
    pub progress_update_period: Duration,
    pub exclusive_progress_updates: bool,
    pub max_diagram_size: usize,
    pub reuse_output: bool,
}
