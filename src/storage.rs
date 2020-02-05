use crate::chunk::Chunk;

pub mod raw;
pub mod sparse;
pub mod null;
pub mod layered;
pub mod dedicated_index;
pub mod shared_index;

pub trait Storage {
    /// Gets the next chunk stored by the storage. Note that there is
    /// no guarantee as to the ordering or presence of chunks provided
    /// by this trait. Such details may depend on the implementation.
    /// If there are no more chunks, Ok(None) is returned.
    fn read_chunk(&mut self) -> Result<Option<Chunk>,String>;
    /// Get the chunk with the given chunk number (if it exists)
    fn read_chunk_at(&mut self, chunk_number: usize) -> Result<Option<Chunk>,String>;
    /// Write a chunk. Chunk writes do not need to be ordered or
    /// complete.
    fn write_chunk(&mut self, chunk: &Chunk) -> Result<(),String>;
    /// Finish backup, writing any remaining data to file. This should
    /// only be called once. Calling multiple times may be considered
    /// a panicking error.
    fn commit(&mut self) -> Result<(),String>;
}


pub struct StorageProperties {
    /// Logical size of the data represented by the store.
    pub size: u64,
    /// Whether specific chunks can be looked up with random access.
    ///
    /// Note that this does not necessarily mean that the store has an
    /// explicit index. For example, this is set true for raw backups,
    /// where chunk positions are predictable.
    pub indexed: bool,
}


pub trait Index {
    fn replace(&mut self, chunk_number: usize, offset: u64);
    fn lookup(&self, chunk_number: usize) -> Option<u64>;
}
