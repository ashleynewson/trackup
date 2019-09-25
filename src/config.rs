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
    pub diagram_cells: &'c [&'c str; 4],
    pub diagram_cells_reset: &'c str,
}

pub const PLAIN_DIAGRAM_CELLS: [&str; 4] = ["#", "*", ".", "o"];
pub const COLOR_DIAGRAM_CELLS: [&str; 4] = ["\x1b[42m#", "\x1b[41m*", "\x1b[100m.", "\x1b[44mo"];
