use std::sync::mpsc::Receiver;
use crate::backup_file::BackupFile;
use crate::chunk::Chunk;

pub fn run(destinations: &mut Vec<BackupFile>, write_queue_consume: Receiver<(usize, Chunk)>) {
    while let Ok((device_number, chunk)) = write_queue_consume.recv() {
        destinations[device_number].write_chunk(chunk);
    }
}
