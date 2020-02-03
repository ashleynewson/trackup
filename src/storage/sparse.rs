// File Format:
//
// File starts with a JSON object formatted header, with binary data
// following _immediately_ afterwards.
//
// sparse_storage_file = header, chunk_data, index?;
//
// header = json;
// (see FileHeader below for properties)
//
// chunk_data = numbered_chunk*, end_of_chunk_data_marker;
//
// numbered_chunk_count = big_endian_u64;
//
// numbered_chunk = chunk_number, chunk;
// chunk_number = big_endian_u64;
// end_of_chunk_data_marker = 0xffff_ffff_ffff_ffff;
//
// chunk is a blob of raw chunk data. Its size is equal to the chunk
// size as specified in the header. The stored form is _always_ equal
// to this chunk size, even if it is a logically-last and undersized
// chunk.
//
// numbered_chunks with the same chunk_number _may_ be repeated within
// the file. When there are multiple numbered_chunks with the same
// chunk number, later entries in the file take priority. Valid chunk
// numbers range from 0 to 2^64-2 (see below).
//
// The end of raw chunk data is signified by an
// end_of_chunk_data_marker, which is simply the number
// 0xffff_ffff_ffff_ffff (i.e. 2^64-1). This marker allows the file to
// be parsed sequentially, without first checking for a possible
// index.
//
// An index is optional. If present, it positioned at the end of a
// file, and the header will state that it is included. The index
// provides a means for looking up the location of a chunk in the file
// when given its chunk_number. The index's size in bytes is provided
// at the end in order allow its location to be determined without a
// full read of the file data (as its position is predictable). This
// size does not include itself.
//
// index = location_run*, index_size;
//
// The index itself is stored in a sparse format, having a skip_length
// indicating how many logical chunks forward to skip (i.e., they are
// not indexed because they are not in the file), and a run length,
// indicating how many indexed chunks' locations follow.
//
// location_run = skip_length, run_length, location*;
//
// skip_length = big_endian_u64;
// run_length  = big_endian_u64;
// location    = big_endian_u64 excluding 0xffff_ffff_ffff_ffff;
//
// location represents how many bytes from the start of the chunk_data
// the referenced numbered_chunk is. (Note that this is different to
// how the index is stored in memory at run-time, where it is relative
// to the beginning of the file.) 0xffff_ffff_ffff_ffff is a reserved
// number. (It is used internally as a null value.)
//
// The sum of all skips and runs must equal the chunk count, which is
// implied by the size and chunk_size specified in the header. If
// there are no logical chunks (i.e. the represented device is zero
// bytes long), the index has zero size (excluding the index_size
// record). The file should not contain a location_run record with
// both skip and run equal to zero.
//
// No additional data is permitted at the end of the file.
//
//
// Format considerations:
//   - Can be written sequentially (without seeking) to a stream (may waste storage).
//   - Can be read sequentially (without seeking) from a stream (no index support).
//   - Can be indexed for selective reads.
//   - Overhead should be small, regardless of size of underlying storage or chunk size.
//   - Header size (and thus the position of chunk data) has no effect on index positions.

use std::path::{Path,PathBuf};
use std::fs::File;
use std::io::{Read,Write,Seek,SeekFrom};
use std::cell::RefCell;
use serde::{Serialize,Deserialize};
use crate::quick_io::{read_be_u64,write_be_u64,read_skip_run,write_skip_run,CountedWrite};
use crate::chunk::Chunk;
use super::{Storage,StorageProperties,Index};


#[derive(Serialize,Deserialize)]
struct FileHeader {
    size: u64,
    chunk_size: usize,
    /// Dictates that chunks are sorted and unique within the file
    optimized: bool,
    indexed: bool,
}

pub struct SparseStorage<IndexType> {
    path: PathBuf,
    file: File,
    size: u64,
    chunk_size: usize,
    chunk_count: usize,
    numbered_chunks_start: Option<u64>,
    /// If there is an ending marker written yet, this falls on
    /// it. Otherwise, just after last written numbered_chunk.
    numbered_chunks_end:   Option<u64>,
    readable: bool,
    writeable: bool,
    /// If true, enables re-writing over previously written chunks
    seekable: bool,
    end_of_chunks: bool,
    optimize_after: bool,
    /// Logical chunk number to file offset index.
    ///
    /// Note that although this internal representation uses the
    /// offset from the start of the file, the index stored on disk is
    /// relative to the start of the numbered chunks
    /// (numbered_chunks_start).
    index: Option<Box<IndexType>>,
    save_index: bool,
}

#[derive(Eq,PartialEq,Clone,Serialize,Deserialize,Debug)]
pub struct Parameters {
    /// Whether or not to save an index.
    ///
    /// Note that an index may or may not be used whilst processing
    /// regardless of this value.
    pub save_index: bool,
    pub append_only: bool,
    pub optimize: bool,
}

#[derive(Eq,PartialEq,Clone,Serialize,Deserialize,Debug)]
#[serde(default)]
pub struct InterfaceParameters {
    index: bool,
    append_only: bool,
    optimize: bool,
}
impl Default for InterfaceParameters {
    fn default() -> Self {
        Self {
            index: true,
            append_only: false,
            optimize: false,
        }
    }
}
impl crate::control::interface::Internalize<Parameters> for InterfaceParameters {
    fn internalize(&self) -> Result<Parameters,String> {
        Ok(Parameters {
            save_index: self.index,
            append_only: self.append_only,
            optimize: self.optimize,
        })
    }
}


impl<IndexType: Index> SparseStorage<IndexType> {
    pub fn create_file(path: &Path, size: u64, chunk_size: usize, parameters: &Parameters, index: Option<Box<IndexType>>) -> Result<Self, String> {
        let chunk_count: usize = ((size + chunk_size as u64 - 1) / chunk_size as u64) as usize;

        let mut file = match File::create(path) {
            Ok(x) => x,
            Err(e) => {
                return Err(format!("Could not create sparse backup file {}: {:?}", path.display(), e));
            },
        };
        let header = FileHeader {
            size,
            chunk_size,
            optimized: false,
            indexed: parameters.save_index && index.is_some(),
        };
        if let Err(e) = serde_json::to_writer::<&File,FileHeader>(&mut file, &header) {
            return Err(format!("Could not write header to backup file {}: {:?}", path.display(), e));
        }
        let numbered_chunks_start = if !parameters.append_only {
            match file.seek(std::io::SeekFrom::Current(0)) {
                Ok(offset) => Some(offset),
                Err(e) => {
                    return Err(format!("Could not seek/find offset in non-append-only backup file {}: {:?}", path.display(), e));
                },
            }
        } else {
            None
        };

        Ok(Self{
            path: path.to_path_buf(),
            file,
            size,
            chunk_size,
            chunk_count,
            numbered_chunks_start,
            numbered_chunks_end: numbered_chunks_start,
            readable: false,
            writeable: true,
            seekable: !parameters.append_only,
            end_of_chunks: false,
            optimize_after: parameters.optimize,
            index,
            save_index: parameters.save_index,
        })
    }

    pub fn get_path(&self) -> &Path {
        self.path.as_path()
    }

    fn open_file(path: &Path, mut index: Option<Box<IndexType>>) -> Result<Self,String> {
        match File::open(path) {
            Ok(mut file) => {
                let header_result = {
                    // Doing it this way means parsing stops as soon as the JSON object is read.
                    let mut deserializer = serde_json::Deserializer::from_reader(&mut file);
                    FileHeader::deserialize(&mut deserializer)
                };
                match header_result {
                    Ok(header) => {
                        let numbered_chunks_start = match file.seek(SeekFrom::Current(0)) {
                            Ok(offset) => Some(offset),
                            Err(e) => {
                                eprintln!("Could not seek/find offset in backup file {}. Assuming pipe: {}", path.display(), e);
                                None
                            },
                        };
                        let seekable = numbered_chunks_start.is_some();
                        let chunk_count: usize = ((header.size + header.chunk_size as u64 - 1) / header.chunk_size as u64) as usize;
                        if let Some(mut index) = index.as_mut() {
                            if !header.indexed {
                                return Err(format!("Index not included in backup {}", path.display()));
                            }
                            if !seekable {
                                return Err(format!("Index cannot be used as backup {} is not seekable", path.display()));
                            }
                            let numbered_chunks_start = numbered_chunks_start.unwrap();
                            let index_end = file.seek(SeekFrom::End(-8)).expect("Could not seek to index size record");
                            let index_size = read_be_u64(&mut file)?;
                            let index_start = match index_end.checked_sub(index_size) {
                                Some(index_start) => {
                                    // Header + mandatory end of chunks marker
                                    if index_start < numbered_chunks_start.checked_add(8).unwrap() {
                                        return Err(format!("index size suggests index start is before beginning of chunks"));
                                    }
                                    index_start
                                },
                                None => return Err(format!("index size suggests index start is before beginning of file")),
                            };
                            file.seek(SeekFrom::Start(index_start)).expect("Could not seek to index");

                            {
                                let index = RefCell::new(&mut index);
                                read_skip_run(
                                    &mut file,
                                    chunk_count as u64,
                                    |read, position| {
                                        let raw_position: u64 = read_be_u64(read)?;
                                        // Convert from chunk data-space to file-space
                                        let remapped_position =
                                            match raw_position.checked_add(numbered_chunks_start) {
                                                Some(x) => {
                                                    if x == std::u64::MAX {
                                                        return Err(format!("Index position converts to reserved value when converted to file space"));
                                                    }
                                                    x
                                                },
                                                None => {
                                                    return Err(format!("Index position overflows when converted to file space"));
                                                },
                                            };
                                        (**index.borrow_mut()).replace(position as usize, remapped_position);
                                        Ok(())
                                    },
                                    |_position| {
                                        Ok(())
                                    },
                                )?;
                            }
                            let test_index_end = file.seek(SeekFrom::Current(0)).expect("Could not check file position");
                            if test_index_end != index_end {
                                return Err(format!("Index size or index structure is incorrect for {}", path.display()));
                            }
                            file.seek(SeekFrom::Start(numbered_chunks_start)).expect("Could not seek backup file after reading index");
                        }
                        Ok(SparseStorage{
                            path: path.to_path_buf(),
                            file,
                            size: header.size,
                            chunk_size: header.chunk_size,
                            chunk_count,
                            numbered_chunks_start,
                            numbered_chunks_end: None,
                            readable: true,
                            writeable: false,
                            seekable,
                            end_of_chunks: false,
                            optimize_after: false,
                            index,
                            save_index: false, // Doesn't make sense if not writeable
                        })
                    },
                    Err(e) => {
                        return Err(format!("Failed to read a valid sparse backup header from file {}: {:?}", path.display(), e))
                    },
                }
            },
            Err(e) => {
                return Err(format!("Could not open sparse backup file {}: {:?}", path.display(), e))
            },
        }
    }

    fn inspect_file(path: &Path) -> Result<StorageProperties,String> {
        match File::open(path) {
            Ok(mut file) => {
                let header_result = {
                    // Doing it this way means parsing stops as soon as the JSON object is read.
                    let mut deserializer = serde_json::Deserializer::from_reader(&mut file);
                    FileHeader::deserialize(&mut deserializer)
                };
                match header_result {
                    Ok(header) => {
                        Ok(StorageProperties {
                            size: header.size,
                            indexed: header.indexed,
                        })
                    }
                    Err(e) => {
                        return Err(format!("Failed to read a valid sparse backup header from file {} for inspection: {:?}", path.display(), e))
                    },
                }
            },
            Err(e) => {
                return Err(format!("Could not open sparse backup file {} for inspection: {:?}", path.display(), e))
            },
        }
    }
}

impl<IndexType: Index> Storage for SparseStorage<IndexType> {
    fn write_chunk(&mut self, chunk: &Chunk) -> Result<(),String> {
        if !self.writeable {
            panic!("Storage is not writeable");
        }
        if chunk.data.len() != self.chunk_size {
            panic!("Chunk has unexpected chunk size");
        }
        let chunk_number = chunk.chunk_number(self.chunk_size, self.size);
        let write_at = if self.seekable {
            if let Some(index) = &self.index {
                match index.lookup(chunk_number) {
                    None => {
                        self.file.seek(SeekFrom::End(0)).expect("Backup file seek failed")
                    },
                    Some(file_offset) => {
                        self.file.seek(SeekFrom::Start(file_offset)).expect("Backup file seek failed")
                    }
                }
            } else {
                self.numbered_chunks_end.unwrap()
            }
        } else {
            self.numbered_chunks_end.unwrap()
        };

        let padding: usize =
            if chunk.data.len() < self.chunk_size {
                self.chunk_size - chunk.data.len()
            } else {
                0
            };

        let mut counted_write = CountedWrite::new(&mut self.file);
        write_be_u64(&mut counted_write, chunk.offset)?;
        counted_write.write_all(&chunk.data).map_err(|e|{format!("Could not write {} bytes of chunk data: {:?}", chunk.data.len(), e)})?;
        if padding > 0 {
            let padding_bytes: Vec<u8> = vec![0; padding];
            counted_write.write_all(&padding_bytes).map_err(|e|{format!("Could not write {} bytes of padding (after {} bytes of chunk data): {:?}", padding, chunk.data.len(), e)})?;
        }

        if let Some(index) = self.index.as_mut() {
            if write_at == std::u64::MAX {
                panic!("Reserved index value cannot be used.");
            }
            index.replace(chunk_number, write_at);
        }
        if self.seekable {
            if self.numbered_chunks_end.unwrap() == write_at {
                self.numbered_chunks_end = Some(write_at + counted_write.get_count());
            }
        }
        Ok(())
    }
    fn read_chunk_at(&mut self, chunk_number: usize) -> Result<Option<Chunk>,String> {
        if !self.readable {
            return Err(format!("Backup is not readable"));
        }
        if !self.seekable {
            return Err(format!("Backup {} is not seekable", self.path.display()));
        }
        if chunk_number > self.chunk_count {
            panic!("chunk number exceeds chunk count");
        }
        if let Some(index) = &self.index {
            match index.lookup(chunk_number) {
                None => {
                    // Chunk not contained in this backup
                    Ok(None)
                },
                Some(file_offset) => {
                    self.file.seek(SeekFrom::Start(file_offset)).expect("Seek failed during backup");
                    let chunk = self.read_chunk()?.expect("No chunk at indexed location");
                    if chunk.offset != (chunk_number as u64) * (self.chunk_size as u64) {
                        return Err(format!("Chunk at indexed location has incorrect offset. Chunk number {} indexed at file offset {} has chunk offset {}, but should be {}.", chunk_number, file_offset, chunk.offset, (chunk_number as u64) * (self.chunk_size as u64)));
                    }
                    Ok(Some(chunk))
                }
            }
        } else {
            return Err(format!("Backup {} has no index", self.path.display()));
        }
    }

    fn read_chunk(&mut self) -> Result<Option<Chunk>,String> {
        if !self.readable {
            return Err(format!("Backup is not readable"));
        }
        if self.end_of_chunks {
            return Err(format!("End of chunks already encountered"));
        }
        let offset: u64 = read_be_u64(&mut self.file)?;
        if offset == std::u64::MAX {
            self.end_of_chunks = true;
            return Ok(None);
        }
        if offset >= self.size {
            panic!("Offset {} is not within size {}", offset, self.size);
        }
        if offset % self.chunk_size as u64 != 0 {
            panic!("Offset {} is not a multiple of the chunk size {}", offset, self.chunk_size);
        }
        let available: u64 = self.size - offset;
        let data_len: usize = if available < self.chunk_size as u64 {
            available as usize
        } else {
            self.chunk_size
        };
        let mut data: Vec<u8> = vec![0; self.chunk_size];
        self.file.read_exact(&mut data).map_err(|e|{format!("Could not read {} bytes of chunk data: {:?}", self.chunk_size, e)})?;
        if data_len != self.chunk_size {
            // The padding isn't checked for being all zeroed.
            data.truncate(data_len);
        }
        Ok(Some(Chunk{
            offset,
            data,
        }))
    }

    fn commit(&mut self) -> Result<(),String> {
        if !self.writeable {
            panic!("Storage is not writeable. Committing does not make sense.");
        }
        if self.seekable {
            let position = self.file.seek(SeekFrom::End(0)).expect("Backup file seek failed");
            if position != self.numbered_chunks_end.unwrap() {
                panic!("End of numbered chunks is not at the end of file");
            }
        }
        // Write end-of-chunks marker
        write_be_u64(&mut self.file, std::u64::MAX)?;
        if let Some(index) = &self.index {
            let numbered_chunks_start = self.numbered_chunks_start.unwrap();
            // Write index and record its size
            let index_size = {
                let mut counted_write = CountedWrite::new(&mut self.file);
                write_skip_run(
                    &mut counted_write,
                    self.chunk_count as u64,
                    |write, position| {
                        let position = index.lookup(position as usize).unwrap();
                        // Convert from file-space to chunk data-space
                        let remapped_position = position.checked_sub(numbered_chunks_start).expect("Bogus internal index value underflows when remapped");
                        write_be_u64(write, remapped_position)?;
                        Ok(())
                    },
                    |position| {
                        Ok(index.lookup(position as usize).is_some())
                    },
                )?;
                counted_write.get_count()
            };
            // Write index size
            write_be_u64(&mut self.file, index_size)?;
        }
        if let Err(e) = self.file.sync_all() {
            return Err(format!("Failed to sync all data before closing: {:?}", e));
        }
        Ok(())
    }
}
