use crate::chunk::Chunk;
use super::{Checksums,ChecksumDiff};

pub struct NullChecksums {
    checksum_algorithm: String,
    checksum_size: usize,
    chunk_size: usize,
    chunk_count: usize,
}

impl NullChecksums {
    pub fn new(checksum_algorithm: &str, checksum_size: usize, chunk_size: usize, chunk_count: usize) -> Self {
        Self {
            checksum_algorithm: String::from(checksum_algorithm),
            checksum_size,
            chunk_size,
            chunk_count,
        }
    }
}

impl Checksums for NullChecksums {
    fn merge_chunk(&mut self, _chunk_number: usize, _checksum: &[u8]) {
        // null implementation
    }
    fn record_chunk(&mut self, _chunk: &Chunk) -> ChecksumDiff {
        ChecksumDiff::Replaced
    }
    fn commit(&self) -> Result<(),String> {
        Ok(())
    }
    fn get_checksum_algorithm(&self) -> &str {
        &self.checksum_algorithm
    }
    fn get_checksum_size(&self) -> usize {
        self.checksum_size
    }
    fn get_chunk_size(&self) -> usize {
        self.chunk_size
    }
    fn get_chunk_count(&self) -> usize {
        self.chunk_count
    }
}
