use std::sync::mpsc::channel;
use std::sync::mpsc::sync_channel;
use std::sync::mpsc::{TryRecvError,TrySendError};
use std::sync::{Arc,Barrier};
use std::time::{Duration,Instant};
use std::io::Write;
use std::path::PathBuf;

use config::Config;
use device::{Device,DeviceFile};
use backup_file::BackupFile;
use chunk_tracker::ChunkTracker;
use writer::Writer;
use change_logger::ChangeLogger;

pub struct Copier<'c, 's, 'd> {
    config: &'c Config<'c>,
    sources: &'s mut Vec<DeviceFile>,
    destinations: &'d mut Vec<BackupFile>,
}

impl<'c, 's, 'd> Copier<'c, 's, 'd> {
    pub fn new(config: &'c Config, sources: &'s mut Vec<DeviceFile>, destinations: &'d mut Vec<BackupFile>) -> Self {
        Copier {
            config,
            sources,
            destinations,
        }
    }

    pub fn run(&mut self) -> Result<(),()> {
        let number_of_devices = self.sources.len();

        let source_paths: Vec<PathBuf> = self.sources.iter().map(
            |source| {source.get_path().to_path_buf()}
        ).collect();
        let destination_paths: Vec<PathBuf> = self.destinations.iter().map(
            |destination| {destination.get_path().to_path_buf()}
        ).collect();

        let devices = self.sources.iter().map(
            |source| {
                Device::from_file(self.config, source).unwrap()
            }
        ).collect();

        let mut total_chunk_count = 0;
        let mut chunk_trackers = self.sources.iter().map(
            |source| {
                let chunk_count: usize = (
                    source.get_size() / (self.config.chunk_size as u64)
                        + if source.get_size() % (self.config.chunk_size as u64) != 0 {1} else {0}
                ) as usize;

                total_chunk_count += chunk_count;

                ChunkTracker::new(chunk_count)
            }
        ).collect();

        let (change_queue_produce, change_queue_consume) = channel();
        // The sync channel size could possibly be enlarged.
        let (write_queue_produce, write_queue_consume) = sync_channel(4);
        let (sync_barrier_produce, sync_barrier_consume) = channel();

        crossbeam::scope(|thread_scope| {
            {
                let devices_ref = &devices;
                let config = self.config;

                thread_scope.builder()
                    .name("change-logger".to_string())
                    .spawn(move |_| {
                        let change_logger = ChangeLogger::new(config, devices_ref);
                        change_logger.run(change_queue_produce, sync_barrier_consume);
                    })
                    .unwrap();
            }
            {
                let destinations = &mut self.destinations;
                thread_scope.builder()
                    .name("writer".to_string())
                    .spawn(move |_| {
                        let mut writer = Writer::new(*destinations);
                        writer.run(write_queue_consume);
                    })
                    .unwrap();
            }

            // Constrain the lifetime of our producers/consumers so that
            // the child threads can witness a disconnect.
            let sync_barrier_produce = sync_barrier_produce;
            let write_queue_produce = write_queue_produce;
            // We don't need to constrain change_queue as it doesn't
            // strictly control any looping behaviour.

            let update_chunk_trackers = |chunk_trackers: &mut Vec<ChunkTracker>| {
                'drain_change_queue: loop {
                    match change_queue_consume.try_recv() {
                        Ok((device_number, change_index)) => {
                            chunk_trackers[device_number].mark_chunk(change_index);
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

            let display_detail: usize = 
                if total_chunk_count <= self.config.max_diagram_size {
                    0
                } else {
                    // Mathematically, the first ceil isn't necessary, but I'm
                    // being (likely unnecessarily) paranoid about precision.
                    (total_chunk_count as f64 / self.config.max_diagram_size as f64).ceil().log2().ceil() as u64
                } as usize;

            let mut last_progress_update = Instant::now();
            let mut total_writes = 0;

            // Only stop when we've done two consecutive syncs without any events in between them.
            let mut synced = false;
            while !synced {
                // Everything in libc is unsafe. :P
                unsafe {libc::sync()};

                // Make sure all the sync write events are captured.
                let barrier = Arc::new(Barrier::new(2));
                sync_barrier_produce.send(Arc::clone(&barrier)).expect("Change logger thread died before it was relieved");
                barrier.wait();

                update_chunk_trackers(&mut chunk_trackers);
                synced = true;

                let mut still_copying = true;
                while still_copying {
                    still_copying = false;
                    for device_number in 0..number_of_devices {
                        let mut find_index: Option<usize> = None;
                        'device_copy_loop: loop {
                            // Find next dirty index
                            match find_index {
                                None => {
                                    find_index = chunk_trackers[device_number].find_next(0);
                                },
                                Some(index) => {
                                    find_index = chunk_trackers[device_number].find_next(index);
                                },
                            }

                            // Act on index (or end if none)
                            match find_index {
                                None => {
                                    break 'device_copy_loop;
                                },
                                Some(index) => {
                                    still_copying = true;
                                    synced = false;

                                    // Clear here, so it has a chance to get re-marked as
                                    // dirty in case it's written to whilst we read it.
                                    chunk_trackers[device_number].clear_chunk(index);

                                    let chunk = self.sources[device_number].get_chunk(index as u64 * self.config.chunk_size as u64, self.config.chunk_size);
                                    let mut message = Some( (device_number, chunk) );

                                    'write_try_loop: loop {
                                        update_chunk_trackers(&mut chunk_trackers);

                                        match write_queue_produce.try_send( message.take().unwrap() ) {
                                            Ok(()) => {
                                                break 'write_try_loop;
                                            },
                                            Err(TrySendError::Full(bounced)) => {
                                                // Put back the message.
                                                message.replace(bounced);
                                                // Might as well not hammer the CPU if
                                                // we're waiting on something.
                                                std::thread::sleep(Duration::from_millis(1));
                                            },
                                            Err(TrySendError::Disconnected(_)) => {
                                                panic!("Writer thread died before it was relieved");
                                            },
                                        }
                                    }
                                    total_writes += 1;
                                },
                            }

                            if last_progress_update.elapsed() >= self.config.progress_update_period {
                                if self.config.exclusive_progress_updates {
                                    std::io::stdout().write_all(b"\x1b[2J").unwrap();
                                }
                                for i in 0..number_of_devices {
                                    println!("Copying '{}' to '{}'\nProcessing as {} chunks of size {}\n{}", source_paths[i].display(), destination_paths[i].display(), chunk_trackers[i].get_chunk_count(), self.config.chunk_size, chunk_trackers[i].summary_report(self.config, display_detail));
                                }
                                println!(
                                    "Done {}{}   Dirty {}{}   Unprocessed {}{}   UnprocessedDirty {}{}",
                                    self.config.diagram_cells[0], self.config.diagram_cells_reset,
                                    self.config.diagram_cells[1], self.config.diagram_cells_reset,
                                    self.config.diagram_cells[2], self.config.diagram_cells_reset,
                                    self.config.diagram_cells[3], self.config.diagram_cells_reset
                                );
                                println!("Chunk writes: {}", total_writes);
                                last_progress_update = Instant::now();
                            }
                        }
                    }
                }
            }
            println!("Copying complete!");
            println!("Chunk writes: {} (efficiency is {})", total_writes, total_chunk_count as f64 / total_writes as f64);
        }).unwrap();

        println!("All threads finished");

        Ok(())
    }
}
