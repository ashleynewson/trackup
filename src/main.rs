extern crate trackup;
extern crate clap;

use std::path::{Path,PathBuf};
use std::time::Duration;
use trackup::control::{Config,Job,ManagementInterface,Manifest};

fn main() {
    let app = trackup::cli::get_app();
    let matches = app.get_matches();

    let chunk_size: usize = matches.value_of("chunk-size").unwrap().parse().unwrap();
    let reuse_output = matches.is_present("reuse");

    let mut jobs = Vec::new();
    if let Some(mut copy_it) = matches.values_of("copy") {
        while let (Some(source), Some(destination)) = (copy_it.next(), copy_it.next()) {
            jobs.push(Job {
                source: PathBuf::from(source),
                destination: PathBuf::from(destination),
                chunk_size,
                reuse_output,
            });
        }
    }

    let tracing_path = PathBuf::from(matches.value_of("tracing-path").unwrap());
    let sys_path = PathBuf::from(matches.value_of("sys-path").unwrap());
    let trace_buffer_size: usize = matches.value_of("trace-buffer-size").unwrap().parse().unwrap();
    let progress_update_period = Duration::from_secs(matches.value_of("progress-period").unwrap().parse().unwrap());
    let exclusive_progress_updates = matches.is_present("exclusive-progress-updates");
    let max_diagram_size: usize = matches.value_of("max-diagram-size").unwrap().parse().unwrap();
    let color_mode = matches.is_present("color");
    let daemon_mode = matches.is_present("daemon");

    let diagram_cells =
        if color_mode {
            &trackup::control::COLOR_DIAGRAM_CELLS
        } else {
            &trackup::control::PLAIN_DIAGRAM_CELLS
        }
        .iter()
        .map(|x| {String::from(*x)})
        .collect();

    let config = Config {
        tracing_path,
        sys_path,
        trace_buffer_size,
        progress_update_period,
        exclusive_progress_updates,
        max_diagram_size,
        diagram_cells,
        diagram_cells_reset: String::from(if color_mode {"\x1b[m"} else {""}),
    };
    let manifest = Manifest {
        jobs,
        do_sync: true,
        command_locks: Vec::new(),
        file_locks: Vec::new(),
        lock_time_limit: std::time::Duration::new(0, 0),
        lock_cooldown: std::time::Duration::new(0, 0),
    };

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
