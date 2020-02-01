use std::sync::mpsc::Receiver;
use crate::backup::Backup;
use crate::chunk::Chunk;

pub fn run(destinations: &mut Vec<Backup>, write_queue_consume: Receiver<(usize, Chunk)>) -> Result<(),String> {
    while let Ok((device_number, chunk)) = write_queue_consume.recv() {
        destinations[device_number].process_chunk(&chunk)?;
    }
    Ok(())
}
