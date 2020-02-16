use std::path::Path;

use crate::chunk::Chunk;
use crate::control::StorageFormat;
use crate::state::State;
use super::{Storage,Index};
use super::null::NullStorage;
use super::sparse::SparseStorage;
use super::raw::RawStorage;
use super::shared_index::{SharedIndex,SharedIndexHandle};

/// Presents multiple layered stores as a single logical store.
pub struct LayeredStorage {
    chunk_count: usize,
    next_chunk: usize,
    index: SharedIndex,
    layers: Vec<Box<dyn Storage>>,
}

impl LayeredStorage {
    /// Create LayeredStorage
    ///
    /// Loads all stores representing a backup of _source_ in the
    /// chain defined in the state file.
    ///
    /// If write_path is set, the layered store will be writable, with
    /// changes being loaded/recorded from/into a sparse store located
    /// at the given path. The path is not assumed to be relative to a
    /// store directory.
    pub fn open(state: &State, source: &Path, write_path: Option<&Path>) -> Result<Self, String> {
        let top_job = state.source_to_job(source);
        let chunk_size = top_job.chunk_size;
        let storage_properties = match &top_job.storage.format {
            StorageFormat::Raw => {
                RawStorage::inspect_file(&top_job.storage.destination)?
            },
            StorageFormat::Sparse(_) => {
                SparseStorage::<SharedIndexHandle>::inspect_file(&top_job.storage.destination)?
            },
            StorageFormat::Null => {
                return Err(format!("Null storage in backup chain"));
            },
        };
        let size = storage_properties.size;
        let chunk_count = Chunk::chunk_count(size, chunk_size);
        let index = SharedIndex::new(chunk_count);
        let mut layers = Vec::new();

        {
            // Add potentially writable top layer
            let shared_index_handle = index.add_layer(chunk_count);
            let top_layer: Box<dyn Storage> = match write_path {
                Some(write_path) => {
                    if write_path.exists() {
                        Box::new(SparseStorage::open_file(
                            write_path,
                            size,
                            chunk_size,
                            Some(Box::new(shared_index_handle))
                        )?)
                    } else {
                        Box::new(SparseStorage::create_file(
                            write_path,
                            size,
                            chunk_size,
                            &crate::storage::sparse::Parameters {
                                save_index: true,
                                append_only: false,
                                optimize: false,
                            },
                            Some(Box::new(shared_index_handle))
                        )?)
                    }
                },
                None => {
                    Box::new(NullStorage::new())
                },
            };
            layers.push(top_layer);
        }

        // Add subsequent layers
        for loading_state in state.chain() {
            let job = loading_state.source_to_job(source);
            let mut shared_index_handle = index.add_layer(chunk_count);
            let layer: Box<dyn Storage> = match &job.storage.format {
                StorageFormat::Raw => {
                    for chunk_number in 0..chunk_count {
                        shared_index_handle.replace(chunk_number, chunk_number as u64 * chunk_size as u64);
                    }
                    Box::new(RawStorage::open_file(
                        &job.storage.destination,
                        size,
                        chunk_size,
                        false
                    )?)
                },
                StorageFormat::Sparse(_) => {
                    Box::new(SparseStorage::open_file(
                        &job.storage.destination,
                        size,
                        chunk_size,
                        Some(Box::new(shared_index_handle))
                    )?)
                },
                StorageFormat::Null => {
                    return Err(format!("Null storage in backup chain"));
                },
            };
            layers.push(layer);
            if index.is_complete() {
                // Every chunk is fulfilled at some layer, so there's no point adding any more layers.
                break;
            }
        }
        if !index.is_complete() {
            eprintln!("Warning: missing chunks in backup chain");
        }

        Ok(Self {
            chunk_count,
            next_chunk: 0,
            index,
            layers,
        })
    }
}

impl Storage for LayeredStorage {
    fn read_chunk(&mut self) -> Result<Option<Chunk>,String> {
        while self.next_chunk < self.chunk_count && self.index.lookup_layer(self.next_chunk).is_none() {
            self.next_chunk = self.next_chunk + 1;
        }
        if self.next_chunk == self.chunk_count {
            return Ok(None);
        }
        self.read_chunk_at(self.next_chunk)
    }
    fn read_chunk_at(&mut self, chunk_number: usize) -> Result<Option<Chunk>,String> {
        if let Some(layer) = self.index.lookup_layer(chunk_number) {
            self.next_chunk = chunk_number + 1;
            self.layers[layer].read_chunk_at(chunk_number)
        } else {
            Ok(None)
        }
    }
    fn write_chunk(&mut self, chunk: &Chunk) -> Result<(),String> {
        // Note: the top layer is either a null (which will panic
        // here) or a sparse storage (which will update the index).
        self.layers[0].write_chunk(chunk)
    }
    fn commit(&mut self) -> Result<(),String> {
        self.layers[0].commit()
    }
}
