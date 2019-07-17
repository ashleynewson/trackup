use chunk::Chunk;
use std::sync::mpsc::Receiver;
use backup_file::BackupFile;

pub struct Writer<'d> {
    destination: &'d mut BackupFile,
}

impl<'d> Writer<'d> {
    pub fn new(destination: &'d mut BackupFile) -> Self {
        Writer {
            destination,
        }
    }

    pub fn run(&mut self, write_queue_consume: Receiver<Chunk>) {
        while let Ok(chunk) = write_queue_consume.recv() {
            self.destination.write_chunk(chunk);
        }
    }
}
