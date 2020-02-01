extern crate trackup;
extern crate clap;

use std::path::{Path,PathBuf};
use std::time::Duration;
use trackup::control::{Job,ManagementInterface,Manifest};
use trackup::control::interface::Internalize;

fn main() {
    let app = trackup::cli::get_app();
    let matches = app.get_matches();

    let mut config = if let Some(config_path) = matches.value_of("config") {
        match trackup::control::interface::read_config_file(Path::new(config_path)) {
            Ok(config) => config,
            Err(e) => {
                panic!("Failed to load config file: {}", e);
            },
        }
    } else {
        trackup::control::interface::Config::default().internalize().unwrap()
    };

    if let Some(tracing_path) = matches.value_of("tracing-path") {
        config.tracing_path = PathBuf::from(tracing_path);
    }
    if let Some(sys_path) = matches.value_of("sys-path") {
        config.sys_path = PathBuf::from(sys_path);
    }
    if let Some(trace_buffer_size) = matches.value_of("trace-buffer-size") {
        config.trace_buffer_size = trace_buffer_size.parse().expect("Could not parse trace-buffer-size as usize integer");
    }
    if matches.is_present("progress-period")
        || matches.is_present("max-diagram-size")
        || matches.is_present("exclusive-progress-updates")
        || matches.is_present("color")
    {
        if config.progress_logging.is_none() {
            config.progress_logging = Some(trackup::control::interface::ProgressLogging::default().internalize().unwrap());
        }
        let progress_logging = config.progress_logging.as_mut().unwrap();
        if let Some(update_period) = matches.value_of("progress-period") {
            progress_logging.update_period = Duration::from_secs(update_period.parse().unwrap());
        }
        if matches.is_present("exclusive-progress-updates") {
            progress_logging.exclusive = true;
        }
        if let Some(max_diagram_size) = matches.value_of("max-diagram-size") {
            progress_logging.max_diagram_size = max_diagram_size.parse().unwrap();
        }
        if matches.is_present("color") {
            progress_logging.diagram_cells =
                trackup::control::COLOR_DIAGRAM_CELLS
                .iter()
                .map(|x| {String::from(*x)})
                .collect();
            progress_logging.diagram_cells_reset = String::from("\x1b[m");
        }
    }

    let manifest =
        if let Some(manifest_path) = matches.value_of("manifest") {
            match trackup::control::interface::read_manifest_file(Path::new(manifest_path)) {
                Ok(manifest) => manifest,
                Err(e) => {
                    panic!("Failed to load manifest file: {}", e);
                },
            }
        } else {
            let chunk_size: usize = matches.value_of("chunk-size").unwrap().parse().unwrap();
            let reuse_output = matches.is_present("reuse");

            let mut jobs = Vec::new();
            if let Some(mut copy_it) = matches.values_of("copy") {
                while let (Some(source), Some(destination)) = (copy_it.next(), copy_it.next()) {
                    jobs.push(Job {
                        source: PathBuf::from(source),
                        storage: trackup::control::Storage {
                            destination: PathBuf::from(destination),
                            storage_policy: trackup::control::StoragePolicy::Full,
                            format: trackup::control::StorageFormat::Raw,
                        },
                        checksum: None,
                        chunk_size,
                        reuse_output,
                    });
                }
            }

            Manifest {
                jobs,
                do_sync: true,
                locking: None,
                state_path: None,
                parent_state_path: None,
                store_path: None,
            }
        };

    let daemon_mode = matches.is_present("daemon");

    nix::sys::mman::mlockall(nix::sys::mman::MlockAllFlags::all()).expect("Could not mlock pages in RAM. (Are you root?)");

    let management_interface =
        match matches.value_of("management-socket") {
            Some(path) => {
                trackup::server::start_server(Path::new(path))
            },
            None => {
                ManagementInterface::new(None)
            },
        };

    if daemon_mode {
        let optional_manifest =
            if manifest.jobs.len() > 0 {
                Some(manifest)
            } else {
                None
            };
        trackup::server::task_loop(&config, &management_interface, optional_manifest);
    } else {
        eprintln!("Starting backup");
        if let Err(e) = trackup::copier::run(&config, &manifest, &management_interface) {
            eprintln!("Backup failed: {:?}", e);
        }
    }
}
