use std::convert::TryInto;

pub struct Chunk {
    pub offset: u64,
    pub data: Vec<u8>,
}

impl Chunk {
    /// Validate offset and return the chunk number (offset / chunk_size)
    pub fn offset_chunk_number(offset: u64, chunk_size: usize, size: u64) -> usize {
        let chunk_number: usize = (offset / (chunk_size as u64)).try_into().expect(&format!("Chunk number from offset {} divided by chunk size {} too large for this platform", offset, chunk_size));
        if offset % (chunk_size as u64) != 0 {
            panic!("Offset {} is not a multiple of the chunk size {}", offset, chunk_size);
        }
        if offset >= size {
            panic!("Offset {} is not within size {}", offset, size);
        }
        chunk_number
    }
    /// Validate offset and return the size of the chunk
    pub fn offset_chunk_size(offset: u64, chunk_size: usize, size: u64) -> usize {
        // For checks only
        Self::offset_chunk_number(offset, chunk_size, size);
        if size - offset < chunk_size as u64 {
            (size - offset) as usize
        } else {
            chunk_size
        }
    }
    /// Validate chunk and return the chunk number (offset / chunk_size)
    pub fn chunk_number(&self, chunk_size: usize, size: u64) -> usize {
        let chunk_number = Self::offset_chunk_number(self.offset, chunk_size, size);
        if self.offset.checked_add(self.data.len() as u64).expect("Chunk offset+size overflow") > size {
            panic!("Offset {} + chunk size {} = {} is not within size {}", self.offset, self.data.len(), self.offset + self.data.len() as u64, size);
        }
        if self.data.len() > chunk_size {
            panic!("Chunk size {} exceeds common chunk size {}", self.data.len(), chunk_size);
        } else if self.data.len() < chunk_size && self.offset + self.data.len() as u64 != size {
            panic!("Chunk size {} is less than the common chunk size {}, and chunk does not appear to fit at the end", self.data.len(), chunk_size);
        }
        chunk_number
    }
}
