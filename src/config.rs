use std::path::PathBuf;
use std::time::Duration;
use serde::{Serialize,Deserialize};

#[derive(Clone,Serialize,Deserialize)]
pub struct Config {
    pub tracing_path: PathBuf,
    pub sys_path: PathBuf,
    pub trace_buffer_size: usize,
    pub progress_update_period: Duration,
    pub exclusive_progress_updates: bool,
    pub max_diagram_size: usize,
    pub diagram_cells: Vec<String>,
    pub diagram_cells_reset: String,
}

pub const PLAIN_DIAGRAM_CELLS: [&str; 4] = ["#", "*", ".", "o"];
pub const COLOR_DIAGRAM_CELLS: [&str; 4] = ["\x1b[42m#", "\x1b[41m*", "\x1b[100m.", "\x1b[44mo"];
