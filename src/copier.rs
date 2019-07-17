use std::sync::mpsc::channel;
use std::sync::mpsc::sync_channel;
use std::sync::mpsc::TryRecvError;
use std::sync::{Arc,Barrier};
use std::path::Path;

use device::{Device,DeviceFile};
use backup_file::BackupFile;
use chunk_tracker::ChunkTracker;
use writer::Writer;
use change_logger::ChangeLogger;

pub struct Copier<'s, 'd> {
    chunk_size: usize,
    source: &'s mut DeviceFile,
    destination: &'d mut BackupFile,
}

impl<'s, 'd> Copier<'s, 'd> {
    pub fn new(chunk_size: usize, source: &'s mut DeviceFile, destination: &'d mut BackupFile) -> Self {
        Copier {
            chunk_size,
            source,
            destination,
        }
    }

    pub fn run(&mut self) -> Result<(),()> {
        let source_path = self.source.get_path().to_path_buf();
        let destination_path = self.destination.get_path().to_path_buf();

        let device = Device::from_file(&self.source).unwrap();

        let chunk_count: usize = (
            self.source.get_size() / (self.chunk_size as u64)
            + if self.source.get_size() % (self.chunk_size as u64) != 0 {1} else {0}
        ) as usize;

        let mut chunk_tracker = ChunkTracker::new(chunk_count);

        let (change_queue_produce, change_queue_consume) = channel();
        // The sync channel size could possibly be enlarged.
        let (write_queue_produce, write_queue_consume) = sync_channel(1);
        let (sync_barrier_produce, sync_barrier_consume) = channel();

        crossbeam::scope(|thread_scope| {
            // let change_logger_thread = 
            {
                let chunk_size = self.chunk_size;
                let device_ref = &device;
                thread_scope.spawn(move |_| {
                    let change_logger = ChangeLogger::new(chunk_size, device_ref, Path::new("/sys/kernel/debug/tracing"));
                    change_logger.run(change_queue_produce, sync_barrier_consume);
                });
            }
            // let writer_thread = 
            {
                let destination = &mut self.destination;
                thread_scope.spawn(move |_| {
                    let mut writer = Writer::new(*destination);
                    writer.run(write_queue_consume);
                });
            }

            let update_chunk_tracker = |chunk_tracker: &mut ChunkTracker| {
                'drain_change_queue: loop {
                    match change_queue_consume.try_recv() {
                        Ok(change_index) => {
                            chunk_tracker.mark_chunk(change_index);
                        },
                        Err(TryRecvError::Empty) => {
                            break 'drain_change_queue;
                        },
                        Err(_) => {
                            panic!("Unexpected error reading change queue");
                        },
                    }
                }
            };

            // Make sure the change logger is ready
            {
                let barrier = Arc::new(Barrier::new(2));
                sync_barrier_produce.send(Arc::clone(&barrier)).expect("Change logger thread died before it was relieved");
                barrier.wait();
            }

            let mut find_index: Option<usize> = None;
            let mut synced = false;

            let log_chunk_count: u64 = (chunk_count as f64).log2() as u64;
            let display_detail: usize = 
                if log_chunk_count <= 9 {
                    0
                } else {
                    log_chunk_count - 9
                } as usize;
            let display_detail_size = 1 << display_detail;
            let mut display_ticker = 0;

            'copy_loop: loop {
                // Find next dirty index
                match find_index {
                    None => {
                        find_index = chunk_tracker.find_next(0);
                    },
                    Some(index) => {
                        find_index = chunk_tracker.find_next(index);
                        if find_index.is_none() {
                            // Wrap around
                            continue 'copy_loop;
                        }
                    },
                }

                // Act on index (or end if none)
                match find_index {
                    None => {
                        // Only stop when we've done two consecutive syncs without any events in between them.
                        if synced {
                            // We've caught up!
                            break 'copy_loop;
                        } else {
                            // Not a clue why this function is unsafe :P
                            unsafe {libc::sync()};

                            // Make sure all the sync write events are captured.
                            let barrier = Arc::new(Barrier::new(2));
                            sync_barrier_produce.send(Arc::clone(&barrier)).expect("Change logger thread died before it was relieved");
                            barrier.wait();

                            update_chunk_tracker(&mut chunk_tracker);
                            synced = true;
                        }
                    },
                    Some(index) => {
                        synced = false;

                        // Clear here, so it has a change to get re-marked as dirty.
                        chunk_tracker.clear_chunk(index);

                        update_chunk_tracker(&mut chunk_tracker);

                        // We need to get the chunk here (synchronously) before we
                        // next consume the change queue.
                        let chunk = self.source.get_chunk(index as u64 * self.chunk_size as u64, self.chunk_size);

                        write_queue_produce.send(chunk).expect("Writer thread died before it was relieved");
                    },
                }

                display_ticker += 1;
                if display_ticker >= display_detail_size {
                    display_ticker = 0;
                    println!("Copying '{}' to '{}'\nProcessing as {} chunks of size {}\n{}", source_path.display(), destination_path.display(), chunk_count, self.chunk_size, chunk_tracker.summary_report(display_detail));
                }
            }
        }).unwrap();
        // change_logger_thread.join().expect("Change logger thread paniced");
        // writer_thread.join().expect("Writer thread paniced");

        Ok(())
    }
}
