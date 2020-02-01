use ::digest::{Digest,DynDigest};
use std::marker::Send;
use crate::chunk::Chunk;

pub mod sparse;
pub mod null;

#[derive(Eq,PartialEq,Clone,Copy)]
enum ChunkSource {
    Absent,
    Historic,
    Current,
}

pub enum ChecksumDiff {
    /// There was no change to the checksum. The reference checksum is from this run.
    Unchanged,
    /// The was no change to the checksum. The reference checksum is from a historic run.
    Touched,
    /// The checksum has (possibly) been updated to a new value.
    Replaced,
}

pub trait Checksums {
    /// Set a chunk's checksum (for pre-backup initialisation).
    ///
    /// Later calls overwrite existing values. The source will be set
    /// to historic. Merges where chunk_number is greater than the
    /// chunk count should be ignored.
    fn merge_chunk(&mut self, chunk_number: usize, checksum: &[u8]);
    /// Record a chunk's checksum (during backup), and return whether or not the checksum has changed
    fn record_chunk(&mut self, chunk: &Chunk) -> ChecksumDiff;
    /// Save data to file
    fn commit(&self) -> Result<(),String>;
    fn get_checksum_algorithm(&self) -> &str;
    fn get_checksum_size(&self) -> usize;
    fn get_chunk_count(&self) -> usize;
    fn get_chunk_size(&self) -> usize;
}

fn resolve_algorithm(algorithm_name: &str, checksum_size: usize) -> Result<Box<dyn DynDigest + Send>,String> {
    let hasher: Box<dyn DynDigest + Send> = match (algorithm_name, checksum_size) {
        ("sha256", 28) => {
            Box::new(sha2::Sha224::new())
        },
        ("sha256", 32) => {
            Box::new(sha2::Sha256::new())
        },
        ("sha512", 28) => {
            Box::new(sha2::Sha512Trunc224::new())
        },
        ("sha512", 32) => {
            Box::new(sha2::Sha512Trunc256::new())
        },
        ("sha512", 48) => {
            Box::new(sha2::Sha384::new())
        },
        ("sha512", 64) => {
            Box::new(sha2::Sha512::new())
        },
        ("blake2b", size) => {
            if size > 64 || size == 0 {
                return Err(format!("blake2b only supports checksum sizes from 1 to 64 bytes inclusive"));
            }
            Box::new(blake2::Blake2b::new())
        },
        ("blake2s", size) => {
            if size > 32 || size == 0 {
                return Err(format!("blake2s only supports checksum sizes from 1 to 32 bytes inclusive"));
            }
            Box::new(blake2::Blake2s::new())
        },
        _ => {
            return Err(format!("Unknown checksum (algorithm, size) combo ({}, {})", algorithm_name, checksum_size));
        }
    };
    Ok(hasher)
}
