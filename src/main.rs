extern crate trackup;
extern crate clap;

use std::path::{Path,PathBuf};
use std::time::Duration;
use trackup::config::Config;
use trackup::job::Job;
use trackup::control::ManagementInterface;
use trackup::control::Manifest;

fn main() {
    let matches = clap::App::new(env!("CARGO_PKG_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .author(env!("CARGO_PKG_AUTHORS"))
        .about("Snapshotless device backup")
        .arg(
            clap::Arg::with_name("copy")
                .short("f")
                .long("copy")
                .value_names(&["SOURCE", "DESTINATION"])
                .help("Input block device and output file or block device to copy. Can be specified multiple times to copy multiple block devices.")
                .takes_value(true)
                .number_of_values(2)
                .multiple(true)
                .required_unless("daemon")
        )
        .arg(
            clap::Arg::with_name("chunk-size")
                .short("c")
                .long("chunk-size")
                .value_name("CHUNK_SIZE")
                .help("Granularity of modification tracking")
                .takes_value(true)
                .required(true)
        )
        .arg(
            clap::Arg::with_name("tracing-path")
                .short("t")
                .long("tracing-path")
                .value_name("TRACING_PATH")
                .help("Path to kernel tracing directory within a debugfs")
                .takes_value(true)
                .default_value("/sys/kernel/debug/tracing")
        )
        .arg(
            clap::Arg::with_name("sys-path")
                .short("s")
                .long("sys-path")
                .value_name("SYS_PATH")
                .help("Path to sysfs")
                .takes_value(true)
                .default_value("/sys")
        )
        .arg(
            clap::Arg::with_name("trace-buffer-size")
                .short("b")
                .long("trace-buffer-size")
                .value_name("BUFFER_SIZE_KB")
                .help("Per-CPU size of kernel tracing buffer in KB")
                .takes_value(true)
                .default_value("8192")
        )
        .arg(
            clap::Arg::with_name("progress-period")
                .short("p")
                .long("progress-period")
                .value_name("SECONDS")
                .help("Time in seconds between progress updates")
                .takes_value(true)
                .default_value("5")
        )
        .arg(
            clap::Arg::with_name("max-diagram-size")
                .short("d")
                .long("max-diagram-size")
                .value_name("SECONDS")
                .help("Maximum number of characters to use for progress diagrams")
                .takes_value(true)
                .default_value("1024")
        )
        .arg(
            clap::Arg::with_name("exclusive-progress-updates")
                .short("x")
                .long("exclusive-progress-updates")
                .help("Clear screen before each progress update")
                .takes_value(false)
        )
        .arg(
            clap::Arg::with_name("reuse")
                .short("r")
                .long("reuse")
                .help("Write over an existing output file/device (requires the file to be present). By default, any existing file will be deleted, and a new file will be pre-allocated.")
                .takes_value(false)
        )
        .arg(
            clap::Arg::with_name("color")
                .short("C")
                .long("color")
                .help("Display diagrams in color")
                .takes_value(false)
        )
        .arg(
            clap::Arg::with_name("management-socket")
                .short("m")
                .long("management-socket")
                .value_name("SOCKET_PATH")
                .help("Unix socket to use for management")
                .takes_value(true)
        )
        .arg(
            clap::Arg::with_name("daemon")
                .short("D")
                .long("daemon")
                .help("Start a backup daemon")
                .takes_value(false)
                .requires("management-socket")
        )
        .get_matches();

    let chunk_size: usize = matches.value_of("chunk-size").unwrap().parse().unwrap();
    let reuse_output = matches.is_present("reuse");

    let mut copy_it = matches.values_of("copy").unwrap();
    let mut jobs = Vec::new();
    while let (Some(source), Some(destination)) = (copy_it.next(), copy_it.next()) {
        jobs.push(Job {
            source: PathBuf::from(source),
            destination: PathBuf::from(destination),
            chunk_size,
            reuse_output,
        });
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
            &trackup::config::COLOR_DIAGRAM_CELLS
        } else {
            &trackup::config::PLAIN_DIAGRAM_CELLS
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
