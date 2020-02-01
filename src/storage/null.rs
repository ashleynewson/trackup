use crate::chunk::Chunk;
use crate::storage::Storage;

pub struct NullStorage {
}

impl NullStorage {
    pub fn new() -> Self {
        Self {
        }
    }
}

impl Storage for NullStorage {
    fn read_chunk(&mut self) -> Result<Option<Chunk>,String> {
        Err(format!("Attempt to read from null storage"))
    }
    fn read_chunk_at(&mut self, _chunk_number: usize) -> Result<Option<Chunk>,String> {
        Err(format!("Attempt to read from null storage"))
    }
    fn write_chunk(&mut self, _chunk: &crate::chunk::Chunk) -> Result<(),String> {
        Ok(())
    }
    fn commit(&mut self) -> Result<(),String> {
        Ok(())
    }
}
