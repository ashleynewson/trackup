extern crate trackup;
extern crate clap;

use std::path::PathBuf;
use std::time::Duration;
use trackup::config::Config;
use trackup::job::Job;

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
                .required(true)
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
                .help("Maximum number of characters to use for progress diagram")
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
        .get_matches();

    let mut copy_it = matches.values_of("copy").unwrap();
    let mut jobs = Vec::new();
    while let (Some(source), Some(destination)) = (copy_it.next(), copy_it.next()) {
        jobs.push(Job {
            source: PathBuf::from(source),
            destination: PathBuf::from(destination),
        });
    }

    let chunk_size: usize = matches.value_of("chunk-size").unwrap().parse().unwrap();
    let tracing_path = PathBuf::from(matches.value_of("tracing-path").unwrap());
    let sys_path = PathBuf::from(matches.value_of("sys-path").unwrap());
    let trace_buffer_size: usize = matches.value_of("trace-buffer-size").unwrap().parse().unwrap();
    let progress_update_period = Duration::from_secs(matches.value_of("progress-period").unwrap().parse().unwrap());
    let exclusive_progress_updates = matches.is_present("exclusive-progress-updates");
    let max_diagram_size: usize = matches.value_of("max-diagram-size").unwrap().parse().unwrap();
    let reuse_output = matches.is_present("reuse");

    let config = Config {
        jobs: &jobs,
        chunk_size,
        tracing_path: tracing_path.as_path(),
        sys_path: sys_path.as_path(),
        trace_buffer_size,
        progress_update_period,
        exclusive_progress_updates,
        max_diagram_size,
        reuse_output,
    };

    if let Err(_) = trackup::backup_device(&config) {
        eprintln!("Backup failed");
    }
}
