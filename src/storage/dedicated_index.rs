use super::Index;

/// Basic index representing a single layer of storage.
pub struct DedicatedIndex {
    chunk_offsets: Vec<u64>,
}

impl DedicatedIndex {
    pub fn new(chunk_count: usize) -> Self {
        Self {
            chunk_offsets: vec![std::u64::MAX; chunk_count],
        }
    }
}

impl Index for DedicatedIndex {
    /// Record an chunk's offset for this layer into the index.
    fn replace(&mut self, chunk_number: usize, offset: u64) {
        if offset == std::u64::MAX {
            panic!("0xffff_ffff_ffff_ffff is a reserved value not permitted in indexes");
        }
        self.chunk_offsets[chunk_number] = offset;
    }
    /// Perform a lookup in the index for owned offsets.
    fn lookup(&self, chunk_number: usize) -> Option<u64> {
        match self.chunk_offsets[chunk_number] {
            std::u64::MAX => None,
            offset => Some(offset),
        }
    }
}
