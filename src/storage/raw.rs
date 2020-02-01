// File format:
//
// A byte-for-byte copy of a block device, with no additional data of
// any kind.

use std::path::{Path,PathBuf};
use std::fs::File;
use std::io::{Read,Write,Seek,SeekFrom};
use crate::chunk::Chunk;
use super::Storage;

pub struct RawStorage {
    path: PathBuf,
    file: File,
    size: u64,
    chunk_size: usize,
    writeable: bool,
}

impl RawStorage {
    pub fn create_file(path: &Path, size: u64, chunk_size: usize) -> Result<Self, String> {
        let file = match File::create(path) {
            Ok(x) => x,
            Err(_) => {
                return Err(format!("Could not create full backup file"));
            },
        };

        if let Err(e) = file.set_len(size) {
            eprintln!("Warning: could not pre-allocate full backup file: {:?}", e);
        }

        Ok(Self{
            path: path.to_path_buf(),
            file,
            size,
            chunk_size,
            writeable: true,
        })
    }

    pub fn use_file(path: &Path, size: u64, chunk_size: usize, writeable: bool) -> Result<Self, String> {
        let mut file = match std::fs::OpenOptions::new().write(writeable).open(path) {
            Ok(x) => x,
            Err(_) => {
                return Err(format!("Could not open (reuse) full backup file"));
            },
        };

        let existing_size = file.seek(SeekFrom::End(0)).expect("Could not determine file size");
        file.seek(SeekFrom::Start(0)).expect("Could not seek back to beginning of backup file");

        if existing_size < size {
            return Err(format!("Existing backup file is not large enough"));
        }

        Ok(Self {
            path: path.to_path_buf(),
            file,
            size,
            chunk_size,
            writeable,
        })
    }

    pub fn get_path(&self) -> &Path {
        self.path.as_path()
    }
}

impl Storage for RawStorage {
    fn read_chunk(&mut self) -> Result<Option<Chunk>,String> {
        let offset: u64 = self.file.seek(SeekFrom::Current(0)).expect("Backup seek failed");
        if offset == self.size {
            return Ok(None);
        }
        // For checks
        let this_chunk_size = Chunk::offset_chunk_size(offset, self.chunk_size, self.size);
        let mut data = vec![0; this_chunk_size];
        self.file.read_exact(&mut data).expect("Write to backup failed");
        let chunk = Chunk {
            offset,
            data,
        };
        Ok(Some(chunk))
    }
    fn read_chunk_at(&mut self, chunk_number: usize) -> Result<Option<Chunk>,String> {
        let offset: u64 = (self.chunk_size as u64).checked_mul(chunk_number as u64).unwrap();
        if offset >= self.size {
            panic!("Chunk number {} has offset {}, exceeding size {}", chunk_number, offset, self.size);
        }
        self.file.seek(SeekFrom::Start(offset)).expect("Backup seek failed");
        let chunk = self.read_chunk()?.unwrap();
        Ok(Some(chunk))
    }
    fn write_chunk(&mut self, chunk: &Chunk) -> Result<(),String> {
        if !self.writeable {
            panic!("Backup is not writeable");
        }
        // For checks
        chunk.chunk_number(self.chunk_size, self.size);
        self.file.seek(SeekFrom::Start(chunk.offset)).expect("Backup seek failed");
        self.file.write_all(&chunk.data).expect("Write to backup failed");
        Ok(())
    }
    fn commit(&mut self) -> Result<(),String> {
        if let Err(e) = self.file.sync_all() {
            return Err(format!("Failed to sync all data before closing: {:?}", e));
        }
        Ok(())
    }
}
