// File Format:
//
// File starts with a JSON object formatted header, with binary data
// following _immediately_ afterwards.
//
// sparse_checksum_file = header, data;
//
// header = json;
// (see FileHeader below for properties)
//
// data = checksum_run*;
//
// checksum runs have a skip_length indicating how many checksums
// forward to skip (as this is sparse data), and a run length,
// indicating how many checksums follow.
//
// checksum_run = skip_length, run_length, checksum*;
//
// skip_length = big_endian_u64;
// run_length  = big_endian_u64;
//
// The checksum is stored as a binary blob. Its size is dependent on
// the checksum used. The size in bytes (checksum_size) is specified
// in the file header. If there are no logical chunks (i.e. the
// represented device is zero bytes long), there are zero checksum_sum
// records. The file should not contain a checksum_run record with
// both skip and run equal to zero.
//
// The sum of all skips and runs must equal the chunk count, also
// specified in the header.
//
// No additional data is permitted at the end of the file.

use serde::{Serialize,Deserialize};
use digest::DynDigest;
use std::fs::File;
use std::io::{Read,Write,BufReader,BufWriter};
use std::path::{Path,PathBuf};
use std::marker::Send;
use crate::control::StoragePolicy;
use crate::chunk::Chunk;
use super::{Checksums,ChunkSource,ChecksumDiff};
use crate::quick_io::{read_skip_run,write_skip_run};

#[derive(Serialize,Deserialize)]
struct FileHeader {
    checksum_algorithm: String,
    checksum_size: usize,
    chunk_size: usize,
    chunk_count: usize,
    storage_policy: StoragePolicy,
    /// This should always be "SparseChecksums"
    format: String,
}

pub struct SparseChecksums {
    path: PathBuf,
    algorithm_name: String,
    checksum_size: usize,
    chunk_size: usize,
    chunk_count: usize,
    storage_policy: StoragePolicy,
    digest: Box<dyn DynDigest + Send>,
    sources: Vec<ChunkSource>,
    checksums: Vec<u8>,
}

impl SparseChecksums {
    pub fn new(path: &Path, algorithm_name: &str, checksum_size: usize, chunk_size: usize, chunk_count: usize, storage_policy: StoragePolicy) -> Result<Self,String> {
        let digest = crate::checksums::resolve_algorithm(algorithm_name, checksum_size)?;
        let sources = vec![ChunkSource::Absent; chunk_count];
        let checksums = vec![0; checksum_size*chunk_count];

        Ok(Self {
            path: path.to_path_buf(),
            algorithm_name: String::from(algorithm_name),
            checksum_size,
            chunk_size,
            chunk_count,
            storage_policy,
            digest,
            sources,
            checksums,
        })
    }

    fn checksum_slice(checksums: &[u8], checksum_size:usize, chunk_number: usize) -> &[u8] {
        &checksums[chunk_number*checksum_size..(chunk_number+1)*checksum_size]
    }
    fn mut_checksum_slice(checksums: &mut [u8], checksum_size:usize, chunk_number: usize) -> &mut [u8] {
        &mut checksums[chunk_number*checksum_size..(chunk_number+1)*checksum_size]
    }

    /// Load a SparseChecksums file into a Checksums implementation.
    pub fn load_file(path: &Path, checksums: &mut dyn Checksums) -> Result<(),String> {
        match File::open(path) {
            Ok(file) => {
                let mut bufreader = BufReader::new(file);
                let header_result = {
                    // Doing it this way means parsing stops as soon as the JSON object is read.
                    let mut deserializer = serde_json::Deserializer::from_reader(&mut bufreader);
                    FileHeader::deserialize(&mut deserializer)
                };
                match header_result {
                    Ok(header) => {
                        if header.format != "SparseChecksums" {
                            return Err(format!("Checksum file format is not SparseChecksums"));
                        } else if header.checksum_algorithm != checksums.get_checksum_algorithm() {
                            return Err(format!("Checksum file uses a different checksum algorithm"));
                        } else if header.checksum_size != checksums.get_checksum_size() {
                            return Err(format!("Checksum file uses a different checksum size"));
                        } else if header.chunk_size != checksums.get_chunk_size() {
                            return Err(format!("Checksum file uses a different chunk size"));
                        }
                        let chunk_count: usize = checksums.get_chunk_count();
                        if header.chunk_count != chunk_count {
                            eprintln!("Warning: checksum file has a different chunk count ({} instead of {})", header.chunk_count, chunk_count);
                        }

                        let mut checksum: Vec<u8> = vec![0; checksums.get_checksum_size()];
                        read_skip_run(
                            &mut bufreader,
                            chunk_count as u64,
                            |read, chunk_number| {
                                if let Err(e) = read.read_exact(&mut checksum) {
                                    return Err(format!("Error reading checksum file {}: {:?}", path.display(), e));
                                }
                                checksums.merge_chunk(chunk_number as usize, &checksum);
                                Ok(())
                            },
                            |_chunk_number| {
                                Ok(())
                            }
                        )?;
                        Ok(())
                    },
                    Err(e) => {
                        return Err(format!("Failed to read a valid config struture from file {}: {:?}", path.display(), e))
                    }
                }
            },
            Err(e) => {
                Err(format!("Failed to open checksum file {}: {:?}", path.display(), e))
            },
        }
    }
}

impl Checksums for SparseChecksums {
    fn get_checksum_algorithm(&self) -> &str {
        &self.algorithm_name
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
    fn merge_chunk(&mut self, chunk_number: usize, checksum: &[u8]) {
        if checksum.len() != self.checksum_size {
            panic!("Merge checksum has incorrect size");
        }
        Self::mut_checksum_slice(&mut self.checksums, self.checksum_size, chunk_number).copy_from_slice(checksum);
        self.sources[chunk_number] = ChunkSource::Historic;
    }
    fn record_chunk(&mut self, chunk: &Chunk) -> ChecksumDiff {
        self.digest.reset();
        self.digest.input(&chunk.data);
        let box_work_checksum = self.digest.result_reset();
        let work_checksum = &box_work_checksum[0..self.checksum_size];

        let chunk_number: usize = (chunk.offset / self.chunk_size as u64) as usize;
        let source = &mut self.sources[chunk_number];
        let destination_checksum = Self::mut_checksum_slice(&mut self.checksums, self.checksum_size, chunk_number);
        let source_was = *source;
        *source = ChunkSource::Current;
        if source_was == ChunkSource::Absent || work_checksum != destination_checksum {
            destination_checksum.copy_from_slice(work_checksum);
            ChecksumDiff::Replaced
        } else {
            match source_was {
                ChunkSource::Current => ChecksumDiff::Unchanged,
                ChunkSource::Historic => ChecksumDiff::Touched,
                ChunkSource::Absent => panic!("Unreachable"),
            }
        }
    }
    fn commit(&self) -> Result<(),String> {
        let save_historic = match self.storage_policy {
            StoragePolicy::Full => true,
            StoragePolicy::Incremental => false,
            StoragePolicy::Volatile => return Ok(()),
        };
        match File::create(&self.path) {
            Ok(file) => {
                let header = FileHeader {
                    chunk_size: self.chunk_size,
                    chunk_count: self.chunk_count,
                    checksum_size: self.checksum_size,
                    checksum_algorithm: self.algorithm_name.clone(),
                    storage_policy: self.storage_policy,
                    format: String::from("SparseChecksums"),
                };
                let mut bufwriter = BufWriter::new(file);
                match serde_json::to_writer::<&mut BufWriter<File>,FileHeader>(&mut bufwriter, &header) {
                    Ok(_) => {
                        let should_save = |source: ChunkSource| {
                            match source {
                                ChunkSource::Absent => false,
                                ChunkSource::Historic => save_historic,
                                ChunkSource::Current => true,
                            }
                        };

                        write_skip_run(
                            &mut bufwriter,
                            self.chunk_count as u64,
                            |write, chunk_number| {
                                if let Err(e) = write.write_all(&Self::checksum_slice(&self.checksums, self.checksum_size, chunk_number as usize)) {
                                    return Err(format!("Failed to write checksum data to file {}: {:?}", self.path.display(), e));
                                }
                                Ok(())
                            },
                            |chunk_number| {
                                Ok(should_save(self.sources[chunk_number as usize]))
                            }
                        )?;

                        Ok(())
                    },
                    Err(e) => {
                        return Err(format!("Failed to write checksum data to file {}: {:?}", self.path.display(), e))
                    }
                }
            },
            Err(e) => {
                Err(format!("Failed to open checksum file {}: {:?}", self.path.display(), e))
            },
        }
    }
}
