use std::path::{PathBuf};
use serde::{Serialize,Deserialize};

#[derive(Clone,Serialize,Deserialize)]
pub struct Job {
    pub source: PathBuf,
    pub destination: PathBuf,
    // Currently only used for management interface
    pub chunk_size: usize,
    pub reuse_output: bool,
}
