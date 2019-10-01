use std::path::{Path,PathBuf};
use std::fs::File;
use std::io::{Write,Seek,SeekFrom};
use std::os::unix::io::IntoRawFd;
use crate::chunk::Chunk;

pub struct BackupFile {
    path: PathBuf,
    file: File,
    // fd: RawFd,
}

impl BackupFile {
    pub fn create_file(path: &Path, size: u64) -> Result<Self, String> {
        let file = match File::create(path) {
            Ok(x) => x,
            Err(_) => {
                return Err(format!("Could not create backup file"));
            },
        };

        if let Err(e) = nix::fcntl::fallocate(
            file.try_clone().unwrap().into_raw_fd(),
            nix::fcntl::FallocateFlags::FALLOC_FL_ZERO_RANGE,
            0,
            size as i64,
        ) {
            return Err(format!("Could not pre-allocate backup file: {:?}", e));
        }

        Ok(Self{
            path: path.to_path_buf(),
            file,
        })
    }

    pub fn use_file(path: &Path, size: u64) -> Result<Self, String> {
        let mut file = match std::fs::OpenOptions::new().write(true).open(path) {
            Ok(x) => x,
            Err(_) => {
                return Err(format!("Could not open (reuse) backup file"));
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
        })
    }

    pub fn write_chunk(&mut self, chunk: Chunk) {
        self.file.seek(SeekFrom::Start(chunk.offset)).expect("Backup seek failed");
        self.file.write_all(&chunk.data).expect("Write to backup failed");
    }

    pub fn get_path(&self) -> &Path {
        self.path.as_path()
    }
}

impl Drop for BackupFile {
    fn drop(&mut self) {
        self.file.sync_all().expect("Failed to sync all data before closing");
    }
}
