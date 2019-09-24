use chunk::Chunk;
use std::sync::mpsc::Receiver;
use backup_file::BackupFile;

pub struct Writer<'d> {
    destinations: &'d mut Vec<BackupFile>,
}

impl<'d> Writer<'d> {
    pub fn new(destinations: &'d mut Vec<BackupFile>) -> Self {
        Writer {
            destinations,
        }
    }

    pub fn run(&mut self, write_queue_consume: Receiver<(usize, Chunk)>) {
        while let Ok((device_number, chunk)) = write_queue_consume.recv() {
            self.destinations[device_number].write_chunk(chunk);
        }
    }
}
