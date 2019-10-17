use std::sync::mpsc::channel;
use std::sync::mpsc::sync_channel;
use std::sync::mpsc::{TryRecvError,TrySendError};
use std::sync::{Arc,Barrier};
use std::time::{Duration,Instant};
use std::io::Write;
use std::path::PathBuf;

use crate::config::Config;
use crate::device::{Device,DeviceFile};
use crate::backup_file::BackupFile;
use crate::chunk_tracker::{ChunkTracker,calculate_display_detail};
use crate::control::{Request,Response,Status,RunStatus,JobProgress,ManagementInterface,Manifest};
use crate::lock::AutoLocker;


pub fn run(config: &Config, manifest: &Manifest, management_interface: &ManagementInterface) -> Result<(),()> {
    let mut sources = Vec::new();
    let mut destinations = Vec::new();
    for job in &manifest.jobs {
        let source = DeviceFile::from_path(&job.source).expect("Could not open device");
        destinations.push(
            if job.reuse_output {
                BackupFile::use_file(&job.destination, source.get_size()).expect("Could not open backup file (reuse)")
            } else {
                BackupFile::create_file(&job.destination, source.get_size()).expect("Could not open backup file (create)")
            }
        );
        sources.push(source);
    }

    let number_of_devices = sources.len();

    let source_paths: Vec<PathBuf> = sources.iter().map(
        |source| {source.get_path().to_path_buf()}
    ).collect();
    let destination_paths: Vec<PathBuf> = destinations.iter().map(
        |destination| {destination.get_path().to_path_buf()}
    ).collect();

    let devices = sources.iter().map(
        |source| {
            Device::from_file(config, source).unwrap()
        }
    ).collect();

    let mut total_chunk_count = 0;
    let mut chunk_trackers = sources.iter().enumerate().map(
        |(i, source)| {
            let chunk_size: u64 = manifest.jobs[i].chunk_size as u64;
            let bytes: u64 = source.get_size();
            let chunk_count: usize = (
                bytes / chunk_size + (if bytes % chunk_size != 0 {1} else {0})
            ) as usize;

            total_chunk_count += chunk_count;

            ChunkTracker::new(chunk_count)
        }
    ).collect();
    let total_chunk_count = total_chunk_count; // drop mut

    let (change_queue_produce, change_queue_consume) = channel();
    // The sync channel size could possibly be enlarged.
    let (write_queue_produce, write_queue_consume) = sync_channel(4);
    let (sync_barrier_produce, sync_barrier_consume) = channel();

    crossbeam::scope(|thread_scope| {
        {
            let devices_ref = &devices;
            thread_scope.builder()
                .name("change-logger".to_string())
                .spawn(move |_| {
                    crate::change_logger::run(config, manifest, devices_ref, change_queue_produce, sync_barrier_consume);
                })
                .unwrap();
        }
        {
            let destinations = &mut destinations;
            thread_scope.builder()
                .name("writer".to_string())
                .spawn(move |_| {
                    crate::writer::run(destinations, write_queue_consume);
                })
                .unwrap();
        }
        let auto_locker = AutoLocker::new(config, manifest);

        // Constrain the lifetime of our producers/consumers so that
        // the child threads can witness a disconnect.
        let sync_barrier_produce = sync_barrier_produce;
        let write_queue_produce = write_queue_produce;
        // We don't need to constrain change_queue as it doesn't
        // strictly control any looping behaviour.

        let display_detail: usize = calculate_display_detail(total_chunk_count, config.max_diagram_size);

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

        let mut cancelled = false;
        let mut paused = false;

        let handle_management_tickets =
            |cancelled: &mut bool, paused: &mut bool, chunk_trackers: &Vec<ChunkTracker>| {
                while let Some(ticket) = management_interface.get_ticket() {
                    let response =
                        match &ticket.request {
                            Request::Start(_) => {
                                Response::Start(Err(String::from("A backup is already running.")))
                            },
                            Request::Cancel => {
                                *cancelled = true;
                                Response::Cancel(Ok(()))
                            },
                            Request::Pause => {
                                *paused = true;
                                Response::Pause(Ok(()))
                            },
                            Request::Resume => {
                                *paused = false;
                                Response::Resume(Ok(()))
                            },
                            Request::Query(query) => {
                                let progress =
                                    manifest.jobs.iter().zip(chunk_trackers).map(
                                        |(job, chunk_tracker)| {
                                            let chunk_count = chunk_tracker.get_chunk_count();
                                            let detail = calculate_display_detail(chunk_count, query.max_diagram_size);
                                            JobProgress {
                                                job: job.clone(),
                                                chunk_count,
                                                cells: chunk_tracker.snapshot_level(detail),
                                                chunks_per_cell: 1 << detail,
                                            }
                                        }
                                    ).collect();

                                let run_status = RunStatus {
                                    manifest: manifest.clone(),
                                    progress,
                                    paused: *paused,
                                };

                                Response::Query(Status::Running(run_status))
                            },
                        };
                    ticket.respond(response);
                }
            };

        let mut last_progress_update = Instant::now();
        let mut total_writes = 0;
        let mut first_go = true;

        // Only stop when we've done an (optional) sync whilst locked without any events occuring after it.
        let mut consistent = false;
        'consistency_loop: while !consistent {
            let locked = !first_go && auto_locker.check() == crate::lock::AutoLockerStatus::Locked;
            let should_sync = first_go || (locked && manifest.do_sync);
            if should_sync {
                // Everything in libc is unsafe. :P
                unsafe {libc::sync()};

                // Make sure all the sync write events are captured.
                let barrier = Arc::new(Barrier::new(2));
                sync_barrier_produce.send(Arc::clone(&barrier)).expect("Change logger thread died before it was relieved");
                barrier.wait();

                update_chunk_trackers(&mut chunk_trackers);
            }
            consistent = locked;

            let mut still_copying = true;
            while still_copying {
                still_copying = false;
                for device_number in 0..number_of_devices {
                    let mut find_index: Option<usize> = None;
                    'device_copy_loop: loop {
                        if paused {
                            std::thread::sleep(Duration::from_millis(10));
                            update_chunk_trackers(&mut chunk_trackers);
                        } else {
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
                                    consistent = false;

                                    // Clear here, so it has a chance to get re-marked as
                                    // dirty in case it's written to whilst we read it.
                                    chunk_trackers[device_number].clear_chunk(index);

                                    let chunk = sources[device_number].get_chunk(index as u64 * manifest.jobs[device_number].chunk_size as u64, manifest.jobs[device_number].chunk_size);
                                    let mut message = Some( (device_number, chunk) );

                                    'write_try_loop: loop {
                                        update_chunk_trackers(&mut chunk_trackers);
                                        handle_management_tickets(&mut cancelled, &mut paused, &chunk_trackers);
                                        if cancelled {
                                            break 'consistency_loop;
                                        }

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
                        } // <- if paused {...} else >>>{...}<<<

                        if last_progress_update.elapsed() >= config.progress_update_period {
                            if config.exclusive_progress_updates {
                                std::io::stdout().write_all(b"\x1b[2J").unwrap();
                            }
                            for i in 0..number_of_devices {
                                println!("Copying '{}' to '{}'\nProcessing as {} chunks of size {}\n{}", source_paths[i].display(), destination_paths[i].display(), chunk_trackers[i].get_chunk_count(), manifest.jobs[i].chunk_size, chunk_trackers[i].summary_report(&config, display_detail));
                            }
                            println!(
                                "Done {}{}   Dirty {}{}   Unprocessed {}{}   UnprocessedDirty {}{}",
                                config.diagram_cells[0], config.diagram_cells_reset,
                                config.diagram_cells[1], config.diagram_cells_reset,
                                config.diagram_cells[2], config.diagram_cells_reset,
                                config.diagram_cells[3], config.diagram_cells_reset
                            );
                            println!("Chunk writes: {}", total_writes);
                            last_progress_update = Instant::now();
                        }
                        handle_management_tickets(&mut cancelled, &mut paused, &chunk_trackers);
                        if cancelled {
                            break 'consistency_loop;
                        }
                    } // <- 'device_copy_loop loop
                } // <- for device_number in 0..number_of_devices
            } // <- while still_copying
            first_go = false;
        } // <- while !consistent
        if !cancelled {
            println!("Copying complete!");
            println!("Chunk writes: {} (efficiency is {})", total_writes, total_chunk_count as f64 / total_writes as f64);
        } else {
            println!("Copying aborted!");
        }
    }).unwrap();

    println!("All copier threads finished");

    Ok(())
}
