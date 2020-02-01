use std::convert::TryInto;
use std::path::Path;
use std::marker::Send;

use crate::control::{Job,StorageFormat,StoragePolicy};
use crate::state::State;
use crate::chunk::Chunk;

use crate::storage::Storage;
use crate::storage::raw::RawStorage;
use crate::storage::sparse::SparseStorage;
use crate::storage::null::NullStorage;

use crate::checksums::{Checksums,ChecksumDiff};
use crate::checksums::sparse::SparseChecksums;
use crate::checksums::null::NullChecksums;


pub struct Backup {
    job: Job,
    storage: Box<dyn Storage + Send>,
    checksums: Box<dyn Checksums + Send>,
    write_when_checksum_unchanged: bool,
    write_when_checksum_touched: bool,
    write_when_checksum_replaced: bool,
}

impl Backup {
    pub fn new(job: &Job, size: u64, state: &State) -> Result<Self,String> {
        let chunk_count: usize = (size / (job.chunk_size as u64)).try_into().expect(&format!("A backup of size {} using a chunk size of {} creates too many chunks to be supported on this platform.", size, job.chunk_size));
        let job = job.clone();
        let storage: Box<dyn Storage + Send> =
            match &job.storage.format {
                StorageFormat::Raw => {
                    match &job.storage.storage_policy {
                        StoragePolicy::Full => Box::new(RawStorage::create_file(&state.stored_path(&job.storage.destination), size, job.chunk_size)?),
                        x => return Err(format!("Raw backup format only supports the Full storage policy - not {:?}", x)),
                    }
                },
                StorageFormat::Sparse(parameters) => {
                    match &job.storage.storage_policy {
                        StoragePolicy::Full => {}, // OK
                        StoragePolicy::Incremental => {}, // OK
                        x => return Err(format!("Sparse backup format only supports the Full and Incremental storage policies - not {:?}", x)),
                    }
                    Box::new(SparseStorage::create_file(&state.stored_path(&job.storage.destination), size, job.chunk_size, &parameters)?)
                },
                StorageFormat::Null => {
                    match &job.storage.storage_policy {
                        StoragePolicy::Volatile => Box::new(NullStorage::new()),
                        x => return Err(format!("Null backup only supports the Volatile storage policy - not {:?}", x)),
                    }
                },
            };
        let checksums: Box<dyn Checksums + Send>;
        let trust_checksums;
        if let Some(job_checksum) = &job.checksum {
            let mut sparse_checksums = SparseChecksums::new(
                &state.stored_path(&job_checksum.destination),
                &job_checksum.algorithm,
                job_checksum.size,
                job.chunk_size,
                chunk_count,
                job_checksum.storage_policy
            )?;

            // Only bother loading checksums if we're doing an incremental
            if job.storage.storage_policy == StoragePolicy::Incremental {
                let mut checksum_chain = Vec::new();
                for historical_state in state.history() {
                    let historical_job = historical_state.source_to_job(&job.source);
                    if let Some(historical_job_checksum) = &historical_job.checksum {
                        if historical_job_checksum.trust
                            && historical_job_checksum.algorithm
                            == job_checksum.algorithm
                            && historical_job_checksum.size
                            == job_checksum.size
                        {
                            match historical_job_checksum.storage_policy {
                                StoragePolicy::Full => {
                                    // This checksum has everything
                                    // before it, so any previous
                                    // chain is redundant.
                                    checksum_chain.clear();
                                    // New chain starts
                                    checksum_chain.push(historical_state.stored_path(&historical_job_checksum.destination));
                                },
                                StoragePolicy::Incremental => {
                                    // Compatible checksum, build upon
                                    // the existing chain. Note that
                                    // it's fine for this to be the
                                    // start of a chain.
                                    checksum_chain.push(historical_state.stored_path(&historical_job_checksum.destination));
                                },
                                StoragePolicy::Volatile => {
                                    // Equivalent to no checksum
                                    checksum_chain.clear();
                                },
                            }
                        } else {
                            // This checksum isn't even compatible, so
                            // it breaks any chain that was previously
                            // forming.
                            checksum_chain.clear();
                        }
                    } else {
                        // No checksum, so any existing chain is
                        // broken.
                        checksum_chain.clear();
                    }
                }
                let checksum_chain = checksum_chain; // un-mut
                for historical_checksum_location in &checksum_chain {
                    // For now, SparseChecksums is the only format.
                    SparseChecksums::load_file(&historical_checksum_location, &mut sparse_checksums)?;
                }
            }
            checksums = Box::new(sparse_checksums);
            trust_checksums = job_checksum.trust;
        } else {
            checksums = Box::new(NullChecksums::new(
                "null",
                0,
                job.chunk_size,
                chunk_count
            ));
            trust_checksums = false;
        };
        let write_when_checksum_touched;
        let write_when_checksum_unchanged;
        let write_when_checksum_replaced;
        match &job.storage.storage_policy {
            StoragePolicy::Full => {
                write_when_checksum_unchanged = !trust_checksums;
                write_when_checksum_touched   = true;
                write_when_checksum_replaced  = true;
            },
            StoragePolicy::Incremental => {
                if !trust_checksums {
                    return Err(format!("Incremental backups cannot be performed without trustable checksums"));
                }
                write_when_checksum_unchanged = false;
                write_when_checksum_touched   = false;
                write_when_checksum_replaced  = true;
            },
            StoragePolicy::Volatile => {
                write_when_checksum_unchanged = false;
                write_when_checksum_touched   = false;
                write_when_checksum_replaced  = false;
            },
        }

        Ok(Self {
            job,
            storage,
            checksums,
            write_when_checksum_unchanged,
            write_when_checksum_touched,
            write_when_checksum_replaced,
        })
    }

    pub fn process_chunk(&mut self, chunk: &Chunk) -> Result<(),String> {
        let diff = self.checksums.record_chunk(&chunk);
        let should_write = match diff {
            ChecksumDiff::Unchanged => self.write_when_checksum_unchanged,
            ChecksumDiff::Touched   => self.write_when_checksum_touched,
            ChecksumDiff::Replaced  => self.write_when_checksum_replaced,
        };
        if should_write {
            self.storage.write_chunk(&chunk)?;
        }
        Ok(())
    }

    pub fn commit(&mut self) -> Result<(),String> {
        self.checksums.commit()?;
        self.storage.commit()?;
        Ok(())
    }

    pub fn get_storage_path(&self) -> &Path {
        &self.job.storage.destination
    }
}
